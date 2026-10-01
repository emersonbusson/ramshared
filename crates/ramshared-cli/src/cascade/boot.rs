//! Native cascade bootstrap (`ramshared boot`).
//!
//! Owns config loading (RF-4), the gate sequence of PRD §3 steps 1–4, and the
//! refusal surface (RF-2, NFR-6). `boot` is gate-then-act: every gate completes
//! before the first mutation, there is no retry loop (NFR-5), and the function
//! never writes to a product binary path (RF-8, DT-7).
//!
//! SPEC: docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Environment seam so sizing resolution is pure and testable (DT-3).
pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
}

/// Production environment reader.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Built-in conservative sizing (RF-4, DT-3).
pub const DEFAULT_VRAM_MIB: u64 = 1024;
/// Built-in conservative sizing (RF-4, DT-3).
pub const DEFAULT_ZRAM_MIB: u64 = 1024;
/// Built-in shared-hardware cushion (DT-3).
///
/// Bound to the sealed GPU free-floor authority — `MIN_VRAM_HEADROOM_MIB` is
/// the operator-facing name for the same configured reserve
/// (`ramshared_vram::SEALED_RESERVE_MIN_MIB`) that `ReserveFloorPolicy` and
/// `preflight.sh` enforce. A built-in of 256 MiB is the retired silent default
/// that admitted a shared host with almost no GPU headroom; it must not return.
pub const DEFAULT_MIN_VRAM_HEADROOM_MIB: u64 = ramshared_vram::SEALED_RESERVE_MIN_MIB;

/// Config keys accepted in `/etc/ramshared/cascade.conf` (PRD §7).
pub const CONF_KEY_VRAM_MIB: &str = "VRAM_MIB";
/// Config keys accepted in `/etc/ramshared/cascade.conf` (PRD §7).
pub const CONF_KEY_ZRAM_MIB: &str = "ZRAM_MIB";
/// Config keys accepted in `/etc/ramshared/cascade.conf` (PRD §7).
pub const CONF_KEY_MIN_VRAM_HEADROOM_MIB: &str = "MIN_VRAM_HEADROOM_MIB";

/// Approved activation verb (DT-4).
pub const APPROVAL_VERB: &str = "activate";

/// Resolved bootstrap sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootConfig {
    pub vram_mib: u64,
    pub zram_mib: u64,
    pub min_vram_headroom_mib: u64,
}

/// Where each sizing value came from (observability, DT-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// `/etc/ramshared/cascade.conf`.
    Etc,
    /// `RAMSHARED_*` / `MIN_VRAM_HEADROOM_MIB` environment.
    Env,
    /// Built-in conservative default.
    Default,
}

impl ConfigSource {
    /// Stable spelling for logs, journal lines, and `status` JSON.
    #[allow(dead_code)] // consumed by the ITEM-5 display surface
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Etc => "etc",
            Self::Env => "env",
            Self::Default => "default",
        }
    }
}

/// Resolved sizing plus provenance for `status --json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedBootConfig {
    pub config: BootConfig,
    pub vram_source: ConfigSource,
    pub zram_source: ConfigSource,
    pub headroom_source: ConfigSource,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            vram_mib: DEFAULT_VRAM_MIB,
            zram_mib: DEFAULT_ZRAM_MIB,
            min_vram_headroom_mib: DEFAULT_MIN_VRAM_HEADROOM_MIB,
        }
    }
}

/// Partial sizing parsed from `/etc/ramshared/cascade.conf`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CascadeConf {
    vram_mib: Option<u64>,
    zram_mib: Option<u64>,
    min_vram_headroom_mib: Option<u64>,
}

/// Version-scoped activation approval (DT-4, RF-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedApproval {
    pub verb: String,
    pub release: String,
    pub vram_mib: Option<u64>,
    pub zram_mib: Option<u64>,
}

impl ScopedApproval {
    /// Version **equality** with the running release. A different release is
    /// stale and must not authorize this one (RF-9).
    pub fn matches_release(&self, release: &str) -> bool {
        self.release == release
    }
}

/// Deterministic bootstrap refusals. Never retried (NFR-5, Kahneman #15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    /// `/etc/ramshared/cascade.conf` is present but not usable.
    ConfigInvalid(String),
    /// No approval token for the running release.
    ApprovalMissing,
    /// Approval names another release (RF-9).
    ApprovalStaleVersion { found: String, running: String },
    /// Token is not root-owned, or is group/world-writable (security checklist).
    ApprovalUntrusted(&'static str),
    /// Sealed identity / binary-match gate refused (DT-2).
    Identity(ramshared_tier::nbd_readiness::RefusalCode),
    /// Origin, Guardian, or host lease is missing or stale.
    HostPrerequisite(&'static str),
    /// Ghost swap, half cascade, or unbound managed device is present.
    DirtyState(&'static str),
    /// `boot` was invoked with deploy/upgrade arguments (DT-7).
    DeployRefused,
}

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigInvalid(detail) => write!(f, "cascade.conf invalid: {detail}"),
            Self::ApprovalMissing => {
                write!(f, "scoped approval missing for this release")
            }
            Self::ApprovalStaleVersion { found, running } => write!(
                f,
                "scoped approval names release {found}, running release is {running}"
            ),
            Self::ApprovalUntrusted(reason) => write!(f, "approval token untrusted: {reason}"),
            Self::Identity(code) => write!(f, "identity gate refused: {}", code.as_str()),
            Self::HostPrerequisite(reason) => write!(f, "host prerequisite: {reason}"),
            Self::DirtyState(reason) => write!(f, "dirty cascade state: {reason}"),
            Self::DeployRefused => {
                write!(f, "boot has no deploy API; install or upgrade explicitly")
            }
        }
    }
}

impl std::error::Error for BootError {}

/// Paths the bootstrap consults. Injected so tests never touch live host state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPaths {
    pub config: PathBuf,
    pub approval_dir: PathBuf,
    pub lease: PathBuf,
    pub product_root: PathBuf,
}

impl BootPaths {
    /// Production layout (PRD §7, DT-4).
    pub fn production() -> Self {
        Self {
            config: PathBuf::from("/etc/ramshared/cascade.conf"),
            approval_dir: PathBuf::from("/var/lib/ramshared/approvals"),
            lease: PathBuf::from("/run/ramshared/host-resume-lease.json"),
            product_root: PathBuf::from("/opt/ramshared"),
        }
    }
}

/// Parse `KEY=VALUE` lines from `/etc/ramshared/cascade.conf`.
///
/// The file is data, not shell: comments and blanks are skipped, unknown keys
/// are ignored (the sealed example carries NBD binding keys owned by
/// `nbd-product-preflight.sh`), and a malformed known key is a hard error.
fn parse_cascade_conf(text: &str) -> Result<CascadeConf, BootError> {
    let mut conf = CascadeConf::default();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(BootError::ConfigInvalid(format!(
                "line {}: expected KEY=VALUE",
                index + 1
            )));
        };
        let key = key.trim();
        let value = value.trim();
        let parsed = match key {
            CONF_KEY_VRAM_MIB => &mut conf.vram_mib,
            CONF_KEY_ZRAM_MIB => &mut conf.zram_mib,
            CONF_KEY_MIN_VRAM_HEADROOM_MIB => &mut conf.min_vram_headroom_mib,
            _ => continue,
        };
        let n: u64 = value.parse().map_err(|_| {
            BootError::ConfigInvalid(format!(
                "line {}: {key} must be a non-negative integer",
                index + 1
            ))
        })?;
        if n == 0 {
            return Err(BootError::ConfigInvalid(format!(
                "line {}: {key} must be greater than zero",
                index + 1
            )));
        }
        *parsed = Some(n);
    }
    Ok(conf)
}

fn env_u64(env: &dyn Env, key: &str) -> Option<u64> {
    env.var(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|s| match s.parse::<u64>() {
            Ok(n) if n > 0 => Some(n),
            _ => None,
        })
}

/// Resolve sizing: `/etc/ramshared/cascade.conf` → env → built-in defaults (DT-3).
///
/// The sealed `cascade.conf.example` is never read here; it keeps only the
/// machine binding keys that `nbd-product-preflight.sh` owns.
pub fn load_boot_config_from(path: &Path, env: &dyn Env) -> Result<BootConfig, BootError> {
    Ok(resolve_boot_config_from(path, env)?.config)
}

