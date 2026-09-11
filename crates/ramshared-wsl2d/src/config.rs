use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Nbd,
    Ublk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Vram,
    Vulkan,
    Ram,
}

pub const GIB: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_SIZE: u64 = 256 * 1024 * 1024;
pub const DEFAULT_ORIGIN_SIZE: u64 = 4 * GIB;
pub const MIN_ORIGIN_LOGICAL_SIZE: u64 = GIB;
pub const MAX_ORIGIN_LOGICAL_SIZE: u64 = 24 * GIB;
pub const BLOCK_SIZE: u32 = 4096;
pub const ORIGIN_MANIFEST_PATH: &str = "/etc/ramshared/origin.conf";

pub fn documented_private_listener_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            a == 127
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 100 && (64..=127).contains(&b))
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.octets()[0] & 0xfe == 0xfc,
    }
}

pub fn parse_private_listen(s: &str) -> Result<SocketAddr, String> {
    let raw = s.strip_prefix("tcp://").unwrap_or(s);
    let addr: SocketAddr = raw
        .parse()
        .map_err(|_| format!("invalid address '{s}' (use IP:PORT)"))?;
    if !documented_private_listener_ip(addr.ip()) {
        return Err(format!(
            "bind on {} refused — RNF-2 permits only loopback, RFC1918, IPv6 ULA, or Tailscale 100.64.0.0/10",
            addr.ip()
        ));
    }
    Ok(addr)
}

pub fn validate_slice_flags(slices: u16, slice_mb: u64, is_ublk: bool) -> Result<(), String> {
    if slices > 0 && is_ublk {
        return Err(
            "--slices does not combine with --transport ublk (DT-3: ublk single-device on WSL2)"
                .into(),
        );
    }
    if slices > 0 && slice_mb == 0 {
        return Err("--slices > 0 requires --slice-mb N".into());
    }
    if slices > ramshared_broker::slices::MAX_SLICES {
        return Err(format!(
            "--slices exceeds protocol limit ({})",
            ramshared_broker::slices::MAX_SLICES
        ));
    }
    Ok(())
}

pub struct AppArgs {
    pub size: u64,
    pub origin: Option<String>,
    pub sock: String,
    pub force: bool,
    pub nbd_dev: String,
    pub transport: Transport,
    pub queue_depth: u16,
    pub backend: BackendKind,
    pub slices: u16,
    pub slice_bytes: u64,
    pub listen_nbd_addr: Option<SocketAddr>,
    pub arbiter_addr: Option<SocketAddr>,
    pub advertise_tcp: Option<(String, u16)>,
    pub telemetry_jsonl: Option<std::path::PathBuf>,
}

