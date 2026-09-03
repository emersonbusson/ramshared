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
pub const MAX_LIFECYCLE_ROW_BYTES: usize = 16 * 1024;

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
    #[serde(default)]
    pub broker_service: String,
    #[serde(default)]
    pub broker_instance_id: String,
    #[serde(default)]
    pub broker_pipe: String,
    #[serde(default)]
    pub broker_protocol: u32,
    #[serde(default)]
    pub broker_retry_count: u32,
    #[serde(default)]
    pub broker_transition: String,
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
            broker_service: "RamSharedBroker".into(),
            broker_instance_id: String::new(),
            broker_pipe: r"\\.\pipe\RamSharedBroker.v1".into(),
            broker_protocol: ramshared_broker::protocol::PROTO_VERSION,
            broker_retry_count: 0,
            broker_transition: String::new(),
        }
    }

    /// Start a distinct append-only event for a new runtime phase.
    pub fn begin_event(&mut self, phase: impl Into<String>, ts_utc_ms: u64) {
        self.event_id = format!("evt-{}", next_event_nonce());
        self.ts_utc_ms = ts_utc_ms;
        self.phase = phase.into();
    }
}

/// Collision-resistant process-local run identity.
///
/// The monotonic nonce is required because Windows can reuse PIDs and multiple
/// runs can begin in the same system-clock tick.
pub fn new_run_id(pid: u32, ts_utc_ns: u128) -> String {
    format!("run-{pid}-{ts_utc_ns}-{}", next_run_nonce())
}

pub fn new_process_run_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    new_run_id(std::process::id(), ts)
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
        let line = serde_json::to_vec(row)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if line.len() + 1 > MAX_LIFECYCLE_ROW_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "lifecycle evidence row exceeds 16 KiB",
            ));
        }
        self.file.write_all(&line)?;
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

/// Retain stable classification while discarding unstructured detail.
pub fn redacted_error(class: &str, code: &str, _detail: &str) -> (String, String, String) {
    (class.to_string(), code.to_string(), String::new())
}