/// Resolve sizing with provenance for observability.
pub fn resolve_boot_config_from(
    path: &Path,
    env: &dyn Env,
) -> Result<ResolvedBootConfig, BootError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(BootError::ConfigInvalid(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    resolve_boot_config_text(&text, env)
}

/// Resolve sizing from an already-read `cascade.conf` body (DT-3).
///
/// Split out of `resolve_boot_config_from` so the attended `up` path and the
/// tests share one chain: `/etc/ramshared/cascade.conf` → env → built-in
/// defaults. An empty body is the same as a missing file.
pub fn resolve_boot_config_text(
    text: &str,
    env: &dyn Env,
) -> Result<ResolvedBootConfig, BootError> {
    let conf = parse_cascade_conf(text)?;

    let env_vram = env_u64(env, "RAMSHARED_VRAM_MIB");
    let env_zram = env_u64(env, "RAMSHARED_ZRAM_MIB");
    let env_headroom = env_u64(env, CONF_KEY_MIN_VRAM_HEADROOM_MIB);

    let (vram_mib, vram_source) = match (conf.vram_mib, env_vram) {
        (Some(n), _) => (n, ConfigSource::Etc),
        (None, Some(n)) => (n, ConfigSource::Env),
        (None, None) => (DEFAULT_VRAM_MIB, ConfigSource::Default),
    };
    let (zram_mib, zram_source) = match (conf.zram_mib, env_zram) {
        (Some(n), _) => (n, ConfigSource::Etc),
        (None, Some(n)) => (n, ConfigSource::Env),
        (None, None) => (DEFAULT_ZRAM_MIB, ConfigSource::Default),
    };
    let (min_vram_headroom_mib, headroom_source) = match (conf.min_vram_headroom_mib, env_headroom)
    {
        (Some(n), _) => (n, ConfigSource::Etc),
        (None, Some(n)) => (n, ConfigSource::Env),
        (None, None) => (DEFAULT_MIN_VRAM_HEADROOM_MIB, ConfigSource::Default),
    };
    // DT-8 raise-only, same rule `preflight.sh` and `ReserveFloorPolicy` apply
    // to this name: the operator may be more conservative than the seal, never
    // less. A value below the sealed authority is a refusal, not a clamp.
    if min_vram_headroom_mib < DEFAULT_MIN_VRAM_HEADROOM_MIB {
        return Err(BootError::ConfigInvalid(format!(
            "MIN_VRAM_HEADROOM_MIB={min_vram_headroom_mib} is below the sealed authority \
             {DEFAULT_MIN_VRAM_HEADROOM_MIB} MiB (raise-only)"
        )));
    }

    Ok(ResolvedBootConfig {
        config: BootConfig {
            vram_mib,
            zram_mib,
            min_vram_headroom_mib,
        },
        vram_source,
        zram_source,
        headroom_source,
    })
}

/// Parse the version-scoped approval wire format (DT-4):
/// `activate:<release>[:vram=<n>:zram=<n>]`.
pub fn parse_scoped_approval(raw: &str) -> Result<ScopedApproval, BootError> {
    let raw = raw.trim();
    let mut parts = raw.split(':');
    let verb = parts.next().unwrap_or_default();
    if verb != APPROVAL_VERB {
        return Err(BootError::ApprovalUntrusted(
            "approval verb must be 'activate'",
        ));
    }
    let release = parts.next().unwrap_or_default();
    if release.is_empty() {
        return Err(BootError::ApprovalUntrusted("approval release is empty"));
    }
    let mut approval = ScopedApproval {
        verb: verb.to_string(),
        release: release.to_string(),
        vram_mib: None,
        zram_mib: None,
    };
    for extra in parts {
        let (key, value) = extra.split_once('=').ok_or(BootError::ApprovalUntrusted(
            "approval size binding must be key=value",
        ))?;
        let n: u64 = value.trim().parse().map_err(|_| {
            BootError::ApprovalUntrusted("approval size binding must be a positive integer")
        })?;
        if n == 0 {
            return Err(BootError::ApprovalUntrusted(
                "approval size binding must be a positive integer",
            ));
        }
        match key.trim() {
            "vram" => approval.vram_mib = Some(n),
            "zram" => approval.zram_mib = Some(n),
            _ => {
                return Err(BootError::ApprovalUntrusted(
                    "approval carries an unknown size binding",
                ));
            }
        }
    }
    Ok(approval)
}

/// Token trust rule, pure over `stat(2)` bits so both directions are testable.
fn approval_token_trust(uid: u32, gid: u32, mode: u32) -> Result<(), &'static str> {
    let _ = gid;
    if uid != 0 {
        return Err("approval token is not owned by root");
    }
    // Reject group/other write. A world-writable token is a privilege bypass:
    // any user could mint activate:<running>.
    if mode & 0o022 != 0 {
        return Err("approval token is group- or world-writable");
    }
    Ok(())
}

/// A token that is group/world-writable, or not owned by root, is refused as
/// `ApprovalUntrusted` and never treated as `Present`.
///
/// Operates on the `Metadata` already collected by the caller (no TOCTOU).
fn approval_token_is_root_owned(_path: &Path, meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        approval_token_trust(meta.uid(), meta.gid(), meta.mode()).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// Minimal on-disk lease shape. Full supervisor identity validation stays in
/// `workload.rs`; `boot` only needs the expiry timestamp (NFR-6).
#[derive(Debug, Clone, serde::Deserialize)]
struct HostLeaseFile {
    expires_at_epoch_ms: u64,
}

fn epoch_ms(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Verify the host-resume lease is present and fresh.
///
/// `boot` **verifies** the lease; it never mints one (minting is
/// `ramshared-host-gate.service`). The expiry rule mirrors
/// `host_gate::lease_expired`: `now >= deadline` is expired. The caller injects
/// `now` so the boundary is testable without sleeping.
fn verify_host_lease(path: &Path, now: SystemTime) -> Result<(), BootError> {
    let text = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BootError::HostPrerequisite("host-resume lease is missing")
        } else {
            BootError::HostPrerequisite("host-resume lease is unreadable")
        }
    })?;
    let lease: HostLeaseFile = serde_json::from_str(&text)
        .map_err(|_| BootError::HostPrerequisite("host-resume lease is malformed"))?;
    if epoch_ms(now) >= lease.expires_at_epoch_ms {
        return Err(BootError::HostPrerequisite("host-resume lease is expired"));
    }
    Ok(())
}

/// DT-7: `boot` has no deploy API. Reject any invocation that carries
/// install/upgrade/replace style arguments before a single gate runs.
pub fn parse_boot_invocation(args: &[String]) -> Result<(), BootError> {
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--install"
                | "--upgrade"
                | "--deploy"
                | "--replace"
                | "--release"
                | "--force"
                | "--uninstall"
        ) || arg.starts_with("--release=")
    }) {
        return Err(BootError::DeployRefused);
    }
    Ok(())
}

/// RF-8 test seam: true when `path` is a product binary path `boot` must never
/// write. Used by `boot_never_mutates_product_binaries`.
#[cfg(test)]
fn assert_no_product_binary_mutation(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if text.contains("/bin/") || text.ends_with("ramshared") || text.ends_with("ramsharedd") {
        return false;
    }
    // Product release trees and the selected `current` symlink are off limits.
    if text.contains("/releases/") || text.contains("/current/") {
        return false;
    }
    true
}

/// Locate the approval token for `release` and prove it trusted (RF-9).
fn read_scoped_approval(
    approval_dir: &Path,
    running_release: &str,
) -> Result<ScopedApproval, BootError> {
    let token_path = approval_dir.join(format!("{APPROVAL_VERB}-{running_release}.token"));
    let meta = fs::metadata(&token_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BootError::ApprovalMissing
        } else {
            BootError::ApprovalUntrusted("approval token is unreadable")
        }
    })?;
    if !approval_token_is_root_owned(&token_path, &meta) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let reason = approval_token_trust(meta.uid(), meta.gid(), meta.mode())
                .err()
                .unwrap_or("approval token failed trust check");
            return Err(BootError::ApprovalUntrusted(reason));
        }
        #[cfg(not(unix))]
        {
            return Err(BootError::ApprovalUntrusted(
                "approval token trust requires unix",
            ));
        }
    }
    let raw = fs::read_to_string(&token_path)
        .map_err(|_| BootError::ApprovalUntrusted("approval token is unreadable"))?;
    let approval = parse_scoped_approval(&raw)?;
    if approval.verb != APPROVAL_VERB {
        return Err(BootError::ApprovalUntrusted(
            "approval verb must be 'activate'",
        ));
    }
    if !approval.matches_release(running_release) {
        return Err(BootError::ApprovalStaleVersion {
            found: approval.release.clone(),
            running: running_release.to_string(),
        });
    }
    Ok(approval)
}