impl AppArgs {
    pub fn parse_from(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut size = DEFAULT_SIZE;
        let mut size_explicit = false;
        let mut origin = None;
        let mut sock = "/run/ramshared/wsl2d.sock".to_string();
        let mut force = false;
        let mut nbd_dev = "/dev/nbd0".to_string();
        let mut transport = Transport::Nbd;
        let mut queue_depth = 1u16;
        let mut backend = BackendKind::Vram;
        let mut slices = 0u16;
        let mut slice_mb = 0u64;
        let mut listen_nbd: Option<String> = None;
        let mut arbiter: Option<String> = None;
        let mut advertise_nbd: Option<String> = None;
        let mut telemetry_jsonl: Option<String> = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--size" => {
                    i += 1;
                    let mb: u64 = args
                        .get(i)
                        .ok_or("--size requires a value (MiB)")?
                        .parse()?;
                    size = mb
                        .checked_mul(1024 * 1024)
                        .ok_or("--size: MiB value overflow")?;
                    size_explicit = true;
                }
                "--sock" => {
                    i += 1;
                    sock = args.get(i).ok_or("--sock requires a path")?.clone();
                }
                "--origin-manifest" => {
                    i += 1;
                    origin = Some(
                        args.get(i)
                            .ok_or("--origin-manifest requires a path")?
                            .clone(),
                    );
                }
                "--origin" => {
                    return Err("--origin is unsafe; use the sealed --origin-manifest path".into());
                }
                "--force" => force = true,
                "--nbd" => {
                    i += 1;
                    nbd_dev = args.get(i).ok_or("--nbd requires a path")?.clone();
                }
                "--transport" => {
                    i += 1;
                    transport = match args.get(i).map(String::as_str) {
                        Some("nbd") => Transport::Nbd,
                        Some("ublk") => Transport::Ublk,
                        _ => return Err("--transport requires 'nbd' or 'ublk'".into()),
                    };
                }
                "--queue-depth" => {
                    i += 1;
                    queue_depth = args
                        .get(i)
                        .ok_or("--queue-depth requires a value")?
                        .parse()
                        .map_err(|_| "--queue-depth is invalid")?;
                }
                "--backend" => {
                    i += 1;
                    backend = match args.get(i).map(String::as_str) {
                        Some("vram") => BackendKind::Vram,
                        Some("vulkan") => BackendKind::Vulkan,
                        Some("ram") => BackendKind::Ram,
                        _ => return Err("--backend requires 'vram', 'vulkan', or 'ram'".into()),
                    };
                }
                "--slices" => {
                    i += 1;
                    slices = args
                        .get(i)
                        .ok_or("--slices requires a value")?
                        .parse()
                        .map_err(|_| "--slices is invalid")?;
                }
                "--slice-mb" => {
                    i += 1;
                    slice_mb = args
                        .get(i)
                        .ok_or("--slice-mb requires a value (MiB)")?
                        .parse()
                        .map_err(|_| "--slice-mb is invalid")?;
                }
                "--listen-nbd" => {
                    i += 1;
                    listen_nbd = Some(
                        args.get(i)
                            .ok_or("--listen-nbd requires tcp://IP:PORT")?
                            .clone(),
                    );
                }
                "--arbiter-listen" => {
                    i += 1;
                    arbiter = Some(
                        args.get(i)
                            .ok_or("--arbiter-listen requires IP:PORT")?
                            .clone(),
                    );
                }
                "--advertise-nbd" => {
                    i += 1;
                    advertise_nbd = Some(
                        args.get(i)
                            .ok_or("--advertise-nbd requires HOST:PORT")?
                            .clone(),
                    );
                }
                "--telemetry-jsonl" => {
                    i += 1;
                    telemetry_jsonl = Some(
                        args.get(i)
                            .ok_or("--telemetry-jsonl requires a path")?
                            .clone(),
                    );
                }
                other => return Err(format!("unknown argument: {other}").into()),
            }
            i += 1;
        }
        if origin.is_some() && !size_explicit {
            size = DEFAULT_ORIGIN_SIZE;
        }
        size -= size % BLOCK_SIZE as u64; // align to the block size
        if origin.is_some() && !(MIN_ORIGIN_LOGICAL_SIZE..=MAX_ORIGIN_LOGICAL_SIZE).contains(&size)
        {
            return Err("origin-cache logical size must be between 1024 and 24576 MiB".into());
        }
        if origin.as_deref().is_some_and(|p| p != ORIGIN_MANIFEST_PATH) {
            return Err(format!(
                "--origin-manifest must use the sealed {ORIGIN_MANIFEST_PATH} path"
            )
            .into());
        }

        validate_slice_flags(slices, slice_mb, matches!(transport, Transport::Ublk))?;

        let listen_nbd_addr = listen_nbd
            .as_deref()
            .map(parse_private_listen)
            .transpose()?;
        let arbiter_addr = arbiter.as_deref().map(parse_private_listen).transpose()?;
        let advertise_nbd_addr = advertise_nbd
            .as_deref()
            .map(parse_private_listen)
            .transpose()?;

        if advertise_nbd_addr.is_some() && listen_nbd_addr.is_none() {
            return Err(
                "--advertise-nbd requires --listen-nbd (cannot advertise an unserved endpoint)"
                    .into(),
            );
        }

        if slices > 0 && arbiter_addr.is_none() {
            return Err("--slices requires --arbiter-listen IP:PORT (broker control point)".into());
        }
        if slices == 0 && (arbiter_addr.is_some() || listen_nbd_addr.is_some()) {
            return Err("--arbiter-listen/--listen-nbd require --slices N (N > 0)".into());
        }

        let advertise_tcp = advertise_nbd_addr
            .or(listen_nbd_addr)
            .map(|a| (a.ip().to_string(), a.port()));
        let telemetry_jsonl = telemetry_jsonl.map(std::path::PathBuf::from);

        let slice_bytes = if slices > 0 {
            slice_mb
                .checked_mul(1024 * 1024)
                .ok_or("--slice-mb: MiB value overflow")?
        } else {
            0
        };

        Ok(Self {
            size,
            origin,
            sock,
            force,
            nbd_dev,
            transport,
            queue_depth,
            backend,
            slices,
            slice_bytes,
            listen_nbd_addr,
            arbiter_addr,
            advertise_tcp,
            telemetry_jsonl,
        })
    }
}

