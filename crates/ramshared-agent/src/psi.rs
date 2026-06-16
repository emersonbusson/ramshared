//! Leitura e parsing de `/proc` para o control-plane do agente:
//! - `/proc/pressure/memory` → [`PsiSample`] (sinal "quem precisa mais", RF-B2).
//! - `/proc/swaps` → `Vec<SwapEntry>` (reconciliação DT-9/DT-21, "mais ociosas" DT-19).
//! - `/proc/self/status` → euid (guarda de privilégio DT-13/DT-26).
//!
//! O parsing é separado da leitura de arquivo para ser testável com fixtures (Disciplina 13:
//! o teste exercita o parser, não o `/proc` da máquina).

use std::io::{Error, ErrorKind, Result};

use ramshared_broker::model::PsiSample;
use ramshared_broker::protocol::SwapEntry;

/// Lê e parseia `/proc/pressure/memory`.
pub fn read_psi() -> Result<PsiSample> {
    let raw = std::fs::read_to_string("/proc/pressure/memory")?;
    parse_psi(&raw).ok_or_else(|| Error::new(ErrorKind::InvalidData, "PSI ilegível"))
}

/// Parseia o conteúdo de `/proc/pressure/memory`. Usa a linha `some` (stall parcial), que é
/// o sinal de pressão relevante para swap. `None` se a linha/campos não baterem.
///
/// Formato: `some avg10=0.00 avg60=0.00 avg300=0.00 total=12345`.
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

/// Lê e parseia `/proc/swaps`.
pub fn read_swaps() -> Result<Vec<SwapEntry>> {
    Ok(parse_swaps(&std::fs::read_to_string("/proc/swaps")?))
}

/// Parseia `/proc/swaps`. A primeira linha é cabeçalho; cada linha seguinte é
/// `Filename Type Size Used Priority`. Linhas malformadas são puladas (robustez de boundary).
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

/// Parseia `memory.swap.current` (cgroup v2): inteiro em bytes. `"max"` (forma de `.max`,
/// defensivo) ou conteúdo inválido → `None`. RF-2/DT-10.
pub fn parse_memcg_swap(content: &str) -> Option<u64> {
    let t = content.trim();
    if t == "max" {
        return None;
    }
    t.parse().ok()
}

/// Lê `memory.swap.current` do cgroup v2 do processo (via `/proc/self/cgroup` → mount unificado em
/// `/sys/fs/cgroup`). `None` se não for cgroup v2 / arquivo ausente (degrade, DT-9). RF-2/DT-10.
pub fn read_memcg_swap() -> Option<u64> {
    let cg = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = cg.lines().find_map(|l| l.strip_prefix("0::"))?; // cgroup v2: linha única `0::/<path>`
    let file = format!(
        "/sys/fs/cgroup{}/memory.swap.current",
        path.trim().trim_end_matches('/')
    );
    parse_memcg_swap(&std::fs::read_to_string(file).ok()?)
}

/// Soma `sectors_read + sectors_written` (×512 = bytes) do device `dev` em `/proc/diskstats`.
/// `dev` pode ser caminho (`/dev/nbd0`) ou nome (`nbd0`). `None` se o device não aparece. RF-2/DT-11.
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

/// Lê `/proc/diskstats` e soma os sectors (×512) do device `dev`. `None` se ausente.
pub fn read_diskstats(dev: &str) -> Option<u64> {
    parse_diskstats(&std::fs::read_to_string("/proc/diskstats").ok()?, dev)
}

/// Lê o euid do processo via `/proc/self/status` (DT-26: sem libc, só `/proc`).
pub fn read_euid() -> Result<u32> {
    let raw = std::fs::read_to_string("/proc/self/status")?;
    parse_euid(&raw).ok_or_else(|| Error::new(ErrorKind::InvalidData, "campo Uid ausente"))
}

/// Parseia a linha `Uid:\t<real>\t<effective>\t<saved>\t<fs>` e devolve o euid (3º campo).
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
        // Sem total= → não dá para montar a amostra.
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
        // real=1000, effective=0 (setuid root) → guarda DT-26 deve ver o effective (0).
        assert_eq!(parse_euid("Uid:\t1000\t0\t0\t1000\n"), Some(0));
    }

    #[test]
    fn parse_euid_no_line_is_none() {
        assert!(parse_euid("Name:\tx\nGid:\t0\t0\t0\t0\n").is_none());
    }
}
