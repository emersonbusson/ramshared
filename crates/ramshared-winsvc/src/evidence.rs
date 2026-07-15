//! Append-only schema-1 runtime evidence (SPEC DT-10 / DT-13).
//!
//! Diagnostic only — never a recovery cursor. No pointers, payloads, or secrets.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Stable schema version for evidence rows.
pub const EVIDENCE_SCHEMA: u32 = 1;

/// I/O counters recorded in evidence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoCounters {
    pub reads: u64,
    pub writes: u64,
    pub flushes: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub errors: u64,
    pub outstanding: u64,
}

/// Nearest-rank latency summary (microseconds).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencySummary {
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub samples: u64,
}

/// One append-only evidence event (schema=1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvidence {
    pub schema: u32,
    pub run_id: String,
    pub event_id: String,
    /// UTC unix millis.
    pub ts_utc_ms: u64,
    pub mode: String,
    pub phase: String,
    pub backend: String,
    pub pid: u32,
    pub exe_sha256: String,
    pub build_id: String,
    pub os: String,
    pub driver: String,
    pub gpu: String,
    pub cuda_ordinal: u32,
    pub cuda_name: String,
    pub requested_bytes: u64,
    pub allocated_bytes: u64,
    pub free_bytes: u64,
    pub reserve_bytes: u64,
    pub lease_id: u32,
    pub lease_bytes: u64,
    pub lun_number: u32,
    pub lun_vendor: String,
    pub lun_product: String,
    pub lun_serial: String,
    pub lun_size_bytes: u64,
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    pub counters: IoCounters,
    pub latency: Option<LatencySummary>,
    pub error_class: Option<String>,
    pub error_code: Option<String>,
    pub duration_ms: u64,
}

impl RuntimeEvidence {
    /// Builder with safe defaults (backend always `cuda` for product path).
    pub fn base(run_id: impl Into<String>, phase: impl Into<String>) -> Self {
        Self {
            schema: EVIDENCE_SCHEMA,
            run_id: run_id.into(),
            event_id: format!("evt-{}", next_event_nonce()),
            ts_utc_ms: utc_ms(),
            mode: "storage-only".into(),
            phase: phase.into(),
            backend: "cuda".into(),
            pid: std::process::id(),
            exe_sha256: String::new(),
            build_id: env!("CARGO_PKG_VERSION").into(),
            os: String::new(),
            driver: String::new(),
            gpu: String::new(),
            cuda_ordinal: 0,
            cuda_name: String::new(),
            requested_bytes: 0,
            allocated_bytes: 0,
            free_bytes: 0,
            reserve_bytes: 0,
            lease_id: 0,
            lease_bytes: 0,
            lun_number: 0,
            lun_vendor: "RAMSHARE".into(),
            lun_product: "VRAMDISK".into(),
            lun_serial: String::new(),
            lun_size_bytes: 0,
            queue_depth: 0,
            max_io_bytes: 0,
            counters: IoCounters::default(),
            latency: None,
            error_class: None,
            error_code: None,
            duration_ms: 0,
        }
    }
}

/// Append-only JSONL writer.
pub struct EvidenceWriter {
    path: PathBuf,
    file: File,
}

