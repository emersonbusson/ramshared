use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct IntegrityTelemetry {
    pub corrupted_blocks: AtomicU64,
    pub repaired_pages: AtomicU64,
    pub quarantine_size_bytes: AtomicU64,
}

impl IntegrityTelemetry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_corruption(&self) {
        self.corrupted_blocks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_repair(&self) {
        self.repaired_pages.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_quarantine_bytes(&self, bytes: u64) {
        self.quarantine_size_bytes.fetch_add(bytes, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_telemetry_counters() {
        let telemetry = IntegrityTelemetry::new();
        telemetry.record_corruption();
        telemetry.record_repair();
        telemetry.add_quarantine_bytes(4096);

        assert_eq!(telemetry.corrupted_blocks.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.repaired_pages.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.quarantine_size_bytes.load(Ordering::Relaxed), 4096);
    }

    #[test]
    fn test_telemetry_chaos_concurrent_updates() {
        let telemetry = Arc::new(IntegrityTelemetry::new());
        let mut handles = vec![];

        for _ in 0..100 {
            let t = Arc::clone(&telemetry);
            handles.push(thread::spawn(move || {
                t.record_corruption();
                t.record_repair();
                t.add_quarantine_bytes(4096);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(telemetry.corrupted_blocks.load(Ordering::Relaxed), 100);
        assert_eq!(telemetry.repaired_pages.load(Ordering::Relaxed), 100);
        assert_eq!(telemetry.quarantine_size_bytes.load(Ordering::Relaxed), 409600);
    }
}
