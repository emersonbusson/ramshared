use std::time::Duration;
use ramshared_broker::model::TransportKind;

pub struct Config {
    pub broker: String,
    pub tenant: String,
    pub swap_prio: Option<i32>,
    pub nbd_base: String,
    pub transport: TransportKind,
    pub watchdog: Duration,
    pub status_only: bool,
}

pub enum ParsedArgs {
    Help,
    Config(Config),
}

pub enum CliExit {
    Help,
    Usage(String),
    Runtime(String),
}

pub fn usage() -> String {
    "Usage:\n  \
     ramshared-agent --broker HOST:PORT --tenant NAME [--swap-prio P] \
     [--nbd-base /dev/nbd] [--transport tcp|unix] [--watchdog-secs 90]\n  \
     ramshared-agent --broker HOST:PORT --status"
        .to_string()
}

pub fn usage_diagnostic(message: &str) -> String {
    let usage = usage();
    if message.ends_with(&usage) {
        message.to_string()
    } else {
        format!("{message}\n{usage}")
    }
}

pub fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut broker = None;
    let mut tenant = None;
    let mut swap_prio = None;
    let mut nbd_base = "/dev/nbd".to_string();
    let mut transport = TransportKind::NbdTcp;
    let mut watchdog = Duration::from_secs(90);
    let mut status_only = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut take = |name: &str| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "--broker" => broker = Some(take("--broker")?),
            "--tenant" => tenant = Some(take("--tenant")?),
            "--swap-prio" => {
                let v = take("--swap-prio")?;
                swap_prio = Some(
                    v.parse()
                        .map_err(|_| format!("--swap-prio is invalid: {v}"))?,
                );
            }
            "--nbd-base" => nbd_base = take("--nbd-base")?,
            "--transport" => {
                transport = match take("--transport")?.as_str() {
                    "tcp" => TransportKind::NbdTcp,
                    "unix" => TransportKind::NbdUnix,
                    other => return Err(format!("--transport is invalid: {other} (use tcp|unix)")),
                };
            }
            "--watchdog-secs" => {
                let v = take("--watchdog-secs")?;
                let s: u64 = v
                    .parse()
                    .map_err(|_| format!("--watchdog-secs is invalid: {v}"))?;
                watchdog = Duration::from_secs(s);
            }
            "--status" => status_only = true,
            "-h" | "--help" => return Ok(ParsedArgs::Help),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }

    Ok(ParsedArgs::Config(Config {
        broker: broker.ok_or_else(|| format!("--broker is required\n{}", usage()))?,
        tenant: tenant.unwrap_or_default(),
        swap_prio,
        nbd_base,
        transport,
        watchdog,
        status_only,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn parse_config(a: &[&str]) -> Config {
        match parse_args(&args(a)) {
            Ok(ParsedArgs::Config(c)) => c,
            Ok(ParsedArgs::Help) => panic!("expected Config, got Help"),
            Err(e) => panic!("expected Config, got error: {}", e),
        }
    }

    #[test]
    fn parse_minimal_agent() {
        let c = parse_config(&["--broker", "10.0.0.1:7000", "--tenant", "wsl2"]);
        assert_eq!(c.broker, "10.0.0.1:7000");
        assert_eq!(c.tenant, "wsl2");
        assert_eq!(c.nbd_base, "/dev/nbd");
        assert!(matches!(c.transport, TransportKind::NbdTcp));
        assert_eq!(c.watchdog, Duration::from_secs(90));
        assert!(!c.status_only);
        assert!(c.swap_prio.is_none());
    }

    #[test]
    fn parse_full_flags() {
        let c = parse_config(&[
            "--broker",
            "h:1",
            "--tenant",
            "t",
            "--swap-prio",
            "-3",
            "--nbd-base",
            "/dev/nbd",
            "--transport",
            "unix",
            "--watchdog-secs",
            "30",
        ]);
        assert_eq!(c.swap_prio, Some(-3));
        assert!(matches!(c.transport, TransportKind::NbdUnix));
        assert_eq!(c.watchdog, Duration::from_secs(30));
    }

    #[test]
    fn status_mode_needs_no_tenant() {
        let c = parse_config(&["--broker", "h:1", "--status"]);
        assert!(c.status_only);
        assert!(c.tenant.is_empty());
    }

    #[test]
    fn missing_broker_errors() {
        assert!(parse_args(&args(&["--tenant", "x"])).is_err());
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--bogus"])).is_err());
    }

    #[test]
    fn bad_transport_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--transport", "rdma"])).is_err());
    }

    #[test]
    fn bad_swap_prio_errors() {
        assert!(parse_args(&args(&["--broker", "h:1", "--swap-prio", "x"])).is_err());
    }

    #[test]
    fn flag_without_value_errors() {
        assert!(parse_args(&args(&["--broker"])).is_err());
    }

    #[test]
    fn help_is_a_parse_outcome() {
        assert!(matches!(
            parse_args(&args(&["--help"])),
            Ok(ParsedArgs::Help)
        ));
    }

    #[test]
    fn usage_diagnostic_adds_usage_once() {
        let usage = usage();
        assert_eq!(
            usage_diagnostic("invalid input"),
            format!("invalid input\n{usage}")
        );
        assert_eq!(usage_diagnostic(&usage), usage);
    }
}
