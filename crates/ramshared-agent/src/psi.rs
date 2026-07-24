//! Reading and parsing of `/proc` for the agent's control-plane:
//! - `/proc/pressure/memory` → [`PsiSample`] (signal "who needs more", RF-B2).
//! - `/proc/swaps` → `Vec<SwapEntry>` (reconciliation DT-9/DT-21, "most idle" DT-19).
//! - `/proc/self/status` → euid (privilege guard DT-13/DT-26).
//!
//! Parsing is separated from file reading to be testable with fixtures (Discipline 13:
//! the test exercises the parser, not the machine's `/proc`).

use std::io::{Error, ErrorKind, Result};

use ramshared_broker::model::PsiSample;
use ramshared_broker::protocol::SwapEntry;

/// Core logic for `read_psi` with dependency injection for the file path.
fn read_psi_impl(path: &str) -> Result<PsiSample> {
    let raw = std::fs::read_to_string(path)?;
    parse_psi(&raw).ok_or_else(|| Error::new(ErrorKind::InvalidData, "PSI ilegível"))
}

/// Reads and parses `/proc/pressure/memory`.
pub fn read_psi() -> Result<PsiSample> {
    read_psi_impl("/proc/pressure/memory")
}

/// Parses the content of `/proc/pressure/memory`. Uses the `some` line (partial stall), which is
/// the relevant pressure signal for swap. `None` if the line/fields do not match.
///
/// Format: `some avg10=0.00 avg60=0.00 avg300=0.00 total=12345`.
pub fn parse_psi(content: &str) -> Option<PsiSample> {
    let line = content.lines().find(|l| l.starts_with("some "))?;
    let (mut avg10, mut avg60, mut total) = (None, None, None);
    for tok in line.split_whitespace() {
        if let Some(v) = tok.strip_prefix("avg10=") {
            avg10 = v.parse::<f32>().ok();
        } else if let Some(v) = tok.strip_prefix("avg60=") {
            avg60 = v.parse::<f32>().ok();
        } else if let Some(v) = tok.strip_prefix("total=") {
            total = v.parse::<u64>().ok();
        }
    }
    Some(PsiSample {
        avg10: avg10?,
        avg60: avg60?,
        stall_us: total?,
    })
}

/// Core logic for `read_swaps` with dependency injection.
fn read_swaps_impl(path: &str) -> Result<Vec<SwapEntry>> {
    Ok(parse_swaps(&std::fs::read_to_string(path)?))
}

/// Reads and parses `/proc/swaps`.
pub fn read_swaps() -> Result<Vec<SwapEntry>> {
    read_swaps_impl("/proc/swaps")
}

/// Parses `/proc/swaps`. The first line is the header; each subsequent line is
/// `Filename Type Size Used Priority`. Malformed lines are skipped (boundary robustness).
pub fn parse_swaps(content: &str) -> Vec<SwapEntry> {
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 5 {
                return None;
            }
            Some(SwapEntry {
                dev: f[0].to_string(),
                size_kb: f[2].parse().ok()?,
                used_kb: f[3].parse().ok()?,
                prio: f[4].parse().ok()?,
            })
        })
        .collect()
}

/// Parses `memory.swap.current` (cgroup v2): integer in bytes. `"max"` (defensive form of `.max`)
/// or invalid content → `None`. RF-2/DT-10.
pub fn parse_memcg_swap(content: &str) -> Option<u64> {
    let t = content.trim();
    if t == "max" {
        return None;
    }
    t.parse().ok()
}

/// Core logic for `read_memcg_swap` with dependency injection.
fn read_memcg_swap_impl(cgroup_path: &str, sysfs_base: &str) -> Option<u64> {
    let cg = std::fs::read_to_string(cgroup_path).ok()?;
    let path = cg.lines().find_map(|l| l.strip_prefix("0::"))?; // cgroup v2: single line `0::/<path>`
    let file = format!(
        "{}{}/memory.swap.current",
        sysfs_base,
        path.trim().trim_end_matches('/')
    );
    parse_memcg_swap(&std::fs::read_to_string(file).ok()?)
}

/// Reads `memory.swap.current` from the process's cgroup v2 (via `/proc/self/cgroup` → unified mount in
/// `/sys/fs/cgroup`). `None` if not cgroup v2 / missing file (degrade, DT-9). RF-2/DT-10.
pub fn read_memcg_swap() -> Option<u64> {
    read_memcg_swap_impl("/proc/self/cgroup", "/sys/fs/cgroup")
}