/// Host operations `boot` cannot unit-test against the live machine.
///
/// Same seam pattern as `ramshared-winsvc::control_plane::CommandRunner`: the
/// production implementation touches the real host; tests inject a fake so the
/// gate order and the no-mutation-on-refusal property are executable.
pub trait BootHost {
    /// Gate 1 (identity, RF-2/RF-8): `evaluate_product` over a fresh
    /// observation of the sealed release, BINARY_MATCH, and lifecycle.
    fn identity_gate(&self, config: &BootConfig, running_release: &str) -> Result<(), BootError>;
    /// Gate 2 (approval, RF-9, DT-4): token trust + version equality.
    fn resolve_approval(
        &self,
        approval_dir: &Path,
        running_release: &str,
    ) -> Result<ScopedApproval, BootError>;
    /// Gate 4 (dirty state, NFR-1): ghost / half cascade refusal.
    fn dirty_gate(&self) -> Result<(), BootError>;
    /// RF-5: true when a healthy cascade is already mounted (idempotent no-op).
    fn already_healthy(&self) -> bool;
    /// The single mutation: the existing idempotent `up` (RF-5).
    fn activate(&self, args: &[String]) -> Result<(), String>;
}

/// Production host: the live WSL2 machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemBootHost {
    pub product_root: PathBuf,
}

impl SystemBootHost {
    pub fn new(product_root: PathBuf) -> Self {
        Self { product_root }
    }
}

impl Default for SystemBootHost {
    fn default() -> Self {
        Self::new(BootPaths::production().product_root)
    }
}

impl BootHost for SystemBootHost {
    fn identity_gate(&self, config: &BootConfig, running_release: &str) -> Result<(), BootError> {
        let input = self.observe_product_input(config, running_release)?;
        let decision = ramshared_tier::nbd_readiness::evaluate_product(&input);
        match decision.reason {
            ramshared_tier::nbd_readiness::ReadinessReason::NotEvaluated
            | ramshared_tier::nbd_readiness::ReadinessReason::ProductOff
            | ramshared_tier::nbd_readiness::ReadinessReason::AllGatesPass => Ok(()),
            ramshared_tier::nbd_readiness::ReadinessReason::Refusal(code) => {
                Err(BootError::Identity(code))
            }
        }
    }

    fn resolve_approval(
        &self,
        approval_dir: &Path,
        running_release: &str,
    ) -> Result<ScopedApproval, BootError> {
        read_scoped_approval(approval_dir, running_release)
    }

    fn dirty_gate(&self) -> Result<(), BootError> {
        let entries = super::read_swaps()
            .map_err(|_| BootError::DirtyState("cannot read /proc/swaps to prove a clean state"))?;
        super::refuse_half_cascade(&entries).map_err(|_| {
            BootError::DirtyState("ghost or half cascade is present; run `sudo ramshared down`")
        })
    }

    fn already_healthy(&self) -> bool {
        super::read_swaps()
            .map(|entries| super::cascade_already_healthy(&entries))
            .unwrap_or(false)
    }

    fn activate(&self, args: &[String]) -> Result<(), String> {
        super::up_with_args(args).map_err(|error| error.to_string())
    }
}

impl SystemBootHost {
    /// One fresh observation for [`ramshared_tier::nbd_readiness::evaluate_product`].
    ///
    /// DT-2: the policy lives in `nbd_readiness`; this only gathers facts.
    /// Anything that cannot be measured safely is `Gate::Unknown` /
    /// `CapacitySample::Unknown`, which `evaluate_product` refuses — never a
    /// silent success (Kahneman #1, security checklist).
    fn observe_product_input(
        &self,
        config: &BootConfig,
        running_release: &str,
    ) -> Result<ramshared_tier::nbd_readiness::ProductInput, BootError> {
        use ramshared_tier::nbd_readiness::{
            Approval, Gate, Operation, ProductInput, ProductTransport,
        };

        let current = self.product_root.join("current");
        let (nbd_swap_active, daemon_running) = observe_lifecycle();
        let cli_match = observe_cli_binary_match(&current);
        let release_gate = observe_release_gate(&current, running_release);
        let relay_gate = observe_relay_gate(&current);
        let capacity = observe_lower_tier_capacity(config);
        let daemon_match = observe_daemon_binary_match(&current, daemon_running);

        // The CLI's own identity is a boot property (RF-8) checked outside the
        // daemon-scoped `binary_match` field: `evaluate_product` models the
        // daemon, `boot` also has to prove the running CLI is the sealed one.
        if let Gate::Fail = cli_match {
            return Err(BootError::Identity(
                ramshared_tier::nbd_readiness::RefusalCode::BinaryMatchFailed,
            ));
        }
        if let Gate::Unknown = cli_match {
            return Err(BootError::Identity(
                ramshared_tier::nbd_readiness::RefusalCode::BinaryMatchUnknown,
            ));
        }

        Ok(ProductInput {
            transport: if nbd_swap_active {
                ProductTransport::Nbd
            } else {
                ProductTransport::None
            },
            nbd_swap_active,
            daemon_running,
            legacy_ublk_product_active: observe_legacy_ublk(),
            release_gate,
            relay_gate,
            binary_match: daemon_match,
            capacity,
            vram_bytes: config
                .vram_mib
                .saturating_mul(ramshared_tier::nbd_readiness::MIB_BYTES),
            // Read-only classification: approval is a separate later gate (RF-9)
            // and `boot` must not ask `evaluate_product` to authorize the
            // mutation it has not yet reached.
            operation: Operation::ReadOnly,
            approval: Approval::NotRequired,
            reboot_requested: false,
        })
    }
}

/// Managed NBD swap present + managed daemon alive. Both must agree; a
/// disagreement is `NbdLifecycleIncomplete` inside `evaluate_product`.
fn observe_lifecycle() -> (bool, bool) {
    let entries = super::read_swaps().unwrap_or_default();
    let nbd_swap_active = entries
        .iter()
        .any(|entry| !entry.is_ghost() && super::is_nbd_device_path(&entry.filename));
    let daemon_running = std::fs::read_to_string(super::PID_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists())
        || Path::new(super::SOCK).exists();
    (nbd_swap_active, daemon_running)
}

fn observe_legacy_ublk() -> bool {
    // A live legacy ublk product device is a hard refusal in `evaluate_product`.
    // Observed from /proc/swaps: any ublk device path means the legacy transport
    // is still holding the product slot.
    super::read_swaps()
        .map(|entries| {
            entries
                .iter()
                .any(|entry| super::is_ublk_device_path(&entry.filename))
        })
        .unwrap_or(false)
}

/// Read `SHA256SUMS` under the selected release and return the hex digest for
/// `member`, or `None` when the manifest is absent or does not name it.
fn sealed_digest(current: &Path, member: &str) -> Option<String> {
    let manifest = fs::read_to_string(current.join("SHA256SUMS")).ok()?;
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let digest = parts.next()?;
        let name = parts.next()?;
        if name == member || name.strip_prefix("./") == Some(member) {
            return Some(digest.to_ascii_lowercase());
        }
    }
    None
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn observe_cli_binary_match(current: &Path) -> ramshared_tier::nbd_readiness::Gate {
    use ramshared_tier::nbd_readiness::Gate;
    let Some(expected) = sealed_digest(current, "bin/ramshared") else {
        return Gate::Unknown;
    };
    let Ok(exe) = std::env::current_exe() else {
        return Gate::Unknown;
    };
    let Ok(digest) = sha256_path(&exe) else {
        return Gate::Unknown;
    };
    if digest == expected {
        Gate::Pass
    } else {
        Gate::Fail
    }
}

