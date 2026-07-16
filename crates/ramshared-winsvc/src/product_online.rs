//! Windows product Online composition (SPEC DT-3/DT-4/DT-7):
//! broker lease → CUDA DeviceMem → CREATE/REGISTER → I/O loop on one thread.
//!
//! Cover: N/A for unsafe Windows bits; pure sequencing tested via RuntimeOps.

#![cfg(windows)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use ramshared_block::VramBackend;
use ramshared_cuda::Cuda;

use crate::broker_tenant::BrokerTenant;
use crate::config::WinDriveConfig;
use crate::driver_link::{DriverLink, QueueAccess};
use crate::evidence::{
    EvidenceWriter, IoCounters, RuntimeEvidence, new_process_run_id, summarize_latencies, utc_ms,
};
use crate::proto::DiskParams;
use crate::runtime::{
    RunMode, RuntimeError, RuntimeErrorClass, RuntimePhase, RuntimeState, RuntimeSummary,
};
use crate::service::{
    DiskControl, PagefileGates, ServiceState, TeardownTarget, WipeVram,
    pagefile_refusal_to_runtime, teardown_storage_only,
};
use crate::windows_driver::{WindowsDriverLink, WindowsMappedQueue};
use crate::windows_host::WindowsHostState;

/// Serial: 16 uppercase hex digits derived from run id hash (DT-11).
pub fn serial_from_run_id(run_id: &str) -> [u8; 16] {
    let mut out = [b'0'; 16];
    let h = simple_hash(run_id.as_bytes());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, digit) in out.iter_mut().enumerate() {
        let nibble = ((h >> (i * 4)) & 0xF) as usize;
        *digit = HEX[nibble];
    }
    out
}