/// Sums `sectors_read + sectors_written` (×512 = bytes) of device `dev` in `/proc/diskstats`.
/// `dev` can be path (`/dev/nbd0`) or name (`nbd0`). `None` if device is not found. RF-2/DT-11.
pub fn parse_diskstats(content: &str, dev: &str) -> Option<u64> {
    let name = dev.rsplit('/').next().unwrap_or(dev);
    content.lines().find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 || f[2] != name {
            return None;
        }
        let rd: u64 = f[5].parse().ok()?;
        let wr: u64 = f[9].parse().ok()?;
        Some(rd.saturating_add(wr).saturating_mul(512))
    })
}

/// Core logic for `read_diskstats` with dependency injection.
fn read_diskstats_impl(path: &str, dev: &str) -> Option<u64> {
    parse_diskstats(&std::fs::read_to_string(path).ok()?, dev)
}

/// Reads `/proc/diskstats` and sums sectors (×512) of device `dev`. `None` if missing.
pub fn read_diskstats(dev: &str) -> Option<u64> {
    read_diskstats_impl("/proc/diskstats", dev)
}

/// Core logic for `read_euid` with dependency injection.
fn read_euid_impl(path: &str) -> Result<u32> {
    let raw = std::fs::read_to_string(path)?;
    parse_euid(&raw).ok_or_else(|| Error::new(ErrorKind::InvalidData, "campo Uid ausente"))
}

/// Reads the euid of the process via `/proc/self/status` (DT-26: no libc, only `/proc`).
pub fn read_euid() -> Result<u32> {
    read_euid_impl("/proc/self/status")
}