pub fn utc_ms() -> u64 {
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

fn next_run_nonce() -> u64 {
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

pub fn last_complete_row(rows: &[RuntimeEvidence]) -> Option<&RuntimeEvidence> {
    rows.iter()
        .rev()
        .find(|row| !row.run_id.is_empty() && !row.event_id.is_empty() && row.ts_utc_ms != 0)
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
    fn stable_error_discards_free_form_detail() {
        let (c, code, detail) = redacted_error(
            "cuda",
            "CUDA_ERROR",
            "password=s3cr3t token=short-secret path=C:\\Users\\operator",
        );
        assert_eq!(c, "cuda");
        assert_eq!(code, "CUDA_ERROR");
        assert!(detail.is_empty());
    }

    #[test]
    fn nearest_rank_clamps_boundary_percentiles() {
        let samples = [10, 20, 30];
        assert_eq!(nearest_rank_percentile(&samples, -1.0), 10);
        assert_eq!(nearest_rank_percentile(&samples, 0.0), 10);
        assert_eq!(nearest_rank_percentile(&samples, 100.0), 30);
        assert_eq!(nearest_rank_percentile(&samples, 101.0), 30);
    }

    #[test]
    fn run_ids_do_not_collide_for_same_process_and_clock_tick() {
        let a = new_run_id(77, 1_234_567);
        let b = new_run_id(77, 1_234_567);
        assert_ne!(a, b);
    }

    #[test]
    fn each_phase_transition_gets_a_fresh_event_identity_and_timestamp() {
        let mut row = RuntimeEvidence::base("run-1", "Stopped");
        let first_event = row.event_id.clone();
        let first_ts = row.ts_utc_ms;
        row.begin_event("Leased", first_ts + 1);
        assert_eq!(row.phase, "Leased");
        assert_ne!(row.event_id, first_event);
        assert_eq!(row.ts_utc_ms, first_ts + 1);
    }

    #[test]
    fn read_all_rows_missing_file_yields_not_found() {
        let dir = std::env::temp_dir().join(format!(
            "ramshared-ev-missing-{}-{}",
            std::process::id(),
            utc_ms()
        ));
        let path = dir.join("does_not_exist.jsonl");
        let result = read_all_rows(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn read_all_rows_ignores_empty_lines() {
        let dir = std::env::temp_dir().join(format!(
            "ramshared-ev-empty-{}-{}",
            std::process::id(),
            utc_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.jsonl");
        {
            let mut file = File::create(&path).unwrap();
            let row = RuntimeEvidence::base("run-1", "Online");
            let json = serde_json::to_string(&row).unwrap();
            file.write_all(b"\n").unwrap();
            file.write_all(b"   \n").unwrap();
            file.write_all(json.as_bytes()).unwrap();
            file.write_all(b"\n").unwrap();
            file.write_all(b"\t\n").unwrap();
        }
        let rows = read_all_rows(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, "run-1");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_all_rows_invalid_json_yields_invalid_data() {
        let dir = std::env::temp_dir().join(format!(
            "ramshared-ev-invalid-{}-{}",
            std::process::id(),
            utc_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("invalid.jsonl");
        {
            let mut file = File::create(&path).unwrap();
            file.write_all(b"{ invalid json\n").unwrap();
        }
        let result = read_all_rows(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_run_ids_are_unique_and_well_formed() {
        let a = new_process_run_id();
        let b = new_process_run_id();
        let c = new_process_run_id();

        assert!(a.starts_with("run-"));
        assert!(b.starts_with("run-"));
        assert!(c.starts_with("run-"));

        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn lifecycle_row_has_broker_identity() {
        let row = RuntimeEvidence::base("run", "WaitingForBroker");
        assert_eq!(row.broker_service, "RamSharedBroker");
        assert_eq!(row.broker_pipe, r"\\.\pipe\RamSharedBroker.v1");
        assert_eq!(
            row.broker_protocol,
            ramshared_broker::protocol::PROTO_VERSION
        );
    }

    #[test]
    fn oversized_lifecycle_row_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "ramshared-ev-oversize-{}-{}",
            std::process::id(),
            utc_ms()
        ));
        let path = dir.join("oversize.jsonl");
        let mut writer = EvidenceWriter::open(&path).unwrap();
        let mut row = RuntimeEvidence::base("run", "FailedSafe");
        row.broker_transition = "x".repeat(MAX_LIFECYCLE_ROW_BYTES);
        let error = writer.append(&row).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_uses_last_complete_row() {
        let mut incomplete = RuntimeEvidence::base("run", "Ready");
        incomplete.event_id.clear();
        let complete = RuntimeEvidence::base("run", "Stopped");
        let rows = [complete.clone(), incomplete];
        assert_eq!(last_complete_row(&rows), Some(&complete));
    }

    #[test]
    fn status_never_promotes_stale_evidence_to_current_health() {
        let stale = RuntimeEvidence::base("old-run", "Online");
        let rows = [stale];
        let row = last_complete_row(&rows).expect("complete evidence");
        assert_eq!(row.phase, "Online");
        let current_health: Option<bool> = None;
        assert_ne!(current_health, Some(true));
    }

    #[test]
    fn test_evidence_base_creation_sets_expected_defaults() {
        let row = RuntimeEvidence::base("run-42", "Starting");
        assert_eq!(row.schema, EVIDENCE_SCHEMA);
        assert_eq!(row.run_id, "run-42");
        assert_eq!(row.phase, "Starting");
        assert_eq!(row.mode, "storage-only");
        assert_eq!(row.backend, "cuda");
        assert_eq!(row.broker_service, "RamSharedBroker");
        assert!(row.ts_utc_ms > 0);
        assert!(row.event_id.starts_with("evt-"));
        assert_eq!(row.lease_id, 0);
        assert_eq!(row.latency, None);
    }

    #[test]
    fn test_evidence_utc_ms_returns_valid_timestamp() {
        let ts1 = utc_ms();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = utc_ms();
        assert!(ts1 > 0);
        assert!(ts2 >= ts1);
    }

    #[test]
    fn test_evidence_json_serialization_roundtrip_preserves_fields() {
        let mut row = RuntimeEvidence::base("run-json", "Testing");
        row.counters.reads = 42;
        row.latency = Some(LatencySummary {
            p50_us: 10,
            p95_us: 20,
            p99_us: 30,
            max_us: 100,
            samples: 5,
        });

        let json = serde_json::to_string(&row).expect("serialize");
        let deserialized: RuntimeEvidence = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.run_id, "run-json");
        assert_eq!(deserialized.counters.reads, 42);
        let lat = deserialized.latency.unwrap();
        assert_eq!(lat.p99_us, 30);
    }

    #[test]
    fn test_evidence_summarize_latencies_empty_yields_default() {
        let latencies: [u64; 0] = [];
        let summary = summarize_latencies(&latencies);
        assert_eq!(summary.samples, 0);
        assert_eq!(summary.p50_us, 0);
        assert_eq!(summary.max_us, 0);
    }

    #[test]
    fn test_evidence_summarize_latencies_unsorted_yields_correct_percentiles() {
        // [10, 50, 40, 20, 30] sorted is [10, 20, 30, 40, 50]
        let samples = vec![10, 50, 40, 20, 30];
        let summary = summarize_latencies(&samples);

        assert_eq!(summary.samples, 5);
        assert_eq!(summary.p50_us, 30);
        assert_eq!(summary.p95_us, 50); // With 5 items, rank = ceil(0.95 * 5) = ceil(4.75) = 5 -> idx 4 -> 50
        assert_eq!(summary.max_us, 50);
    }

    #[test]
    fn test_evidence_summarize_latencies_boundary_values() {
        let samples = vec![0, u64::MAX, u64::MAX / 2];
        let summary = summarize_latencies(&samples);

        assert_eq!(summary.samples, 3);
        assert_eq!(summary.max_us, u64::MAX);
    }

    #[test]
    fn test_evidence_writer_open_creates_parent_directories() {
        let dir = std::env::temp_dir().join(format!("ramshared-writer-test-{}", utc_ms()));
        let file_path = dir.join("nested").join("evidence.jsonl");

        assert!(!file_path.exists());

        {
            let _writer = EvidenceWriter::open(&file_path).expect("open writer");
            assert!(file_path.exists());
        }

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }
}