impl EvidenceWriter {
    /// Open (create) evidence JSONL at `path` (parent must exist or be creatable by caller).
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one evidence row. Never rewrites prior rows.
    pub fn append(&mut self, row: &RuntimeEvidence) -> std::io::Result<()> {
        let line = serde_json::to_string(row)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

/// Nearest-rank percentile on a sorted sample slice (`samples` ascending).
///
/// Rank = ceil(p/100 * n) with 1-based indexing; empty → 0.
pub fn nearest_rank_percentile(sorted_asc: &[u64], percentile: f64) -> u64 {
    if sorted_asc.is_empty() {
        return 0;
    }
    if percentile <= 0.0 {
        return sorted_asc[0];
    }
    if percentile >= 100.0 {
        return sorted_asc[sorted_asc.len() - 1];
    }
    let n = sorted_asc.len() as f64;
    let rank = (percentile / 100.0 * n).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted_asc.len() - 1);
    sorted_asc[idx]
}

/// Build latency summary from microsecond samples (unsorted OK).
pub fn summarize_latencies(samples_us: &[u64]) -> LatencySummary {
    if samples_us.is_empty() {
        return LatencySummary::default();
    }
    let mut sorted = samples_us.to_vec();
    sorted.sort_unstable();
    LatencySummary {
        p50_us: nearest_rank_percentile(&sorted, 50.0),
        p95_us: nearest_rank_percentile(&sorted, 95.0),
        p99_us: nearest_rank_percentile(&sorted, 99.0),
        max_us: *sorted.last().unwrap_or(&0),
        samples: sorted.len() as u64,
    }
}

/// Redact free-form error text: drop hex addresses and truncate payload-like blobs.
pub fn redacted_error(class: &str, code: &str, detail: &str) -> (String, String, String) {
    let mut out = String::with_capacity(detail.len().min(256));
    for token in detail.split_whitespace() {
        if looks_like_pointer(token) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str("<redacted>");
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        // Cap individual tokens that look like payload dumps.
        if token.len() > 64 {
            out.push_str(&token[..16]);
            out.push('…');
        } else {
            out.push_str(token);
        }
        if out.len() >= 200 {
            out.push('…');
            break;
        }
    }
    (class.to_string(), code.to_string(), out)
}

fn looks_like_pointer(token: &str) -> bool {
    let t = token.trim_end_matches([',', ')', ']']);
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return rest.len() >= 8 && rest.chars().all(|c| c.is_ascii_hexdigit());
    }
    false
}

fn utc_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn next_event_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Read all evidence rows from a JSONL path (test helper / audit).
pub fn read_all_rows(path: &Path) -> std::io::Result<Vec<RuntimeEvidence>> {
    let f = File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: RuntimeEvidence = serde_json::from_str(&line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::fs;

    #[test]
    fn append_preserves_prior_rows() {
        let dir =
            std::env::temp_dir().join(format!("ramshared-ev-{}-{}", std::process::id(), utc_ms()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("e.jsonl");
        {
            let mut w = EvidenceWriter::open(&path).unwrap();
            let mut a = RuntimeEvidence::base("run-1", "Online");
            a.event_id = "e1".into();
            w.append(&a).unwrap();
            let mut b = RuntimeEvidence::base("run-1", "Stopping");
            b.event_id = "e2".into();
            w.append(&b).unwrap();
        }
        let rows = read_all_rows(&path).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event_id, "e1");
        assert_eq!(rows[1].event_id, "e2");
        assert_eq!(rows[0].backend, "cuda");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_has_no_pointer_or_payload_fields() {
        let row = RuntimeEvidence::base("run", "Leased");
        let v = serde_json::to_value(&row).unwrap();
        let obj = v.as_object().unwrap();
        for forbidden in [
            "pointer",
            "payload",
            "sqe",
            "cqe",
            "config_text",
            "password",
            "token",
            "va",
            "mdl",
        ] {
            assert!(
                !obj.contains_key(forbidden),
                "forbidden field present: {forbidden}"
            );
        }
        assert_eq!(obj.get("schema").and_then(|x| x.as_u64()), Some(1));
        assert_eq!(obj.get("backend").and_then(|x| x.as_str()), Some("cuda"));
    }

    #[test]
    fn nearest_rank_percentiles_are_deterministic() {
        let samples: Vec<u64> = (1..=100).collect();
        assert_eq!(nearest_rank_percentile(&samples, 50.0), 50);
        assert_eq!(nearest_rank_percentile(&samples, 95.0), 95);
        assert_eq!(nearest_rank_percentile(&samples, 99.0), 99);
        let s = summarize_latencies(&[10, 20, 30, 40, 50]);
        assert_eq!(s.p50_us, 30);
        assert_eq!(s.max_us, 50);
        assert_eq!(s.samples, 5);
        assert_eq!(nearest_rank_percentile(&[], 50.0), 0);
    }

    #[test]
    fn stable_error_redacts_payload() {
        let (c, code, detail) = redacted_error(
            "cuda",
            "CUDA_ERROR",
            "op failed at 0x7ffabcd12345 with blob ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789XX",
        );
        assert_eq!(c, "cuda");
        assert_eq!(code, "CUDA_ERROR");
        assert!(!detail.contains("0x7ffabcd12345"));
        assert!(detail.contains("<redacted>"));
        assert!(
            !detail.contains("ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789XX")
        );
    }
}