fn simple_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Run product Online until `stop` is set or fatal error.
pub fn run_product_online(
    cfg: &WinDriveConfig,
    mode: RunMode,
    stop: Arc<AtomicBool>,
) -> Result<RuntimeSummary, RuntimeError> {
    let run_id = new_process_run_id();
    let mut state = RuntimeState::new(mode);
    let serial = serial_from_run_id(&run_id);
    let serial_str = String::from_utf8_lossy(&serial).into_owned();

    // Evidence (diagnostic only).
    let mut evidence = EvidenceWriter::open(cfg.evidence_path().join(format!("{run_id}.jsonl")))
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Internal, 1, e.to_string()))?;
    let mut row = RuntimeEvidence::base(&run_id, "Stopped");
    row.lun_serial = serial_str.clone();
    row.lun_size_bytes = cfg.size_bytes;
    row.requested_bytes = cfg.size_bytes;
    row.queue_depth = cfg.queue_depth;
    row.max_io_bytes = cfg.max_io_bytes;
    row.cuda_ordinal = cfg.cuda_device;
    sync_runtime_evidence(&mut row, &state);
    let _ = evidence.append(&row);

    // --- Broker lease ---
    let addr = cfg
        .broker_addr()
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Config, 2, e.to_string()))?;
    let raw = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| {
        RuntimeError::new(RuntimeErrorClass::Broker, 3, format!("connect {addr}: {e}"))
    })?;
    raw.set_read_timeout(Some(Duration::from_secs(10))).ok();
    raw.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let mut stream = BrokStream::new(raw)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Broker, 3, format!("stream: {e}")))?;
    let mut tenant = BrokerTenant::new(cfg.tenant.clone(), Duration::from_secs(cfg.heartbeat_secs));
    tenant
        .register(&mut stream)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Broker, 3, e.to_string()))?;
    let lease = tenant
        .acquire(&mut stream, cfg.size_bytes)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Broker, 3, e.to_string()))?;
    macro_rules! lease_try {
        ($expr:expr, $class:expr, $code:expr) => {
            match $expr {
                Ok(value) => value,
                Err(error) => {
                    return Err(error_after_release(
                        &mut tenant,
                        &mut stream,
                        $class,
                        $code,
                        error.to_string(),
                    ));
                }
            }
        };
    }
    state.lease_id = Some(lease.lease);
    state.lease_bytes = lease.bytes;
    state.phase = RuntimePhase::Leased;
    row.begin_event("Leased", utc_ms());
    sync_runtime_evidence(&mut row, &state);
    let _ = evidence.append(&row);

    // --- CUDA on this thread (affinity) ---
    let cuda = lease_try!(Cuda::load(), RuntimeErrorClass::Cuda, 4);
    let count = lease_try!(cuda.device_count(), RuntimeErrorClass::Cuda, 4);
    if cfg.cuda_device as i32 >= count {
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Cuda,
            4,
            format!("cuda_device {} >= {count}", cfg.cuda_device),
        ));
    }
    let dev = lease_try!(
        cuda.device(cfg.cuda_device as i32),
        RuntimeErrorClass::Cuda,
        4
    );
    let ctx = lease_try!(cuda.create_context(&dev), RuntimeErrorClass::Cuda, 4);
    let (free, total) = lease_try!(ctx.mem_info(), RuntimeErrorClass::Cuda, 4);
    let reserve = cfg.effective_reserve_bytes(total as u64) as usize;
    let Some(need) = (cfg.size_bytes as usize).checked_add(reserve) else {
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Config,
            2,
            "size+reserve overflow".into(),
        ));
    };
    if free < need {
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Cuda,
            4,
            format!("free {free} < need {need}"),
        ));
    }
    if let Err(e) = tenant.coresidence_gate(free as u64, cfg.size_bytes) {
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Cuda,
            4,
            e.to_string(),
        ));
    }
    let size = cfg.size_bytes as usize;
    let mut mem = lease_try!(ctx.alloc(size), RuntimeErrorClass::Cuda, 4);
    if let Err(error) = mem.zero() {
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Cuda,
            4,
            error.to_string(),
        ));
    }
    state.allocated_bytes = cfg.size_bytes;
    state.phase = RuntimePhase::CudaReady;
    row.begin_event("CudaReady", utc_ms());
    sync_runtime_evidence(&mut row, &state);
    row.free_bytes = free as u64;
    row.reserve_bytes = reserve as u64;
    row.cuda_name = dev.name().to_string();
    let _ = evidence.append(&row);

    let mut backend = VramBackend::new(mem, cfg.block_size);

    // --- Driver CREATE + REGISTER ---
    let mut link = match WindowsDriverLink::open() {
        Ok(link) => link,
        Err(error) => {
            let mut message = error.to_string();
            if let Err(wipe_error) = backend.zero() {
                message.push_str(&format!("; VRAM wipe failed: {wipe_error}"));
            }
            return Err(error_after_release(
                &mut tenant,
                &mut stream,
                RuntimeErrorClass::Abi,
                5,
                message,
            ));
        }
    };
    let params = DiskParams {
        size_bytes: cfg.size_bytes,
        block_size: cfg.block_size,
        reserved: 0,
        serial,
    };
    if let Err(e) = link.create_disk(&params) {
        let mut message = e.to_string();
        if let Err(wipe_error) = backend.zero() {
            message.push_str(&format!("; VRAM wipe failed: {wipe_error}"));
        }
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Abi,
            5,
            message,
        ));
    }
    state.phase = RuntimePhase::DiskCreated;
    row.begin_event("DiskCreated", utc_ms());
    sync_runtime_evidence(&mut row, &state);
    let _ = evidence.append(&row);

    let q = match WindowsMappedQueue::try_new(cfg.queue_depth, cfg.max_io_bytes, cfg.block_size) {
        Ok(queue) => queue,
        Err(error) => {
            if let Err(destroy_error) = link.destroy_disk() {
                eprintln!("queue allocation failed and DESTROY failed: {error}; {destroy_error}");
                preserve_failed_safe("startup unwind could not destroy disk");
            }
            let mut message = error.to_string();
            if let Err(wipe_error) = backend.zero() {
                message.push_str(&format!("; VRAM wipe failed: {wipe_error}"));
            }
            return Err(error_after_release(
                &mut tenant,
                &mut stream,
                RuntimeErrorClass::Abi,
                5,
                message,
            ));
        }
    };
    let reg = q.registration(0);
    if let Err(e) = link.register_queue(&reg) {
        if let Err(destroy_error) = link.destroy_disk() {
            eprintln!("REGISTER failed and DESTROY failed: {e}; {destroy_error}");
            preserve_failed_safe("startup unwind could not destroy disk");
        }
        let mut message = e.to_string();
        if let Err(wipe_error) = backend.zero() {
            message.push_str(&format!("; VRAM wipe failed: {wipe_error}"));
        }
        return Err(error_after_release(
            &mut tenant,
            &mut stream,
            RuntimeErrorClass::Abi,
            5,
            message,
        ));
    }
    state.phase = RuntimePhase::QueueRegistered;
    state.phase = RuntimePhase::Online;
    row.begin_event("Online", utc_ms());
    sync_runtime_evidence(&mut row, &state);
    row.backend = "cuda".into();
    let _ = evidence.append(&row);

    eprintln!(
        "product Online: run_id={run_id} lease={} size={} serial={} cuda={}",
        lease.lease,
        cfg.size_bytes,
        serial_str,
        dev.name()
    );

    // --- I/O loop: one pending COMMIT at a time ---
    let mut dlink = DriverLink::from_queue(q);
    let mut last_progress = Instant::now();
    let cuda_watchdog = CudaWatchdog::start(Duration::from_millis(5_000));
    let commit_watchdog = Duration::from_millis(5_000);

    loop {
        while !stop.load(Ordering::Acquire) {
            match link.commit_and_fetch(Duration::from_millis(500)) {
                Ok(()) => {
                    cuda_watchdog.begin();
                    let served = dlink.commit_and_fetch(&mut backend);
                    cuda_watchdog.end();
                    sync_io_evidence(&mut row, &dlink);
                    if cuda_watchdog.fired() {
                        state.healthy = false;
                        state.phase = RuntimePhase::FailedSafe;
                        sync_runtime_evidence(&mut row, &state);
                        let _ = evidence.append(&row);
                        preserve_failed_safe("CUDA operation exceeded 5,000 ms");
                    }
                    match served {
                        Ok(n) if n > 0 => last_progress = Instant::now(),
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("serve error: {e}");
                            state.healthy = false;
                            state.phase = RuntimePhase::FailedSafe;
                            sync_runtime_evidence(&mut row, &state);
                            let _ = evidence.append(&row);
                            preserve_failed_safe("I/O service failure");
                        }
                    }
                }
                Err(crate::windows_driver::IoctlError::Timeout) => {}
                Err(e) => {
                    eprintln!("commit_and_fetch: {e}");
                    if last_progress.elapsed() > commit_watchdog {
                        state.healthy = false;
                        state.phase = RuntimePhase::FailedSafe;
                        sync_runtime_evidence(&mut row, &state);
                        let _ = evidence.append(&row);
                        preserve_failed_safe("COMMIT watchdog expired");
                    }
                }
            }
            if let Err(e) = tenant.heartbeat_psi(&mut stream) {
                eprintln!("broker heartbeat failed: {e}");
            } else if let Err(e) = stream.flush() {
                eprintln!("broker heartbeat flush failed: {e}");
            }
            thread::sleep(Duration::from_millis(10));
        }

        state.phase = RuntimePhase::Stopping;
        row.begin_event("Stopping", utc_ms());
        sync_runtime_evidence(&mut row, &state);
        let _ = evidence.append(&row);
        let _ = link.cancel_fetch();

        let teardown_letter = cfg.volume_letter.to_ascii_uppercase();
        let mut svc = ServiceState {
            lease: Some(crate::broker_tenant::LeaseState {
                lease: lease.lease,
                bytes: lease.bytes,
            }),
            disk_created: true,
            registered_queue: true,
            online: true,
        };
        let mut disk_ctl = LinkDisk {
            link: &mut link,
            unregistered: false,
            destroyed: false,
        };
        let mut wipe = BackendWipe {
            backend: &mut backend,
        };
        let mut gates = HostGates::new(teardown_letter, &serial_str, cfg.size_bytes)
            .map_err(|e| RuntimeError::new(RuntimeErrorClass::Identity, 6, e))?;
        let mut phases = Vec::new();
        let mut cfg_teardown = cfg.clone();
        cfg_teardown.volume_letter = teardown_letter;

        match teardown_storage_only(
            &cfg_teardown,
            &mut svc,
            &mut disk_ctl,
            &mut wipe,
            &mut gates,
            &mut phases,
        ) {
            Ok(()) => {
                tenant.release(&mut stream).map_err(|e| {
                    RuntimeError::new(
                        RuntimeErrorClass::Broker,
                        3,
                        format!("destructive teardown completed but lease release failed: {e}"),
                    )
                })?;
                state.phase = RuntimePhase::Stopped;
                state.lease_id = None;
                state.allocated_bytes = 0;
                row.begin_event("Stopped", utc_ms());
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                return Ok(RuntimeSummary {
                    phase: state.phase,
                    lease_id: state.lease_id,
                    allocated_bytes: state.allocated_bytes,
                    idempotent_stop: false,
                    exit_code: 0,
                });
            }
            Err(e) => {
                if pagefile_refusal_to_runtime(&e).is_some() {
                    // Retain every live owner and resume service. A later stop
                    // performs a fresh identity/Gate-A/lock/Gate-B attempt.
                    state.phase = RuntimePhase::Online;
                    row.begin_event("Online", utc_ms());
                    sync_runtime_evidence(&mut row, &state);
                    row.error_class = Some("pagefile_safety".into());
                    row.error_code = Some("7".into());
                    let _ = evidence.append(&row);
                    stop.store(false, Ordering::Release);
                    continue;
                }
                eprintln!("teardown entered failed-safe preservation: {e}");
                state.healthy = false;
                state.phase = RuntimePhase::FailedSafe;
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                preserve_failed_safe("partial teardown failure");
            }
        }
    }
}

