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
    std::process::exit(run_cli(std::env::args().skip(1)));
}

fn run_cli<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let command = parse_cli(args);
    match command {
        Ok(Command::Service { config }) => {
            #[cfg(windows)]
            {
                let Some(config) = config else {
                    eprintln!("SCM entry requires --config <absolute>");
                    return 2;
                };
                if let Err(error) = ramshared_winbroker::service::set_service_config(config)
                    .and_then(|()| {
                        ramshared_winbroker::service::dispatch().map_err(|e| e.to_string())
                    })
                {
                    eprintln!("SCM dispatch failed: {error}");
                    return 2;
                }
                0
            }
            #[cfg(not(windows))]
            {
                let _ = config;
                eprintln!("RamSharedBroker SCM entry requires Windows");
                return 2;
            }
        }
        Ok(Command::Console { config }) => {
            let bytes = match std::fs::read(&config) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("config read failed: {error}");
                    return 2;
                }
            };
            let config = match ramshared_winbroker::BrokerConfigV1::from_toml(&bytes) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("config invalid: {error}");
                    return 2;
                }
            };
            #[cfg(windows)]
            {
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                if let Err(error) = ramshared_winbroker::service::run_console(config, stop) {
                    eprintln!("console failed: {error}");
                    return 3;
                }
                0
            }
            #[cfg(not(windows))]
            {
                let _ = config;
                eprintln!("RamSharedBroker console entry requires Windows");
                return 2;
            }
        }
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli;


    #[test]
    fn run_cli_tests() {
        use super::run_cli;
        assert_eq!(run_cli(vec!["unknown_cmd"]), 2);

        #[cfg(not(windows))]
        {
            assert_eq!(run_cli(vec!["--config", "/absolute.toml"]), 2);
            assert_eq!(run_cli(vec!["console", "--config", "/absolute.toml"]), 2);
        }
    }

    #[test]
    fn cli_accepts_absolute_config() {
        #[cfg(windows)]
        let valid = "C:\\broker.toml";
        #[cfg(not(windows))]
        let valid = "/broker.toml";

        assert_eq!(
            parse_cli(vec!["--config", valid]).unwrap(),
            super::Command::Service { config: Some(std::path::PathBuf::from(valid)) }
        );
        assert_eq!(
            parse_cli(vec!["console", "--config", valid]).unwrap(),
            super::Command::Console { config: std::path::PathBuf::from(valid) }
        );
        assert_eq!(
            parse_cli(std::iter::empty::<&str>()).unwrap(),
            super::Command::Service { config: None }
        );
        assert!(parse_cli(vec!["unknown_cmd"]).is_err());
    }

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
