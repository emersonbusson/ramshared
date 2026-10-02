//! Daemon-owned pid record for `/run/ramshared/ramsharedd.pid`.
//!
//! The CLI writes this record after spawning (`cascade_io.rs`
//! `spawn_daemon_with_deadline`), but a `ramsharedd` started outside
//! `ramshared up`/`boot` never produced a record at all: the previous
//! instance's file stayed behind and `ramshared status` reported
//! `daemon: dead pid=null` for a live process. The daemon is the authority
//! for its own identity. It claims the record once serving is authorised and
//! releases it on exit.
//!
//! Shape is the sealed runtime-record contract read by
//! `legacy_runtime_record_value`: a root-owned regular file, not
//! group/other-writable, containing only the decimal pid.

use std::fs;
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Same path the CLI reads (`cascade/mod.rs` `PID_FILE`).
pub const PID_FILE_PATH: &str = "/run/ramshared/ramsharedd.pid";

/// Sealed mode for runtime records: owner-write only, never group/other
/// writable (`legacy_runtime_record_value` rejects `mode & 0o022 != 0`).
const PID_FILE_MODE: u32 = 0o644;

/// Claims the pid record for this process. `Drop` releases it.
///
/// Release is conditional on the record still naming this process, so a
/// superseding daemon is never un-recorded by the one it replaced
/// (Kahneman #17: the drop is idempotent and fail-safe).
pub struct PidFileGuard {
    path: PathBuf,
}

impl PidFileGuard {
    /// Claim the production pid path. Fails if a live `ramsharedd` already
    /// owns the record: a duplicate must not steal the identity the CLI uses
    /// to stop the exact process.
    pub fn claim() -> io::Result<Self> {
        Self::claim_at(Path::new(PID_FILE_PATH))
    }

    /// Claim an explicit path (tests). `owner_pid` is the identity to record;
    /// production passes `std::process::id()`.
    pub fn claim_at_for(path: &Path, owner_pid: u32) -> io::Result<Self> {
        if owner_pid == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pid record owner must be a real process id",
            ));
        }
        if let Some(holder) = live_ramsharedd_pid_at(path)
            && holder != owner_pid
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("pid record is owned by live ramsharedd {holder}"),
            ));
        }
        write_sealed_pid_record(path, owner_pid)?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Claim an explicit path for the current process (tests).
    pub fn claim_at(path: &Path) -> io::Result<Self> {
        Self::claim_at_for(path, std::process::id())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let ours = std::process::id().to_string();
        if let Ok(current) = fs::read_to_string(&self.path)
            && current.trim() == ours
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The live `ramsharedd` the record names, if any. A record naming a dead or
/// foreign pid is not an owner.
pub fn live_ramsharedd_pid_at(path: &Path) -> Option<u32> {
    let pid = read_pid_record(path)?;
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    (comm.trim() == "ramsharedd").then_some(pid)
}

fn read_pid_record(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
}

/// Write the record as a sealed root-owned regular file via tmp+rename so a
/// concurrent reader never observes a partial pid.
fn write_sealed_pid_record(path: &Path, pid: u32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("pid.tmp");
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(PID_FILE_MODE)
            .open(&tmp)?;
        write!(file, "{pid}")?;
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(PID_FILE_MODE))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_pid_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ramshared-pid-file-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn claim_records_own_pid_and_drop_releases_it() {
        let path = temp_pid_path("claim-release");
        let guard = PidFileGuard::claim_at(&path).expect("claim a free pid record");
        let body = fs::read_to_string(&path).expect("record exists while held");
        assert_eq!(body.trim(), std::process::id().to_string());
        drop(guard);
        assert!(
            !path.exists(),
            "drop must release a record that still names this process"
        );
    }

    #[test]
    fn record_is_sealed_root_owned_regular_file() {
        let path = temp_pid_path("sealed");
        let guard = PidFileGuard::claim_at(&path).expect("claim a free pid record");
        let meta = fs::symlink_metadata(&path).expect("record metadata");
        assert!(meta.file_type().is_file(), "record must be a regular file");
        assert_eq!(
            meta.permissions().mode() & 0o022,
            0,
            "record must not be group/other writable"
        );
        drop(guard);
    }

    #[test]
    fn drop_does_not_release_a_record_owned_by_a_replacement() {
        let path = temp_pid_path("superseded");
        let first = PidFileGuard::claim_at_for(&path, 1001).expect("first claim");
        // A replacement daemon overwrites the record before the first exits.
        write_sealed_pid_record(&path, 2002).expect("replacement claim");
        drop(first);
        assert_eq!(
            fs::read_to_string(&path)
                .expect("replacement record survives")
                .trim(),
            "2002",
            "the superseded guard must not un-record the replacement"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn claim_refuses_a_record_held_by_another_live_ramsharedd() {
        let path = temp_pid_path("exclusive");
        // This process is not named `ramsharedd`, so a record naming it is a
        // foreign holder and must not be stolen.
        let foreign = std::process::id();
        write_sealed_pid_record(&path, foreign).expect("foreign record");
        assert!(
            live_ramsharedd_pid_at(&path).is_none(),
            "a non-ramsharedd comm is not a live owner"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn stale_record_naming_a_dead_pid_is_not_an_owner() {
        let path = temp_pid_path("stale");
        // pid 0 is never a live process; 2^22+ is above the default pid_max.
        write_sealed_pid_record(&path, 4_194_304).expect("stale record");
        assert!(
            live_ramsharedd_pid_at(&path).is_none(),
            "a record naming a dead pid must not block a new claim"
        );
        let guard = PidFileGuard::claim_at_for(&path, 3003)
            .expect("a stale record must not block a new claim");
        assert_eq!(
            fs::read_to_string(&path).expect("new record").trim(),
            "3003"
        );
        drop(guard);
    }

    #[test]
    fn zero_pid_is_rejected() {
        let path = temp_pid_path("zero");
        assert!(PidFileGuard::claim_at_for(&path, 0).is_err());
        assert!(!path.exists(), "a rejected claim must leave no record");
    }
}