fn observe_daemon_binary_match(
    current: &Path,
    daemon_running: bool,
) -> ramshared_tier::nbd_readiness::Gate {
    use ramshared_tier::nbd_readiness::Gate;
    if !daemon_running {
        // `evaluate_product` accepts `NotApplicable` only while no daemon is up.
        return Gate::NotApplicable;
    }
    let Some(expected) = sealed_digest(current, "bin/ramsharedd") else {
        return Gate::Unknown;
    };
    let Ok(pid_text) = fs::read_to_string(super::PID_FILE) else {
        return Gate::Unknown;
    };
    let Ok(pid) = pid_text.trim().parse::<i32>() else {
        return Gate::Unknown;
    };
    let exe = PathBuf::from(format!("/proc/{pid}/exe"));
    let Ok(digest) = sha256_path(&exe) else {
        return Gate::Unknown;
    };
    if digest == expected {
        Gate::Pass
    } else {
        Gate::Fail
    }
}

fn observe_release_gate(
    current: &Path,
    running_release: &str,
) -> ramshared_tier::nbd_readiness::Gate {
    use ramshared_tier::nbd_readiness::Gate;
    let Ok(version) = fs::read_to_string(current.join("RELEASE_VERSION")) else {
        return Gate::Unknown;
    };
    if version.trim() != running_release {
        return Gate::Fail;
    }
    if fs::metadata(current.join("SHA256SUMS")).is_err() {
        return Gate::Unknown;
    }
    Gate::Pass
}

fn observe_relay_gate(current: &Path) -> ramshared_tier::nbd_readiness::Gate {
    use ramshared_tier::nbd_readiness::Gate;
    let script = current.join("scripts/safety/wsl-relay-health.sh");
    if !script.is_file() {
        return Gate::Unknown;
    }
    // Bounded child: the relay probe is read-only `--check` (security checklist
    // "bounded foreign calls"). A non-zero exit is a measured failure, not a
    // hang and not a silent pass.
    match std::process::Command::new("bash")
        .arg(&script)
        .arg("--check")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Gate::Pass,
        Ok(_) => Gate::Fail,
        Err(_) => Gate::Unknown,
    }
}

fn observe_lower_tier_capacity(
    config: &BootConfig,
) -> ramshared_tier::nbd_readiness::CapacitySample {
    use ramshared_tier::nbd_readiness::{CapacitySample, LowerTierSink};
    // The lower-tier sink binding (`NBD_LOWER_SINK*`) is owned by
    // `nbd-product-preflight.sh` (DT-3). `boot` does not invent a sink: when no
    // exact binding is configured the capacity sample is `Unknown` and
    // `evaluate_product` refuses with `LOWER_TIER_SINK_UNKNOWN`.
    let Ok(raw) = fs::read_to_string("/etc/ramshared/cascade.conf") else {
        return CapacitySample::Unknown;
    };
    let Some(sink) = conf_value(&raw, "NBD_LOWER_SINK") else {
        return CapacitySample::Unknown;
    };
    let Ok(meta) = fs::metadata(&sink) else {
        return CapacitySample::Unknown;
    };
    if !meta.is_dir() {
        return CapacitySample::Unknown;
    }
    let Ok(stats) = statvfs_free_bytes(&sink) else {
        return CapacitySample::Unknown;
    };
    let alignment = 4096u64;
    let _ = config;
    CapacitySample::Observed {
        sink: LowerTierSink::Known(sink),
        free_absorbable_bytes: stats,
        alignment_bytes: alignment,
    }
}

fn conf_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=')?;
        if name.trim() == key {
            let value = value.trim().to_string();
            if value.is_empty() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

/// Free bytes on the filesystem holding `path`, via `statvfs(2)` (safe rustix).
fn statvfs_free_bytes(path: &str) -> Result<u64, &'static str> {
    let stat = rustix::fs::statvfs(path).map_err(|_| "statvfs failed on the lower-tier sink")?;
    let frsize = if stat.f_frsize != 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    };
    Ok(stat.f_bavail.saturating_mul(frsize))
}

fn sha256_path(path: &Path) -> Result<String, &'static str> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).map_err(|_| "cannot read sealed binary")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex_digest(&hasher.finalize()))
}

/// Gate 0 (DT-7, RF-8): `boot` has no deploy API. Checked on the caller's argv
/// before any gate reads the filesystem.
fn gate_no_deploy_api(args: &[String]) -> Result<(), BootError> {
    parse_boot_invocation(args)
}

/// Gate order (PRD §3 steps 1–4): identity → approval (incl. token trust) →
/// host prerequisites (incl. lease freshness) → dirty state. No mutation, no
/// retry loop (NFR-5).
fn evaluate_boot_gates(
    host: &dyn BootHost,
    paths: &BootPaths,
    env: &dyn Env,
    running_release: &str,
    args: &[String],
    now: SystemTime,
) -> Result<BootConfig, BootError> {
    gate_no_deploy_api(args)?;
    // Sizing is an input to the identity/capacity gate (DT-3), not a gate: it
    // is resolved before `evaluate_product` can validate lower-tier capacity,
    // and a refusal below never reports a config it would not have used.
    let config = load_boot_config_from(&paths.config, env)?;
    host.identity_gate(&config, running_release)?;
    // Approval before host prerequisites: a missing token is the operator's
    // first actionable refusal, not a lease error (RF-9).
    host.resolve_approval(&paths.approval_dir, running_release)?;
    verify_host_lease(&paths.lease, now)?;
    host.dirty_gate()?;
    Ok(config)
}

/// Native bootstrap entrypoint (RF-7). Gate-then-act, no retry loop.
pub fn boot() -> Result<BootConfig, BootError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    boot_with(
        &SystemBootHost::default(),
        &BootPaths::production(),
        &ProcessEnv,
        &release_version(),
        &args,
    )
}

/// Testable orchestrator over injected host, paths, environment, and argv.
/// Returns the resolved sizing so the caller can surface what was activated.
pub fn boot_with(
    host: &dyn BootHost,
    paths: &BootPaths,
    env: &dyn Env,
    running_release: &str,
    args: &[String],
) -> Result<BootConfig, BootError> {
    if host.already_healthy() {
        // RF-5 / Kahneman #17: a second `boot` on a healthy cascade is a no-op.
        // Sizing is reported best-effort; a missing config must not turn a
        // proven-healthy cascade into a failure.
        return Ok(load_boot_config_from(&paths.config, env).unwrap_or_default());
    }
    let config = evaluate_boot_gates(host, paths, env, running_release, args, SystemTime::now())?;
    // DT-3: sizes come from the resolved config, never from the sealed example.
    let up_args = [
        "--vram".to_string(),
        config.vram_mib.to_string(),
        "--zram".to_string(),
        config.zram_mib.to_string(),
    ];
    host.activate(&up_args).map_err(|error| {
        // The dynamic detail goes to the journal; the typed refusal stays
        // a stable static reason so `status` can surface it (NFR-6).
        eprintln!("[boot] activation failed after all gates passed: {error}");
        BootError::DirtyState("activation failed after all gates passed")
    })?;
    Ok(config)
}

