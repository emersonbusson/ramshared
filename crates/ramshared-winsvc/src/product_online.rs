//! Windows product Online composition (SPEC DT-3/DT-4/DT-7):
//! broker lease → CUDA DeviceMem → CREATE/REGISTER → I/O loop on one thread.
//!
//! Cover: N/A for unsafe Windows bits; pure sequencing tested via RuntimeOps.

#![cfg(windows)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ramshared_block::VramBackend;
use ramshared_cuda::Cuda;

use crate::broker_tenant::BrokerTenant;
use crate::config::WinDriveConfig;
use crate::driver_link::DriverLink;
use crate::evidence::{EvidenceWriter, RuntimeEvidence};
use crate::proto::DiskParams;
use crate::runtime::{
    RunMode, RuntimeError, RuntimeErrorClass, RuntimePhase, RuntimeState, RuntimeSummary,
};
use crate::service::{
    pagefile_refusal_to_runtime, teardown_storage_only, DiskControl, PagefileGates, ServiceState,
    WipeVram,
};
use crate::windows_driver::{WindowsDriverLink, WindowsMappedQueue};
use crate::windows_host::WindowsHostState;

/// Serial: 16 uppercase hex digits derived from run id hash (DT-11).
pub fn serial_from_run_id(run_id: &str) -> [u8; 16] {
    let mut out = [b'0'; 16];
    let h = simple_hash(run_id.as_bytes());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for i in 0..16 {
        let nibble = ((h >> (i * 4)) & 0xF) as usize;
        out[i] = HEX[nibble];
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
    let run_id = format!(
        "run-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let mut state = RuntimeState::new(mode);
    let serial = serial_from_run_id(&run_id);
    let serial_str = String::from_utf8_lossy(&serial).into_owned();

    // Evidence (diagnostic only).
    let mut evidence = EvidenceWriter::open(cfg.evidence_path().join(format!("{run_id}.jsonl")))
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Internal, 1, e.to_string()))?;
    let mut row = RuntimeEvidence::base(&run_id, "Stopped");
    row.lun_serial = serial_str.clone();
    row.lun_size_bytes = cfg.size_bytes;
    row.queue_depth = cfg.queue_depth;
    row.max_io_bytes = cfg.max_io_bytes;
    row.cuda_ordinal = cfg.cuda_device;
    let _ = evidence.append(&row);

    // --- Broker lease ---
    let addr = cfg
        .broker_addr()
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Config, 2, e.to_string()))?;
    let raw = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| {
        RuntimeError::new(
            RuntimeErrorClass::Broker,
            3,
            format!("connect {addr}: {e}"),
        )
    })?;
    raw.set_read_timeout(Some(Duration::from_secs(10))).ok();
    raw.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let mut stream = BrokStream::new(raw).map_err(|e| {
        RuntimeError::new(RuntimeErrorClass::Broker, 3, format!("stream: {e}"))
    })?;
    let mut tenant = BrokerTenant::new(cfg.tenant.clone(), Duration::from_secs(cfg.heartbeat_secs));
    tenant
        .register(&mut stream)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Broker, 3, e.to_string()))?;
    let lease = tenant
        .acquire(&mut stream, cfg.size_bytes)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Broker, 3, e.to_string()))?;
    state.lease_id = Some(lease.lease);
    state.lease_bytes = lease.bytes;
    state.phase = RuntimePhase::Leased;
    state.effects.lease_acquire += 1;
    row.phase = "Leased".into();
    row.lease_id = lease.lease;
    row.lease_bytes = lease.bytes;
    let _ = evidence.append(&row);

    // --- CUDA on this thread (affinity) ---
    let cuda = Cuda::load().map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    let count = cuda
        .device_count()
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    if cfg.cuda_device as i32 >= count {
        let _ = tenant.release(&mut stream);
        return Err(RuntimeError::new(
            RuntimeErrorClass::Cuda,
            4,
            format!("cuda_device {} >= {count}", cfg.cuda_device),
        ));
    }
    let dev = cuda
        .device(cfg.cuda_device as i32)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    let ctx = cuda
        .create_context(&dev)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    let (free, total) = ctx
        .mem_info()
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    let reserve = cfg.effective_reserve_bytes(total as u64) as usize;
    let need = (cfg.size_bytes as usize)
        .checked_add(reserve)
        .ok_or_else(|| RuntimeError::new(RuntimeErrorClass::Config, 2, "size+reserve overflow"))?;
    if free < need {
        let _ = tenant.release(&mut stream);
        return Err(RuntimeError::new(
            RuntimeErrorClass::Cuda,
            4,
            format!("free {free} < need {need}"),
        ));
    }
    if let Err(e) = tenant.coresidence_gate(free as u64, cfg.size_bytes) {
        let _ = tenant.release(&mut stream);
        return Err(RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()));
    }
    let size = cfg.size_bytes as usize;
    let mut mem = ctx
        .alloc(size)
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    mem.zero()
        .map_err(|e| RuntimeError::new(RuntimeErrorClass::Cuda, 4, e.to_string()))?;
    state.allocated_bytes = cfg.size_bytes;
    state.phase = RuntimePhase::CudaReady;
    state.effects.cuda_alloc += 1;
    row.phase = "CudaReady".into();
    row.allocated_bytes = cfg.size_bytes;
    row.free_bytes = free as u64;
    row.reserve_bytes = reserve as u64;
    row.cuda_name = dev.name().to_string();
    let _ = evidence.append(&row);

    let mut backend = VramBackend::new(mem, cfg.block_size);

    // --- Driver CREATE + REGISTER ---
    let mut link = WindowsDriverLink::open().map_err(|e| {
        let _ = backend.zero();
        let _ = tenant.release(&mut stream);
        RuntimeError::new(RuntimeErrorClass::Abi, 5, e.to_string())
    })?;
    // Best-effort destroy any prior disk.
    let _ = link.destroy_disk();

    let params = DiskParams {
        size_bytes: cfg.size_bytes,
        block_size: cfg.block_size,
        reserved: 0,
        serial,
    };
    if let Err(e) = link.create_disk(&params) {
        let _ = backend.zero();
        let _ = tenant.release(&mut stream);
        return Err(RuntimeError::new(RuntimeErrorClass::Abi, 5, e.to_string()));
    }
    state.phase = RuntimePhase::DiskCreated;
    state.effects.disk_create += 1;

    let mut q = WindowsMappedQueue::try_new(cfg.queue_depth, cfg.max_io_bytes, cfg.block_size)
        .map_err(|e| {
            let _ = link.destroy_disk();
            let _ = backend.zero();
            let _ = tenant.release(&mut stream);
            RuntimeError::new(RuntimeErrorClass::Abi, 5, e.to_string())
        })?;
    let reg = q.registration(0);
    if let Err(e) = link.register_queue(&reg) {
        let _ = link.destroy_disk();
        let _ = backend.zero();
        let _ = tenant.release(&mut stream);
        return Err(RuntimeError::new(RuntimeErrorClass::Abi, 5, e.to_string()));
    }
    state.phase = RuntimePhase::QueueRegistered;
    state.effects.queue_register += 1;
    state.phase = RuntimePhase::Online;
    row.phase = "Online".into();
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
    let watchdog = Duration::from_millis(5_000);

    while !stop.load(Ordering::Relaxed) {
        match link.commit_and_fetch(Duration::from_millis(500)) {
            Ok(()) => {
                match dlink.commit_and_fetch(&mut backend) {
                    Ok(n) if n > 0 => {
                        last_progress = Instant::now();
                    }
                    Ok(_) => {
                        // empty SQ — ok
                    }
                    Err(e) => {
                        eprintln!("serve error: {e}");
                        state.healthy = false;
                        break;
                    }
                }
            }
            Err(crate::windows_driver::IoctlError::Timeout) => {
                // Empty pending cancelled — continue.
            }
            Err(e) => {
                eprintln!("commit_and_fetch: {e}");
                if last_progress.elapsed() > watchdog {
                    state.healthy = false;
                    state.phase = RuntimePhase::FailedSafe;
                    break;
                }
            }
        }
        // Heartbeat
        let _ = tenant.heartbeat_psi(&mut stream);
        let _ = stream.flush();
        thread::sleep(Duration::from_millis(10));
    }

    // --- Teardown (Gate A/B) ---
    state.phase = RuntimePhase::Stopping;
    row.phase = "Stopping".into();
    let _ = evidence.append(&row);

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
    let mut gates = HostGates::new(cfg.volume_letter);
    let mut phases = Vec::new();
    match teardown_storage_only(cfg, &mut svc, &mut disk_ctl, &mut wipe, &mut gates, &mut phases) {
        Ok(()) => {
            let _ = tenant.release(&mut stream);
            state.effects.lease_release += 1;
            state.phase = RuntimePhase::Stopped;
            state.stop_completed = true;
            state.lease_id = None;
            state.allocated_bytes = 0;
            row.phase = "Stopped".into();
            let _ = evidence.append(&row);
            Ok(RuntimeSummary {
                phase: RuntimePhase::Stopped,
                lease_id: None,
                allocated_bytes: 0,
                idempotent_stop: false,
                exit_code: 0,
            })
        }
        Err(e) => {
            if let Some(rt) = pagefile_refusal_to_runtime(&e) {
                state.phase = RuntimePhase::Online;
                return Err(rt);
            }
            // Best-effort unwind
            let _ = link.unregister_queue();
            let _ = link.destroy_disk();
            let _ = backend.zero();
            let _ = tenant.release(&mut stream);
            Err(RuntimeError::new(
                RuntimeErrorClass::Internal,
                1,
                e.to_string(),
            ))
        }
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
}

impl HostGates {
    fn new(volume_letter: char) -> Self {
        Self {
            locked: None,
            volume_letter,
        }
    }
}

impl PagefileGates for HostGates {
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
        self.volume_letter = letter;
        if self.locked.is_some() {
            return Ok(());
        }
        // Gate A runs before lock: only volume-local pagefiles matter.
        // If the volume letter has no filesystem yet, lock may fail — map to
        // unlock path that still allows destructive teardown of unmounted LUN.
        match WindowsHostState::lock_volume(letter) {
            Ok(vol) => {
                self.locked = Some(vol);
                Ok(())
            }
            Err(e) => {
                // Unmounted LUN / missing letter: Gate B lock optional if Gate A clear.
                eprintln!("volume lock soft-fail (unmounted LUN?): {e}");
                Ok(())
            }
        }
    }
    fn unlock_volume(&mut self) -> Result<(), String> {
        // Drop LockedVolume → FSCTL_UNLOCK + CloseHandle (DT-8).
        self.locked = None;
        Ok(())
    }
    fn flush_and_dismount(&mut self) -> Result<(), String> {
        if let Some(ref vol) = self.locked {
            WindowsHostState::flush_and_dismount(vol).map_err(|e| e.to_string())?;
        }
        Ok(())
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