fn preserve_failed_safe(reason: &str) -> ! {
    eprintln!(
        "FailedSafe: {reason}; preserving disk, allocation, and lease until supervised reboot"
    );
    loop {
        thread::park_timeout(Duration::from_secs(1));
    }
}

fn sync_runtime_evidence(row: &mut RuntimeEvidence, state: &RuntimeState) {
    row.mode = match state.mode {
        RunMode::Console => "console",
        RunMode::Scm => "scm",
        RunMode::ProbeCuda => "probe-cuda",
    }
    .into();
    row.phase = match state.phase {
        RuntimePhase::Stopped => "Stopped",
        RuntimePhase::Leased => "Leased",
        RuntimePhase::CudaReady => "CudaReady",
        RuntimePhase::DiskCreated => "DiskCreated",
        RuntimePhase::QueueRegistered => "QueueRegistered",
        RuntimePhase::Online => "Online",
        RuntimePhase::Stopping => "Stopping",
        RuntimePhase::FailedSafe => "FailedSafe",
    }
    .into();
    row.lease_id = state.lease_id.unwrap_or(0);
    row.lease_bytes = state.lease_bytes;
    row.allocated_bytes = state.allocated_bytes;
    if !state.healthy {
        row.error_class = Some("failed_safe".into());
    } else {
        row.error_class = None;
        row.error_code = None;
    }
}