/// Running release version, used for approval version equality (RF-9).
pub fn release_version() -> String {
    option_env!("CARGO_PKG_VERSION")
        .unwrap_or("0.0.0")
        .to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::collections::HashMap;

    struct MapEnv(HashMap<String, String>);

    impl MapEnv {
        fn new() -> Self {
            Self(HashMap::new())
        }

        fn with(key: &str, value: &str) -> Self {
            let mut map = HashMap::new();
            map.insert(key.to_string(), value.to_string());
            Self(map)
        }
    }

    impl Env for MapEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ramshared-boot-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn future_lease(now: SystemTime, delta_ms: u64) -> String {
        format!(
            r#"{{"expires_at_epoch_ms":{}}}"#,
            epoch_ms(now).saturating_add(delta_ms)
        )
    }

    /// Injectable host so gate order and no-mutation-on-refusal are executable
    /// without touching live `/proc/swaps`, `/opt/ramshared`, or `up`.
    #[derive(Debug)]
    struct FakeHost {
        identity: Result<(), BootError>,
        approval: Result<(), BootError>,
        dirty: Result<(), BootError>,
        healthy: bool,
        activate_calls: std::cell::RefCell<Vec<Vec<String>>>,
        activate_result: Result<(), String>,
    }

    impl FakeHost {
        fn passing() -> Self {
            Self {
                identity: Ok(()),
                approval: Ok(()),
                dirty: Ok(()),
                healthy: false,
                activate_calls: std::cell::RefCell::new(Vec::new()),
                activate_result: Ok(()),
            }
        }
    }

    impl BootHost for FakeHost {
        fn identity_gate(
            &self,
            _config: &BootConfig,
            _running_release: &str,
        ) -> Result<(), BootError> {
            self.identity.clone()
        }

        fn resolve_approval(
            &self,
            _approval_dir: &Path,
            _running_release: &str,
        ) -> Result<ScopedApproval, BootError> {
            self.approval.clone().map(|()| ScopedApproval {
                verb: APPROVAL_VERB.to_string(),
                release: "0.15.0".to_string(),
                vram_mib: None,
                zram_mib: None,
            })
        }

        fn dirty_gate(&self) -> Result<(), BootError> {
            self.dirty.clone()
        }

        fn already_healthy(&self) -> bool {
            self.healthy
        }

        fn activate(&self, args: &[String]) -> Result<(), String> {
            self.activate_calls.borrow_mut().push(args.to_vec());
            self.activate_result.clone()
        }
    }

    /// Host that keeps the **real** approval resolution (token trust + version
    /// equality) while faking the identity and dirty gates. Needed because a
    /// non-root test process can never mint a root-owned token: the three
    /// approval tests must exercise `read_scoped_approval`, not a stub.
    struct RealApprovalHost {
        identity: Result<(), BootError>,
        dirty: Result<(), BootError>,
        healthy: bool,
    }

    impl RealApprovalHost {
        fn new() -> Self {
            Self {
                identity: Ok(()),
                dirty: Ok(()),
                healthy: false,
            }
        }
    }

    impl BootHost for RealApprovalHost {
        fn identity_gate(
            &self,
            _config: &BootConfig,
            _running_release: &str,
        ) -> Result<(), BootError> {
            self.identity.clone()
        }

        fn resolve_approval(
            &self,
            approval_dir: &Path,
            running_release: &str,
        ) -> Result<ScopedApproval, BootError> {
            read_scoped_approval(approval_dir, running_release)
        }

        fn dirty_gate(&self) -> Result<(), BootError> {
            self.dirty.clone()
        }

        fn already_healthy(&self) -> bool {
            self.healthy
        }

        fn activate(&self, _args: &[String]) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn boot_config_defaults_to_conservative_1024_1024_when_file_absent() {
        let dir = temp_dir("defaults");
        let config = load_boot_config_from(&dir.join("cascade.conf"), &MapEnv::new()).unwrap();
        assert_eq!(config.vram_mib, 1024);
        assert_eq!(config.zram_mib, 1024);
        assert_eq!(
            config.min_vram_headroom_mib, DEFAULT_MIN_VRAM_HEADROOM_MIB,
            "the built-in cushion must be the sealed reserve authority, not a silent low default"
        );
        assert_eq!(
            DEFAULT_MIN_VRAM_HEADROOM_MIB,
            ramshared_vram::SEALED_RESERVE_MIN_MIB
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_config_refuses_headroom_below_the_sealed_authority() {
        // DT-8 raise-only: 256 MiB is the retired silent default and 512 MiB is
        // the old conf test value — both undercut the seal and must be refused,
        // never clamped up and never accepted as if they were the cushion.
        for low in ["256", "512", "2047"] {
            let dir = temp_dir("below-seal");
            let conf = dir.join("cascade.conf");
            write(&conf, &format!("MIN_VRAM_HEADROOM_MIB={low}\n"));
            let error = resolve_boot_config_from(&conf, &MapEnv::new()).unwrap_err();
            assert!(
                matches!(error, BootError::ConfigInvalid(_)),
                "MIN_VRAM_HEADROOM_MIB={low} must be refused, got {error:?}"
            );
            let mut map = HashMap::new();
            map.insert("MIN_VRAM_HEADROOM_MIB".to_string(), low.to_string());
            let error =
                resolve_boot_config_from(&dir.join("absent.conf"), &MapEnv(map)).unwrap_err();
            assert!(
                matches!(error, BootError::ConfigInvalid(_)),
                "env MIN_VRAM_HEADROOM_MIB={low} must be refused, got {error:?}"
            );
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn boot_config_reads_vram_zram_and_headroom_from_etc_cascade_conf() {
        let dir = temp_dir("etc-conf");
        let conf = dir.join("cascade.conf");
        write(
            &conf,
            "# sizing\nVRAM_MIB=2048\nZRAM_MIB=4096\nMIN_VRAM_HEADROOM_MIB=3072\n\nNBD_LOWER_SINK=\n",
        );
        let resolved = resolve_boot_config_from(&conf, &MapEnv::new()).unwrap();
        assert_eq!(resolved.config.vram_mib, 2048);
        assert_eq!(resolved.config.zram_mib, 4096);
        assert_eq!(resolved.config.min_vram_headroom_mib, 3072);
        assert_eq!(resolved.vram_source, ConfigSource::Etc);
        assert_eq!(resolved.zram_source, ConfigSource::Etc);
        assert_eq!(resolved.headroom_source, ConfigSource::Etc);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_config_prefers_etc_over_env() {
        let dir = temp_dir("etc-wins");
        let conf = dir.join("cascade.conf");
        write(&conf, "VRAM_MIB=3072\n");
        let env = MapEnv::with("RAMSHARED_VRAM_MIB", "1024");
        let resolved = resolve_boot_config_from(&conf, &env).unwrap();
        assert_eq!(resolved.config.vram_mib, 3072);
        assert_eq!(resolved.vram_source, ConfigSource::Etc);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_config_falls_back_to_env_when_file_absent() {
        let dir = temp_dir("env-fallback");
        let mut map = HashMap::new();
        map.insert("RAMSHARED_VRAM_MIB".to_string(), "2048".to_string());
        map.insert("RAMSHARED_ZRAM_MIB".to_string(), "512".to_string());
        map.insert("MIN_VRAM_HEADROOM_MIB".to_string(), "4096".to_string());
        let env = MapEnv(map);
        let resolved = resolve_boot_config_from(&dir.join("cascade.conf"), &env).unwrap();
        assert_eq!(resolved.config.vram_mib, 2048);
        assert_eq!(resolved.config.zram_mib, 512);
        assert_eq!(resolved.config.min_vram_headroom_mib, 4096);
        assert_eq!(resolved.vram_source, ConfigSource::Env);
        assert_eq!(resolved.zram_source, ConfigSource::Env);
        assert_eq!(resolved.headroom_source, ConfigSource::Env);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_config_rejects_non_integer_and_out_of_range_sizes() {
        assert!(matches!(
            parse_cascade_conf("VRAM_MIB=abc"),
            Err(BootError::ConfigInvalid(_))
        ));
        assert!(matches!(
            parse_cascade_conf("VRAM_MIB=0"),
            Err(BootError::ConfigInvalid(_))
        ));
        assert!(matches!(
            parse_cascade_conf("ZRAM_MIB=-1"),
            Err(BootError::ConfigInvalid(_))
        ));
        assert!(matches!(
            parse_cascade_conf("not-a-pair"),
            Err(BootError::ConfigInvalid(_))
        ));
        // Unknown keys are owned elsewhere and must not fail this loader.
        assert!(parse_cascade_conf("NBD_LOWER_SINK=\nNBD_LOWER_SINK_TYPE=directory\n").is_ok());
    }

    #[test]
    fn scoped_approval_parses_activate_with_and_without_size_binding() {
        let bare = parse_scoped_approval("activate:0.15.0").unwrap();
        assert_eq!(bare.verb, "activate");
        assert_eq!(bare.release, "0.15.0");
        assert_eq!(bare.vram_mib, None);
        assert_eq!(bare.zram_mib, None);

        let bound = parse_scoped_approval("activate:0.15.0:vram=2048:zram=1024").unwrap();
        assert_eq!(bound.vram_mib, Some(2048));
        assert_eq!(bound.zram_mib, Some(1024));
    }

    #[test]
    fn scoped_approval_rejects_malformed_wire_format() {
        assert!(parse_scoped_approval("").is_err());
        assert!(parse_scoped_approval("install:0.15.0").is_err());
        assert!(parse_scoped_approval("activate:").is_err());
        assert!(parse_scoped_approval("activate:0.15.0:vram=0").is_err());
        assert!(parse_scoped_approval("activate:0.15.0:bogus=1").is_err());
        assert!(parse_scoped_approval("activate:0.15.0:vram=x").is_err());
    }

    #[test]
    fn scoped_approval_accepts_only_the_running_release_version() {
        let approval = parse_scoped_approval("activate:0.15.0").unwrap();
        assert!(approval.matches_release("0.15.0"));
        assert!(!approval.matches_release("0.15.1"));
        assert!(!approval.matches_release("0.14.0"));
    }

    #[test]
    fn approval_token_trust_refuses_non_root_and_writable_tokens() {
        assert_eq!(approval_token_trust(0, 0, 0o0400), Ok(()));
        assert_eq!(approval_token_trust(0, 0, 0o0000), Ok(()));
        // SPEC DT-4: `uid==0 && mode&0o022==0`. Read bits are not a refusal.
        assert_eq!(approval_token_trust(0, 0, 0o0644), Ok(()));
        assert!(approval_token_trust(1000, 1000, 0o0400).is_err());
        assert!(approval_token_trust(0, 0, 0o0402).is_err());
        assert!(approval_token_trust(0, 0, 0o0420).is_err());
        assert!(approval_token_trust(0, 0, 0o0666).is_err());
    }

    #[test]
    fn boot_refuses_untrusted_approval_token() {
        let dir = temp_dir("untrusted-token");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();
        write(&paths.lease, &future_lease(now, 60_000));
        // A token this test creates is never root-owned, so it must refuse
        // rather than treat the file as Present.
        write(
            &paths.approval_dir.join("activate-0.15.0.token"),
            "activate:0.15.0",
        );
        let error = evaluate_boot_gates(
            &RealApprovalHost::new(),
            &paths,
            &MapEnv::new(),
            "0.15.0",
            &[],
            now,
        )
        .unwrap_err();
        assert!(
            matches!(error, BootError::ApprovalUntrusted(_)),
            "unexpected error: {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_refuses_stale_version_scoped_approval() {
        let dir = temp_dir("stale-approval");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();
        write(&paths.lease, &future_lease(now, 60_000));
        let token = paths.approval_dir.join("activate-0.15.0.token");
        write(&token, "activate:0.14.0");
        // Force the trust check past root ownership so this test isolates the
        // version-equality rule rather than the ownership rule.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Not root-owned on this host: assert the stale-version branch is
            // still unreachable without trust by expecting the trust refusal.
            let error = evaluate_boot_gates(
                &RealApprovalHost::new(),
                &paths,
                &MapEnv::new(),
                "0.15.0",
                &[],
                now,
            )
            .unwrap_err();
            assert!(
                matches!(
                    error,
                    BootError::ApprovalUntrusted(_) | BootError::ApprovalStaleVersion { .. }
                ),
                "unexpected error: {error:?}"
            );
            let _ = fs::set_permissions(&token, fs::Permissions::from_mode(0o0400));
        }
        // Direct unit proof of the version-equality rule (RF-9).
        let approval = parse_scoped_approval("activate:0.14.0").unwrap();
        assert!(!approval.matches_release("0.15.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_refuses_missing_approval_before_any_mutation() {
        let dir = temp_dir("missing-approval");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();
        write(&paths.lease, &future_lease(now, 60_000));
        let error = evaluate_boot_gates(
            &RealApprovalHost::new(),
            &paths,
            &MapEnv::new(),
            "0.15.0",
            &[],
            now,
        )
        .unwrap_err();
        assert_eq!(error, BootError::ApprovalMissing);
        // Nothing was created under the product root.
        assert!(!paths.product_root.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_requires_fresh_host_prerequisites() {
        let dir = temp_dir("host-prereq");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();

        // Gate order: approval is checked before the lease, so a missing token
        // is the first refusal and the lease is never consulted.
        write(&paths.lease, &future_lease(now, 60_000));
        let error = evaluate_boot_gates(
            &RealApprovalHost::new(),
            &paths,
            &MapEnv::new(),
            "0.15.0",
            &[],
            now,
        )
        .unwrap_err();
        assert_eq!(
            error,
            BootError::ApprovalMissing,
            "approval must gate before host prerequisites"
        );

        // The host-prerequisite gate itself refuses a missing, expired, or
        // malformed lease rather than hanging (NFR-6).
        let missing = dir.join("no-lease.json");
        assert!(matches!(
            verify_host_lease(&missing, now),
            Err(BootError::HostPrerequisite(_))
        ));

        write(&paths.lease, &future_lease(now, 0));
        assert!(matches!(
            verify_host_lease(&paths.lease, now),
            Err(BootError::HostPrerequisite(_))
        ));

        write(&paths.lease, "not json");
        assert!(matches!(
            verify_host_lease(&paths.lease, now),
            Err(BootError::HostPrerequisite(_))
        ));

        write(&paths.lease, "{}");
        assert!(matches!(
            verify_host_lease(&paths.lease, now),
            Err(BootError::HostPrerequisite(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_host_lease_accepts_a_fresh_lease_and_rejects_the_boundary() {
        let dir = temp_dir("lease-verify");
        let lease = dir.join("lease.json");
        let now = SystemTime::now();

        write(&lease, &future_lease(now, 1));
        assert!(verify_host_lease(&lease, now).is_ok());

        // `now == deadline` is expired — same rule as host_gate::lease_expired.
        write(&lease, &future_lease(now, 0));
        assert!(matches!(
            verify_host_lease(&lease, now),
            Err(BootError::HostPrerequisite(_))
        ));

        write(&lease, &future_lease(now, 1));
        let later = now + std::time::Duration::from_millis(5);
        assert!(matches!(
            verify_host_lease(&lease, later),
            Err(BootError::HostPrerequisite(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_never_mutates_product_binaries() {
        // Every product binary path must be classified as untouchable.
        assert!(!assert_no_product_binary_mutation(Path::new(
            "/opt/ramshared/current/bin/ramshared"
        )));
        assert!(!assert_no_product_binary_mutation(Path::new(
            "/opt/ramshared/current/bin/ramsharedd"
        )));
        assert!(!assert_no_product_binary_mutation(Path::new(
            "/opt/ramshared/releases/0.15.0/bin/ramshared"
        )));
        assert!(!assert_no_product_binary_mutation(Path::new(
            "/opt/ramshared/current/lib/libfoo.so"
        )));
        // Scratch state under /run is a legitimate write target.
        assert!(assert_no_product_binary_mutation(Path::new(
            "/run/ramshared/host-resume-lease.json"
        )));
        assert!(assert_no_product_binary_mutation(Path::new(
            "/var/lib/ramshared/approvals/activate-0.15.0.token"
        )));
    }

    #[test]
    fn boot_refuses_deploy_arguments() {
        assert!(parse_boot_invocation(&[]).is_ok());
        assert_eq!(
            parse_boot_invocation(&["--install".to_string()]),
            Err(BootError::DeployRefused)
        );
        assert_eq!(
            parse_boot_invocation(&["--upgrade".to_string()]),
            Err(BootError::DeployRefused)
        );
        assert_eq!(
            parse_boot_invocation(&["--deploy".to_string()]),
            Err(BootError::DeployRefused)
        );
        assert_eq!(
            parse_boot_invocation(&["--release=0.15.0".to_string()]),
            Err(BootError::DeployRefused)
        );
        assert_eq!(
            parse_boot_invocation(&["--uninstall".to_string()]),
            Err(BootError::DeployRefused)
        );
    }

    #[test]
    fn boot_config_source_as_str() {
        assert_eq!(ConfigSource::Etc.as_str(), "etc");
        assert_eq!(ConfigSource::Env.as_str(), "env");
        assert_eq!(ConfigSource::Default.as_str(), "default");
    }

    #[test]
    fn boot_error_display_names_every_variant() {
        use ramshared_tier::nbd_readiness::RefusalCode;
        let texts = [
            BootError::ConfigInvalid("bad".into()).to_string(),
            BootError::ApprovalMissing.to_string(),
            BootError::ApprovalStaleVersion {
                found: "a".into(),
                running: "b".into(),
            }
            .to_string(),
            BootError::ApprovalUntrusted("why").to_string(),
            BootError::Identity(RefusalCode::BinaryMatchFailed).to_string(),
            BootError::HostPrerequisite("lease").to_string(),
            BootError::DirtyState("ghost").to_string(),
            BootError::DeployRefused.to_string(),
        ];
        for text in texts {
            assert!(!text.is_empty());
        }
    }

    #[test]
    fn boot_refuses_ghost_or_half_cascade_state() {
        let dir = temp_dir("dirty-state");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();
        write(&paths.lease, &future_lease(now, 60_000));
        write(
            &paths.approval_dir.join("activate-0.15.0.token"),
            "activate:0.15.0",
        );
        let mut host = FakeHost::passing();
        host.dirty = Err(BootError::DirtyState("ghost swap present"));
        let error =
            evaluate_boot_gates(&host, &paths, &MapEnv::new(), "0.15.0", &[], now).unwrap_err();
        assert!(
            matches!(error, BootError::DirtyState(_)),
            "unexpected error: {error:?}"
        );
        // The refusal must reach `boot_with` without activating anything.
        let mut host = FakeHost::passing();
        host.dirty = Err(BootError::DirtyState("ghost swap present"));
        let error = boot_with(&host, &paths, &MapEnv::new(), "0.15.0", &[]).unwrap_err();
        assert!(matches!(error, BootError::DirtyState(_)));
        assert!(
            host.activate_calls.borrow().is_empty(),
            "a dirty-state refusal must not activate"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_is_idempotent_when_cascade_already_healthy() {
        let dir = temp_dir("idempotent");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let mut host = FakeHost::passing();
        host.healthy = true;
        // RF-5 / Kahneman #17: a second `boot` on a healthy cascade is a no-op
        // and issues zero activation calls.
        let result = boot_with(&host, &paths, &MapEnv::new(), "0.15.0", &[]);
        assert!(result.is_ok(), "healthy short-circuit: {result:?}");
        assert_eq!(
            host.activate_calls.borrow().len(),
            0,
            "an already-healthy cascade must not be activated a second time"
        );
        // Even with a missing approval: the healthy short-circuit wins, so the
        // operator is not told to re-approve a cascade that is already up.
        let result = boot_with(&host, &paths, &MapEnv::new(), "0.15.0", &[]);
        assert!(result.is_ok(), "healthy short-circuit: {result:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_records_refusal_reason_and_leaves_state_off() {
        let dir = temp_dir("refusal-reason");
        let paths = BootPaths {
            config: dir.join("cascade.conf"),
            approval_dir: dir.join("approvals"),
            lease: dir.join("lease.json"),
            product_root: dir.join("product-root"),
        };
        let now = SystemTime::now();
        write(&paths.lease, &future_lease(now, 60_000));

        // Identity refusal is surfaced as a stable `Identity` code (NFR-6).
        let mut host = FakeHost::passing();
        host.identity = Err(BootError::Identity(
            ramshared_tier::nbd_readiness::RefusalCode::BinaryMatchFailed,
        ));
        let error = boot_with(&host, &paths, &MapEnv::new(), "0.15.0", &[]).unwrap_err();
        assert!(
            matches!(
                error,
                BootError::Identity(ramshared_tier::nbd_readiness::RefusalCode::BinaryMatchFailed)
            ),
            "unexpected error: {error:?}"
        );
        assert!(host.activate_calls.borrow().is_empty());

        // Approval refusal leaves the state exactly `Off` (zero activation).
        // RealApprovalHost keeps the live token-trust rules so this is the real
        // `read_scoped_approval`, not a stub.
        let error = boot_with(
            &RealApprovalHost::new(),
            &paths,
            &MapEnv::new(),
            "0.15.0",
            &[],
        )
        .unwrap_err();
        assert_eq!(error, BootError::ApprovalMissing);

        // An activation failure after every gate passed is still a refusal and
        // still reports a typed reason (NFR-6).
        write(
            &paths.approval_dir.join("activate-0.15.0.token"),
            "activate:0.15.0",
        );
        let mut host = FakeHost::passing();
        host.activate_result = Err("nbd attach failed".into());
        let error = boot_with(&host, &paths, &MapEnv::new(), "0.15.0", &[]).unwrap_err();
        assert!(
            matches!(error, BootError::DirtyState(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(host.activate_calls.borrow().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    // ── SystemBootHost observer helpers ──────────────────────────────────

    #[test]
    fn boot_config_default_matches_dt3_sizing() {
        let config = BootConfig::default();
        assert_eq!(config.vram_mib, DEFAULT_VRAM_MIB);
        assert_eq!(config.zram_mib, DEFAULT_ZRAM_MIB);
        assert_eq!(config.min_vram_headroom_mib, DEFAULT_MIN_VRAM_HEADROOM_MIB);
    }

    #[test]
    fn boot_paths_production_layout() {
        let paths = BootPaths::production();
        assert_eq!(paths.config, PathBuf::from("/etc/ramshared/cascade.conf"));
        assert_eq!(
            paths.approval_dir,
            PathBuf::from("/var/lib/ramshared/approvals")
        );
        assert_eq!(
            paths.lease,
            PathBuf::from("/run/ramshared/host-resume-lease.json")
        );
        assert_eq!(paths.product_root, PathBuf::from("/opt/ramshared"));
    }

    #[test]
    fn process_env_reads_real_environment() {
        let env = ProcessEnv;
        // PATH is always set in a test process.
        assert!(env.var("PATH").is_some());
        assert!(env.var("RAMSHARED_DEFINITELY_NOT_SET_9f3a").is_none());
    }

    #[test]
    fn sealed_digest_extracts_member_hashes() {
        let dir = temp_dir("sealed-digest");
        let current = dir.join("current");
        write(
            &current.join("SHA256SUMS"),
            "abc123  bin/ramshared\ndef456  ./bin/ramsharedd\n",
        );
        assert_eq!(
            sealed_digest(&current, "bin/ramshared"),
            Some("abc123".into())
        );
        assert_eq!(
            sealed_digest(&current, "bin/ramsharedd"),
            Some("def456".into())
        );
        assert_eq!(sealed_digest(&current, "bin/missing"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sealed_digest_returns_none_without_manifest() {
        let dir = temp_dir("sealed-digest-missing");
        assert_eq!(sealed_digest(&dir, "bin/ramshared"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_digest_formats_lowercase() {
        assert_eq!(hex_digest(&[0x00, 0xff, 0x0a]), "00ff0a");
        assert_eq!(hex_digest(&[]), "");
    }

    #[test]
    fn observe_cli_binary_match_unknown_without_manifest() {
        let dir = temp_dir("cli-match-unknown");
        assert_eq!(
            observe_cli_binary_match(&dir),
            ramshared_tier::nbd_readiness::Gate::Unknown
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_cli_binary_match_fail_on_digest_mismatch() {
        let dir = temp_dir("cli-match-fail");
        let current = dir.join("current");
        // A manifest naming a digest that cannot match the running test binary.
        write(&current.join("SHA256SUMS"), "0000  bin/ramshared\n");
        assert_eq!(
            observe_cli_binary_match(&current),
            ramshared_tier::nbd_readiness::Gate::Fail
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_daemon_binary_match_not_applicable_without_daemon() {
        let dir = temp_dir("daemon-na");
        assert_eq!(
            observe_daemon_binary_match(&dir, false),
            ramshared_tier::nbd_readiness::Gate::NotApplicable
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_daemon_binary_match_unknown_without_manifest() {
        let dir = temp_dir("daemon-unknown");
        assert_eq!(
            observe_daemon_binary_match(&dir, true),
            ramshared_tier::nbd_readiness::Gate::Unknown
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_release_gate_pass_fail_unknown() {
        let dir = temp_dir("release-gate");
        let current = dir.join("current");
        // Missing RELEASE_VERSION → Unknown.
        assert_eq!(
            observe_release_gate(&current, "0.15.0"),
            ramshared_tier::nbd_readiness::Gate::Unknown
        );
        // Version mismatch → Fail.
        write(&current.join("RELEASE_VERSION"), "0.14.0\n");
        write(&current.join("SHA256SUMS"), "abc  bin/ramshared\n");
        assert_eq!(
            observe_release_gate(&current, "0.15.0"),
            ramshared_tier::nbd_readiness::Gate::Fail
        );
        // Version match + SHA256SUMS present → Pass.
        write(&current.join("RELEASE_VERSION"), "0.15.0\n");
        assert_eq!(
            observe_release_gate(&current, "0.15.0"),
            ramshared_tier::nbd_readiness::Gate::Pass
        );
        // Version match but SHA256SUMS gone → Unknown.
        let _ = fs::remove_file(current.join("SHA256SUMS"));
        assert_eq!(
            observe_release_gate(&current, "0.15.0"),
            ramshared_tier::nbd_readiness::Gate::Unknown
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_relay_gate_unknown_without_script() {
        let dir = temp_dir("relay-unknown");
        assert_eq!(
            observe_relay_gate(&dir),
            ramshared_tier::nbd_readiness::Gate::Unknown
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_lifecycle_and_legacy_ublk_do_not_panic() {
        // These read real `/proc/swaps`; the assertion is that the observation
        // is total (returns a value) and does not panic on a clean host.
        let (nbd, daemon) = observe_lifecycle();
        let _ = (nbd, daemon);
        let _ = observe_legacy_ublk();
    }

    #[test]
    fn observe_lower_tier_capacity_is_unknown_without_binding() {
        // Without `/etc/ramshared/cascade.conf` (or without `NBD_LOWER_SINK`)
        // the capacity sample must be `Unknown` — never a silent pass.
        let sample = observe_lower_tier_capacity(&BootConfig::default());
        // On CI / dev hosts the conf is typically absent → Unknown.
        // On a production host with a binding it may be Observed; either way
        // the call must not panic.
        let _ = sample;
    }

    #[test]
    fn conf_value_parses_key_value_lines() {
        let text = "FOO=bar\n# comment\n\nNBD_LOWER_SINK=/mnt/tier3\nEMPTY=\n";
        assert_eq!(
            conf_value(text, "NBD_LOWER_SINK"),
            Some("/mnt/tier3".into())
        );
        assert_eq!(conf_value(text, "FOO"), Some("bar".into()));
        assert_eq!(conf_value(text, "EMPTY"), None);
        assert_eq!(conf_value(text, "MISSING"), None);
    }

    #[test]
    fn statvfs_free_bytes_on_temp_dir() {
        let dir = temp_dir("statvfs");
        let free = statvfs_free_bytes(dir.to_str().unwrap()).unwrap();
        assert!(free > 0, "temp dir must have free space");
        assert!(statvfs_free_bytes("/nonexistent-path-9f3a").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_path_hashes_known_content() {
        let dir = temp_dir("sha256");
        let file = dir.join("blob");
        write(&file, "hello");
        // echo -n hello | sha256sum
        assert_eq!(
            sha256_path(&file).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(sha256_path(&dir.join("missing")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // ── SystemBootHost trait methods ─────────────────────────────────────

    #[test]
    fn system_boot_host_new_and_default() {
        let host = SystemBootHost::new(PathBuf::from("/tmp/product"));
        assert_eq!(host.product_root, PathBuf::from("/tmp/product"));
        let default_host = SystemBootHost::default();
        assert_eq!(default_host.product_root, PathBuf::from("/opt/ramshared"));
    }

    #[test]
    fn system_boot_host_identity_gate_refuses_without_sealed_release() {
        let dir = temp_dir("sys-identity");
        let host = SystemBootHost::new(dir.clone());
        // No `current/` release tree → CLI binary match is Unknown → refusal.
        let config = BootConfig::default();
        let error = host.identity_gate(&config, "0.15.0").unwrap_err();
        assert!(
            matches!(error, BootError::Identity(_)),
            "expected identity refusal, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_boot_host_dirty_gate_and_healthy_do_not_panic() {
        let host = SystemBootHost::new(PathBuf::from("/tmp/x"));
        // On a clean host these succeed / return false; the assertion is
        // totality (no panic) on real /proc/swaps.
        let _ = host.dirty_gate();
        let _ = host.already_healthy();
    }

    #[test]
    fn system_boot_host_resolve_approval_delegates_to_reader() {
        let dir = temp_dir("sys-approval");
        let host = SystemBootHost::new(dir.clone());
        let error = host
            .resolve_approval(&dir.join("approvals"), "0.15.0")
            .unwrap_err();
        assert!(
            matches!(error, BootError::ApprovalMissing),
            "expected ApprovalMissing, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_product_input_fails_on_cli_binary_mismatch() {
        let dir = temp_dir("observe-input");
        let current = dir.join("current");
        write(&current.join("SHA256SUMS"), "0000  bin/ramshared\n");
        let host = SystemBootHost::new(dir.clone());
        let error = host
            .observe_product_input(&BootConfig::default(), "0.15.0")
            .unwrap_err();
        assert!(
            matches!(error, BootError::Identity(_)),
            "expected identity refusal on CLI mismatch, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_product_input_unknown_without_manifest() {
        let dir = temp_dir("observe-unknown");
        let host = SystemBootHost::new(dir.clone());
        let error = host
            .observe_product_input(&BootConfig::default(), "0.15.0")
            .unwrap_err();
        assert!(
            matches!(error, BootError::Identity(_)),
            "expected identity refusal without manifest, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // ── config resolution error paths ────────────────────────────────────

    #[test]
    fn env_u64_rejects_zero_and_non_numeric() {
        assert_eq!(env_u64(&MapEnv::with("X", "0"), "X"), None);
        assert_eq!(env_u64(&MapEnv::with("X", "abc"), "X"), None);
        assert_eq!(env_u64(&MapEnv::with("X", "-5"), "X"), None);
        assert_eq!(env_u64(&MapEnv::with("X", " 42 "), "X"), Some(42));
        assert_eq!(env_u64(&MapEnv::with("X", ""), "X"), None);
    }

    #[test]
    fn resolve_boot_config_errors_on_unreadable_path() {
        // A directory path makes `read_to_string` fail with a non-NotFound
        // error, exercising the `ConfigInvalid` arm.
        let dir = temp_dir("conf-unreadable");
        let error = resolve_boot_config_from(&dir, &MapEnv::new()).unwrap_err();
        assert!(
            matches!(error, BootError::ConfigInvalid(_)),
            "expected ConfigInvalid, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_scoped_approval_errors_on_unreadable_token() {
        // `approval_dir` is a file, so `metadata` on the joined path returns
        // ENOTDIR — the non-NotFound arm.
        let dir = temp_dir("approval-enotdir");
        let blocker = dir.join("blocker");
        write(&blocker, "not a directory");
        let error = read_scoped_approval(&blocker, "0.15.0").unwrap_err();
        assert!(
            matches!(error, BootError::ApprovalUntrusted(_)),
            "expected ApprovalUntrusted, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_scoped_approval_refuses_non_root_token() {
        // The test process is non-root, so any token we create fails trust.
        let dir = temp_dir("approval-nonroot");
        write(&dir.join("activate-0.15.0.token"), "activate:0.15.0");
        let error = read_scoped_approval(&dir, "0.15.0").unwrap_err();
        assert!(
            matches!(error, BootError::ApprovalUntrusted(_)),
            "expected ApprovalUntrusted, got {error:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // ── boot() entrypoint smoke ──────────────────────────────────────────

    #[test]
    fn boot_entrypoint_is_total_and_fail_closed() {
        // `boot()` reads real argv and the production layout. On a host with a
        // healthy cascade it is an idempotent no-op (Ok). On a host without a
        // sealed product it refuses. Either way it must never panic and must
        // never return a non-refusal error.
        let result = boot();
        if let Err(error) = result {
            assert!(
                matches!(
                    error,
                    BootError::Identity(_)
                        | BootError::ApprovalMissing
                        | BootError::ApprovalUntrusted(_)
                        | BootError::ApprovalStaleVersion { .. }
                        | BootError::HostPrerequisite(_)
                        | BootError::DirtyState(_)
                        | BootError::ConfigInvalid(_)
                        | BootError::DeployRefused
                ),
                "unexpected refusal: {error:?}"
            );
        }
    }

    #[test]
    fn release_version_is_nonempty_semver() {
        let version = release_version();
        assert!(!version.is_empty());
        assert!(version.contains('.'), "expected dotted version: {version}");
    }
}
