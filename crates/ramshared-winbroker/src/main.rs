use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Service { config: Option<PathBuf> },
    Console { config: PathBuf },
}

fn is_absolute_config(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    value.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

fn parse_cli<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    match args.as_slice() {
        [] => Ok(Command::Service { config: None }),
        [flag, config] if flag == "--config" => {
            let path = PathBuf::from(config);
            if !is_absolute_config(&path) {
                return Err("--config must be absolute".into());
            }
            Ok(Command::Service { config: Some(path) })
        }
        [command, flag, config] if command == "console" && flag == "--config" => {
            let path = PathBuf::from(config);
            if !is_absolute_config(&path) {
                return Err("--config must be absolute".into());
            }
            Ok(Command::Console { config: path })
        }
        _ => Err("usage: ramshared-winbroker [console --config <absolute>]".into()),
    }
}

fn main() {
    let command = parse_cli(std::env::args().skip(1));
    match command {
        Ok(Command::Service { config }) => {
            #[cfg(windows)]
            {
                let Some(config) = config else {
                    eprintln!("SCM entry requires --config <absolute>");
                    std::process::exit(2);
                };
                if let Err(error) = ramshared_winbroker::service::set_service_config(config)
                    .and_then(|()| {
                        ramshared_winbroker::service::dispatch().map_err(|e| e.to_string())
                    })
                {
                    eprintln!("SCM dispatch failed: {error}");
                    std::process::exit(2);
                }
            }
            #[cfg(not(windows))]
            {
                let _ = config;
                eprintln!("RamSharedBroker SCM entry requires Windows");
                std::process::exit(2);
            }
        }
        Ok(Command::Console { config }) => {
            let bytes = match std::fs::read(&config) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("config read failed: {error}");
                    std::process::exit(2);
                }
            };
            let config = match ramshared_winbroker::BrokerConfigV1::from_toml(&bytes) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("config invalid: {error}");
                    std::process::exit(2);
                }
            };
            #[cfg(windows)]
            {
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                if let Err(error) = ramshared_winbroker::service::run_console(config, stop) {
                    eprintln!("console failed: {error}");
                    std::process::exit(3);
                }
            }
            #[cfg(not(windows))]
            {
                let _ = config;
                eprintln!("RamSharedBroker console entry requires Windows");
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli;

    #[test]
    fn cli_rejects_relative_config() {
        assert!(parse_cli(["console", "--config", "broker.toml"]).is_err());
        assert!(parse_cli(["--config", "broker.toml"]).is_err());
    }

    #[test]
    fn cli_has_no_tcp_listen_option() {
        assert!(parse_cli(["console", "--listen", "127.0.0.1:7700"]).is_err());
    }

    #[test]
    fn cli_has_no_install_mutation() {
        assert!(parse_cli(["install", "--config", r"C:\broker.toml"]).is_err());
    }
}