fn sync_io_evidence(row: &mut RuntimeEvidence, link: &DriverLink<WindowsMappedQueue>) {
    let stats = link.stats();
    row.counters = IoCounters {
        reads: stats.reads,
        writes: stats.writes,
        flushes: stats.flushes,
        bytes_read: stats.bytes_read,
        bytes_written: stats.bytes_written,
        errors: stats.errors,
        outstanding: u64::from(link.q.sq_pending()),
    };
    row.latency = Some(summarize_latencies(&stats.latencies_us));
}

struct CudaWatchdog {
    active_since: Arc<Mutex<Option<Instant>>>,
    fired: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl CudaWatchdog {
    fn start(limit: Duration) -> Self {
        let active_since = Arc::new(Mutex::new(None::<Instant>));
        let fired = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let active_for_thread = Arc::clone(&active_since);
        let fired_for_thread = Arc::clone(&fired);
        let shutdown_for_thread = Arc::clone(&shutdown);
        thread::spawn(move || {
            while !shutdown_for_thread.load(Ordering::Acquire) {
                let expired = active_for_thread
                    .lock()
                    .ok()
                    .and_then(|active| *active)
                    .is_some_and(|started| started.elapsed() > limit);
                if expired && !fired_for_thread.swap(true, Ordering::AcqRel) {
                    eprintln!(
                        "CUDA watchdog: synchronous operation exceeded {} ms; preserving owners",
                        limit.as_millis()
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
        Self {
            active_since,
            fired,
            shutdown,
        }
    }

    fn begin(&self) {
        if let Ok(mut active) = self.active_since.lock() {
            *active = Some(Instant::now());
        } else {
            self.fired.store(true, Ordering::Release);
        }
    }

    fn end(&self) {
        if let Ok(mut active) = self.active_since.lock() {
            *active = None;
        } else {
            self.fired.store(true, Ordering::Release);
        }
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::Acquire)
    }
}

impl Drop for CudaWatchdog {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

/// Split read/write over cloned TCP so `BufRead + Write` is available.
struct BrokStream {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl BrokStream {
    fn new(stream: TcpStream) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }
}

impl Read for BrokStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

impl BufRead for BrokStream {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.reader.fill_buf()
    }
    fn consume(&mut self, amt: usize) {
        self.reader.consume(amt);
    }
}

impl Write for BrokStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

fn error_after_release(
    tenant: &mut BrokerTenant,
    stream: &mut BrokStream,
    class: RuntimeErrorClass,
    code: i32,
    mut message: String,
) -> RuntimeError {
    if let Err(release_error) = tenant.release(stream) {
        message.push_str(&format!("; lease release failed: {release_error}"));
    }
    RuntimeError::new(class, code, message)
}

struct LinkDisk<'a> {
    link: &'a mut WindowsDriverLink,
    unregistered: bool,
    destroyed: bool,
}

impl DiskControl for LinkDisk<'_> {
    fn create_disk(&mut self, _: u64, _: u32) -> Result<(), String> {
        Ok(())
    }
    fn destroy_disk(&mut self) -> Result<(), String> {
        if self.destroyed {
            return Ok(());
        }
        self.link.destroy_disk().map_err(|e| e.to_string())?;
        self.destroyed = true;
        Ok(())
    }
    fn register_queue(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn unregister_queue(&mut self) -> Result<(), String> {
        if self.unregistered {
            return Ok(());
        }
        self.link.unregister_queue().map_err(|e| e.to_string())?;
        self.unregistered = true;
        Ok(())
    }
}

struct BackendWipe<'a, M> {
    backend: &'a mut VramBackend<M>,
}

impl<M: ramshared_vram::VramMemory> WipeVram for BackendWipe<'_, M> {
    fn zero(&mut self) -> Result<(), String> {
        self.backend.zero().map_err(|e| e.to_string())
    }
}

struct HostGates {
    locked: Option<crate::windows_host::LockedVolume>,
    volume_letter: char,
    target: TeardownTarget,
}

impl HostGates {
    fn new(volume_letter: char, serial: &str, size_bytes: u64) -> Result<Self, String> {
        Ok(Self {
            locked: None,
            volume_letter,
            target: TeardownTarget::new(volume_letter, serial, size_bytes)?,
        })
    }
}

impl PagefileGates for HostGates {
    fn verify_volume_identity(&self, letter: char) -> Result<(), String> {
        let observed =
            WindowsHostState::observe_volume_identity(letter).map_err(|e| e.to_string())?;
        self.target.verify_unique(&[observed]).map(|_| ())
    }

    fn active_pagefiles(&self) -> Result<Vec<String>, String> {
        // Storage-only DT-8: refuse only pagefiles on the product volume letter,
        // not the system C: pagefile (which is expected on a normal host).
        let letter = self
            .locked
            .as_ref()
            .map(|v| v.letter)
            .unwrap_or(self.volume_letter);
        let prefix = format!("{}:", letter.to_ascii_uppercase());
        WindowsHostState::active_pagefiles()
            .map(|v| {
                v.into_iter()
                    .map(|p| p.name)
                    .filter(|n| n.to_ascii_uppercase().starts_with(&prefix))
                    .collect()
            })
            .map_err(|e| e.to_string())
    }
    fn lock_volume(&mut self, letter: char) -> Result<(), String> {
        self.volume_letter = letter.to_ascii_uppercase();
        if self.locked.is_some() {
            return Ok(());
        }
        match WindowsHostState::lock_volume(self.volume_letter) {
            Ok(vol) => {
                self.locked = Some(vol);
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }
    fn unlock_volume(&mut self) -> Result<(), String> {
        // Drop LockedVolume → FSCTL_UNLOCK + CloseHandle (DT-8).
        self.locked = None;
        Ok(())
    }
    fn flush_and_dismount(&mut self) -> Result<(), String> {
        let vol = self
            .locked
            .as_ref()
            .ok_or_else(|| "volume is not exclusively locked".to_string())?;
        WindowsHostState::flush_and_dismount(vol).map_err(|e| e.to_string())
    }
    fn volume_locked(&self) -> bool {
        self.locked.is_some()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn serial_is_16_hex() {
        let s = serial_from_run_id("run-1");
        assert_eq!(s.len(), 16);
        assert!(s.iter().all(|c| c.is_ascii_hexdigit()));
    }
}