pub fn daemon_version_requested(args: &[String]) -> bool {
    matches!(args, [_, flag] if flag == "--version" || flag == "-V" || flag == "version")
}

pub enum DaemonAction {
    Broker(AppArgs),
    Nbd(AppArgs),
    Ublk(AppArgs),
}

pub trait DaemonActionRunner {
    fn execute(&mut self, action: DaemonAction) -> Result<(), Box<dyn std::error::Error>>;
}

pub fn select_daemon_action(args: AppArgs) -> Result<DaemonAction, Box<dyn std::error::Error>> {
    if args.slices > 0 && args.origin.is_some() {
        return Err("--origin-manifest is valid only for the single NBD product path".into());
    }
    if args.slices > 0 && args.arbiter_addr.is_none() {
        return Err("--slices requires --arbiter-listen IP:PORT (broker control point)".into());
    }
    if args.slices > 0 {
        return Ok(DaemonAction::Broker(args));
    }
    if args.arbiter_addr.is_some() || args.listen_nbd_addr.is_some() {
        return Err("--arbiter-listen/--listen-nbd require --slices N (N > 0)".into());
    }
    match (args.transport, args.backend) {
        (Transport::Nbd, BackendKind::Ram) => Err(
            "--backend ram has no single NBD path; use --slices (broker) or ublk".into(),
        ),
        (Transport::Ublk, BackendKind::Vulkan) => Err(
            "ublk with --backend vulkan is not supported (DT-11); use --backend vram, or Vulkan via --slices / --transport nbd"
                .into(),
        ),
        (Transport::Nbd, _) if args.origin.is_some() => Ok(DaemonAction::Nbd(args)),
        (Transport::Nbd, _) => {
            Err(format!(
                "product NBD requires --origin-manifest {ORIGIN_MANIFEST_PATH}"
            )
            .into())
        }
        (Transport::Ublk, _) if args.origin.is_some() => {
            Err("--origin-manifest is valid only with --transport nbd".into())
        }
        (Transport::Ublk, _) => Ok(DaemonAction::Ublk(args)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn daemon_argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn private_listener_accepts_only_documented_untrusted_network_ranges() {
        assert!(parse_private_listen("127.0.0.1:8080").is_ok());
        assert!(parse_private_listen("tcp://10.0.0.1:8080").is_ok());
        assert!(parse_private_listen("172.16.0.1:8080").is_ok());
        assert!(parse_private_listen("172.31.255.255:8080").is_ok());
        assert!(parse_private_listen("192.168.1.1:8080").is_ok());
        assert!(parse_private_listen("100.64.0.1:8080").is_ok());
        assert!(parse_private_listen("100.127.255.255:8080").is_ok());
        assert!(parse_private_listen("[::1]:8080").is_ok());
        assert!(parse_private_listen("[fd00::1]:8080").is_ok());

        assert!(parse_private_listen("8.8.8.8:8080").is_err());
        assert!(parse_private_listen("172.15.255.255:8080").is_err());
        assert!(parse_private_listen("172.32.0.1:8080").is_err());
        assert!(parse_private_listen("192.169.1.1:8080").is_err());
        assert!(parse_private_listen("100.63.255.255:8080").is_err());
        assert!(parse_private_listen("100.128.0.1:8080").is_err());
        assert!(parse_private_listen("[2001:4860:4860::8888]:8080").is_err());
    }

    #[test]
    fn private_listen_rejects_garbage() {
        assert!(parse_private_listen("localhost:8080").is_err());
        assert!(parse_private_listen("127.0.0.1").is_err());
        assert!(parse_private_listen("tcp://").is_err());
    }

    #[test]
    fn slice_flags_reject_ublk_with_slices() {
        assert!(validate_slice_flags(1, 10, true).is_err());
        assert!(validate_slice_flags(1, 10, false).is_ok());
    }

    #[test]
    fn slice_flags_require_slice_mb() {
        assert!(validate_slice_flags(1, 0, false).is_err());
        assert!(validate_slice_flags(0, 0, false).is_ok());
    }

    #[test]
    fn slice_flags_cap_protects_status_line() {
        assert!(validate_slice_flags(257, 10, false).is_err());
        assert!(validate_slice_flags(256, 10, false).is_ok());
    }

    #[test]
    fn daemon_version_flag_is_side_effect_free() {
        assert!(daemon_version_requested(&daemon_argv(&[
            "ramsharedd",
            "--version"
        ])));
        assert!(daemon_version_requested(&daemon_argv(&[
            "ramsharedd",
            "-V"
        ])));
        assert!(daemon_version_requested(&daemon_argv(&[
            "ramsharedd",
            "version"
        ])));
        assert!(!daemon_version_requested(&daemon_argv(&[
            "ramsharedd",
            "--size",
            "100"
        ])));
    }

    #[test]
    fn daemon_args_refuse_invalid_or_unsafe_combinations_before_backend() {
        for argv in [
            daemon_argv(&["ramsharedd", "--unknown"]),
            daemon_argv(&["ramsharedd", "--slices", "1"]),
            daemon_argv(&[
                "ramsharedd",
                "--slices",
                "1",
                "--slice-mb",
                "1",
                "--arbiter-listen",
                "0.0.0.0:7777",
            ]),
            daemon_argv(&["ramsharedd", "--slices", "257", "--slice-mb", "1"]),
            daemon_argv(&["ramsharedd", "--advertise-nbd", "127.0.0.1:10809"]),
            daemon_argv(&[
                "ramsharedd",
                "--transport",
                "ublk",
                "--slices",
                "1",
                "--slice-mb",
                "1",
            ]),
        ] {
            assert!(
                AppArgs::parse_from(&argv).is_err(),
                "unsafe argv unexpectedly parsed: {argv:?}"
            );
        }
    }

    #[test]
    fn daemon_args_cover_flag_boundaries_before_backend() {
        let parsed = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--size",
            "3",
            "--sock",
            "/tmp/ramsharedd-boundary.sock",
            "--force",
            "--nbd",
            "/dev/nbd77",
            "--transport",
            "nbd",
            "--queue-depth",
            "2",
            "--backend",
            "vram",
        ]))
        .unwrap();

        assert_eq!(parsed.size, 3 * 1024 * 1024);
        assert_eq!(parsed.sock, "/tmp/ramsharedd-boundary.sock");
        assert!(parsed.force);
        assert_eq!(parsed.nbd_dev, "/dev/nbd77");
        assert_eq!(parsed.transport, Transport::Nbd);
        assert_eq!(parsed.queue_depth, 2);
        assert_eq!(parsed.backend, BackendKind::Vram);
    }

    #[test]
    fn daemon_plan_routes_validated_actions_without_starting_a_backend() {
        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--origin-manifest",
            ORIGIN_MANIFEST_PATH,
        ]))
        .unwrap();
        assert!(matches!(select_daemon_action(args).unwrap(), DaemonAction::Nbd(_)));

        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--transport",
            "ublk",
            "--backend",
            "vram",
        ]))
        .unwrap();
        assert!(matches!(select_daemon_action(args).unwrap(), DaemonAction::Ublk(_)));

        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--slices",
            "1",
            "--slice-mb",
            "1",
            "--arbiter-listen",
            "127.0.0.1:7777",
        ]))
        .unwrap();
        assert!(matches!(
            select_daemon_action(args).unwrap(),
            DaemonAction::Broker(_)
        ));

        let args = AppArgs::parse_from(&daemon_argv(&[
            "ramsharedd",
            "--transport",
            "ublk",
            "--backend",
            "vulkan",
        ]))
        .unwrap();
        assert!(select_daemon_action(args).is_err());
    }
}