/// Parses the line `Uid:\t<real>\t<effective>\t<saved>\t<fs>` and returns the euid (3rd field).
pub fn parse_euid(status: &str) -> Option<u32> {
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn parse_psi_some_line() {
        let s = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999\n\
                 full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        let p = parse_psi(s).unwrap();
        assert_eq!(p.avg10, 1.23);
        assert_eq!(p.avg60, 4.56);
        assert_eq!(p.stall_us, 999);
    }

    #[test]
    fn parse_psi_idle_zero() {
        let p = parse_psi("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
        assert_eq!(p.avg10, 0.0);
        assert_eq!(p.stall_us, 0);
    }

    #[test]
    fn parse_psi_missing_field_is_none() {
        // Without total= → cannot assemble the sample.
        assert!(parse_psi("some avg10=1.0 avg60=2.0 avg300=3.0\n").is_none());
    }

    #[test]
    fn parse_psi_no_some_line_is_none() {
        assert!(parse_psi("full avg10=1.0 avg60=2.0 avg300=3.0 total=5\n").is_none());
    }

    #[test]
    fn parse_swaps_partition_and_file() {
        let s = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
                 /dev/nbd0                               partition\t1048576\t\t2048\t\t-2\n\
                 /swapfile                               file\t\t524288\t\t0\t\t-3\n";
        let v = parse_swaps(s);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].dev, "/dev/nbd0");
        assert_eq!(v[0].size_kb, 1048576);
        assert_eq!(v[0].used_kb, 2048);
        assert_eq!(v[0].prio, -2);
        assert_eq!(v[1].dev, "/swapfile");
        assert_eq!(v[1].prio, -3);
    }

    #[test]
    fn parse_swaps_skips_header_only() {
        assert!(parse_swaps("Filename\tType\tSize\tUsed\tPriority\n").is_empty());
        assert!(parse_swaps("").is_empty());
    }

    #[test]
    fn parse_swaps_skips_malformed_line() {
        let s = "Filename\tType\tSize\tUsed\tPriority\n\
                 /dev/nbd0 partition 100\n\
                 /dev/nbd1 partition 200 10 -2\n";
        let v = parse_swaps(s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].dev, "/dev/nbd1");
    }

    #[test]
    fn parse_memcg_swap_reads_integer() {
        assert_eq!(parse_memcg_swap("4194304\n"), Some(4194304));
    }

    #[test]
    fn parse_memcg_swap_max_or_garbage_is_none() {
        assert_eq!(parse_memcg_swap("max\n"), None);
        assert_eq!(parse_memcg_swap("lixo"), None);
    }

    #[test]
    fn parse_diskstats_sums_rw_times_512() {
        // major minor name reads rd_merged sectors_read ms_rd writes wr_merged sectors_written ...
        let s = "  43       0 nbd0 100 0 200 5 50 0 80 3 0 0\n";
        assert_eq!(parse_diskstats(s, "/dev/nbd0"), Some((200 + 80) * 512));
        assert_eq!(parse_diskstats(s, "nbd0"), Some((200 + 80) * 512));
    }

    #[test]
    fn parse_diskstats_unknown_dev_is_none() {
        let s = "  43 0 nbd0 1 2 3 4 5 6 7 8 9 10\n";
        assert!(parse_diskstats(s, "nbd9").is_none());
    }

    #[test]
    fn parse_euid_effective_is_third_field() {
        let status =
            "Name:\tramshared-agent\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_euid(status), Some(1000));
    }

    #[test]
    fn parse_euid_root() {
        assert_eq!(parse_euid("Uid:\t0\t0\t0\t0\n"), Some(0));
    }

    #[test]
    fn parse_euid_setuid_differs_from_real() {
        // real=1000, effective=0 (setuid root) → guard DT-26 must see the effective (0).
        assert_eq!(parse_euid("Uid:\t1000\t0\t0\t1000\n"), Some(0));
    }

    #[test]
    fn parse_euid_no_line_is_none() {
        assert!(parse_euid("Name:\tx\nGid:\t0\t0\t0\t0\n").is_none());
    }

    fn write_temp_file(content: &str) -> String {
        use std::env;
        use std::fs;
        use std::io::Write;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!("ramshared_test_{}_{}", std::process::id(), id));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn read_psi_impl_success() {
        let content = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999\n";
        let path = write_temp_file(content);
        let psi = read_psi_impl(&path).unwrap();
        assert_eq!(psi.avg10, 1.23);
        assert_eq!(psi.avg60, 4.56);
        assert_eq!(psi.stall_us, 999);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_psi_impl_not_found() {
        assert!(read_psi_impl("/proc/nonexistent_psi_file_12345").is_err());
    }

    #[test]
    fn read_psi_impl_invalid_data() {
        let path = write_temp_file("invalid content\n");
        let err = read_psi_impl(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_swaps_impl_success() {
        let content = "Filename\tType\tSize\tUsed\tPriority\n\
                       /dev/nbd0\tpartition\t1048576\t2048\t-2\n";
        let path = write_temp_file(content);
        let swaps = read_swaps_impl(&path).unwrap();
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].dev, "/dev/nbd0");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_swaps_impl_not_found() {
        assert!(read_swaps_impl("/proc/nonexistent_swaps_file_12345").is_err());
    }

    #[test]
    fn read_memcg_swap_impl_success() {
        let cgroup_content = "0::/my_cgroup\n";
        let cgroup_path = write_temp_file(cgroup_content);

        let sysfs_base = std::env::temp_dir().join(format!(
            "ramshared_sysfs_{}_{}",
            std::process::id(),
            std::sync::atomic::AtomicUsize::new(0)
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(sysfs_base.join("my_cgroup")).unwrap();
        let swap_current_path = sysfs_base.join("my_cgroup/memory.swap.current");
        std::fs::write(&swap_current_path, "4194304\n").unwrap();

        let val = read_memcg_swap_impl(&cgroup_path, &sysfs_base.to_string_lossy()).unwrap();
        assert_eq!(val, 4194304);

        std::fs::remove_file(cgroup_path).unwrap();
        std::fs::remove_dir_all(sysfs_base).unwrap();
    }

    #[test]
    fn read_memcg_swap_impl_missing_cgroup_file() {
        assert!(read_memcg_swap_impl("/proc/nonexistent_cgroup_file", "/sys/fs/cgroup").is_none());
    }

    #[test]
    fn read_memcg_swap_impl_missing_0_line() {
        let cgroup_content = "1:name=systemd:/\n";
        let cgroup_path = write_temp_file(cgroup_content);

        assert!(read_memcg_swap_impl(&cgroup_path, "/sys/fs/cgroup").is_none());

        std::fs::remove_file(cgroup_path).unwrap();
    }

    #[test]
    fn read_diskstats_impl_success() {
        let content = "  43       0 nbd0 100 0 200 5 50 0 80 3 0 0\n";
        let path = write_temp_file(content);

        assert_eq!(read_diskstats_impl(&path, "nbd0"), Some((200 + 80) * 512));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_diskstats_impl_missing_file() {
        assert!(read_diskstats_impl("/proc/nonexistent_diskstats", "nbd0").is_none());
    }

    #[test]
    fn read_euid_impl_success() {
        let content = "Name:\tramshared-agent\nUid:\t1000\t1001\t1000\t1000\n";
        let path = write_temp_file(content);

        assert_eq!(read_euid_impl(&path).unwrap(), 1001);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_euid_impl_missing_file() {
        assert!(read_euid_impl("/proc/nonexistent_status").is_err());
    }

    #[test]
    fn read_euid_impl_missing_uid() {
        let content = "Name:\tramshared-agent\nGid:\t1000\t1001\t1000\t1000\n";
        let path = write_temp_file(content);

        let err = read_euid_impl(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);

        std::fs::remove_file(path).unwrap();
    }
}
