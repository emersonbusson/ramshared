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
use crate::host_safety::{LockWaitDecision, lock_wait_decision, pagefile_may_target_volume};
use crate::proto::DiskParams;
use crate::runtime::{
    RunMode, RuntimeError, RuntimeErrorClass, RuntimePhase, RuntimeState, RuntimeSummary,
};
use crate::service::{
    DiskControl, PagefileGates, ServiceState, TeardownTarget, WipeVram, pagefile_refusal_to_runtime,
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

    let mut dlink = DriverLink::from_queue(q);
    let startup_lun_deadline = Duration::from_secs(60);
    let startup_lun_started = Instant::now();
    loop {
        // The startup LUN identity wait must pump I/O: Windows disk
        // enumeration may issue READs before Get-Disk exposes the device.
        let want_serial = serial_str.clone();
        let want_size = cfg.size_bytes;
        let startup_lun = readonly_host_call_with_io_pump(
            &mut link,
            &mut dlink,
            &mut backend,
            Duration::from_secs(5),
            move || WindowsHostState::find_lun(&want_serial, want_size),
        );
        let startup_lun_error = match startup_lun {
            Ok(Some(_)) => break,
            Ok(None) => "not enumerated".to_string(),
            Err(e) => e,
        };
        if startup_lun_started.elapsed() >= startup_lun_deadline {
            let mut message = format!(
                "startup LUN identity did not appear after {} ms: {}",
                startup_lun_deadline.as_millis(),
                startup_lun_error
            );
            if let Err(unreg_error) = link.unregister_queue() {
                message.push_str(&format!("; UNREGISTER failed: {unreg_error}"));
            }
            if let Err(destroy_error) = link.destroy_disk() {
                message.push_str(&format!("; DESTROY failed: {destroy_error}"));
                preserve_failed_safe("startup missing LUN could not destroy disk");
            }
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
        thread::sleep(Duration::from_millis(500));
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
        let teardown_started = Instant::now();
        row.begin_event("Stopping", utc_ms());
        sync_runtime_evidence(&mut row, &state);
        let _ = evidence.append(&row);
        teardown_diag(&format!(
            "Stopping: letter={} serial={} size={}",
            cfg.volume_letter.to_ascii_uppercase(),
            serial_str,
            cfg.size_bytes
        ));
        // Do NOT cancel_fetch / stop serving yet. CreateFile(\\.\S:) / FSCTL_LOCK
        // deadlocks if the product I/O loop is idle: NTFS waits on the miniport
        // and the miniport waits on this process's COMMIT loop.

        let teardown_letter = cfg.volume_letter.to_ascii_uppercase();
        let mut gates = match HostGates::new(
            teardown_letter,
            cfg.volume_mount_path.clone(),
            &serial_str,
            cfg.size_bytes,
        ) {
            Ok(g) => g,
            Err(e) => {
                teardown_diag(&format!("HostGates::new identity error: {e}"));
                state.healthy = false;
                state.phase = RuntimePhase::FailedSafe;
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                preserve_failed_safe("teardown identity construction failed");
            }
        };

        // Re-observe the exact letter-to-disk identity while the I/O pump stays
        // available. CREATE-time values are expectations, never OS identity.
        let identity_letter = teardown_letter;
        let identity_serial = serial_str.clone();
        let identity_size = cfg.size_bytes;
        let identity_mount_path = cfg.volume_mount_path.clone();
        let (observed, observed_disk_number, observed_volume_path) =
            match readonly_host_call_with_io_pump(
                &mut link,
                &mut dlink,
                &mut backend,
                Duration::from_secs(5),
                move || {
                    WindowsHostState::observe_product_volume(
                        identity_letter,
                        identity_mount_path.as_deref(),
                        &identity_serial,
                        identity_size,
                    )
                },
            ) {
                Ok(observed) => observed,
                Err(e) => {
                    refuse_stop_online(
                        &mut state,
                        &mut row,
                        &mut evidence,
                        &stop,
                        &format!("volume_identity_observation: {e}"),
                    );
                    continue;
                }
            };
        if let Err(e) =
            gates.accept_observed_identity(observed, observed_disk_number, observed_volume_path)
        {
            refuse_stop_online(
                &mut state,
                &mut row,
                &mut evidence,
                &stop,
                &format!("volume_identity: {e}"),
            );
            continue;
        }

        // Identity + Gate A (read-only, safe to refuse back to Online).
        if let Err(e) = gates.verify_volume_identity(teardown_letter) {
            teardown_diag(&format!(
                "teardown refused (code 7) resume Online: volume_identity: {e}"
            ));
            state.phase = RuntimePhase::Online;
            row.begin_event("Online", utc_ms());
            sync_runtime_evidence(&mut row, &state);
            row.error_class = Some("pagefile_safety".into());
            row.error_code = Some("7".into());
            let _ = evidence.append(&row);
            stop.store(false, Ordering::Release);
            continue;
        }
        let gate_a = readonly_host_call_with_io_pump(
            &mut link,
            &mut dlink,
            &mut backend,
            Duration::from_secs(5),
            WindowsHostState::active_pagefiles,
        )
        .and_then(|rows| gates.filter_pagefiles(rows));
        match gate_a {
            Ok(pf) if pf.is_empty() => {
                teardown_diag("teardown phase=GateA pagefiles_on_volume=0");
            }
            Ok(pf) => {
                teardown_diag(&format!(
                    "teardown refused (code 7) resume Online: gate_a_active: {}",
                    pf.join(",")
                ));
                state.phase = RuntimePhase::Online;
                row.begin_event("Online", utc_ms());
                sync_runtime_evidence(&mut row, &state);
                row.error_class = Some("pagefile_safety".into());
                row.error_code = Some("7".into());
                let _ = evidence.append(&row);
                stop.store(false, Ordering::Release);
                continue;
            }
            Err(e) => {
                teardown_diag(&format!(
                    "teardown refused (code 7) resume Online: gate_a_query: {e}"
                ));
                state.phase = RuntimePhase::Online;
                row.begin_event("Online", utc_ms());
                sync_runtime_evidence(&mut row, &state);
                row.error_class = Some("pagefile_safety".into());
                row.error_code = Some("7".into());
                let _ = evidence.append(&row);
                stop.store(false, Ordering::Release);
                continue;
            }
        }

        // Volume lock while pumping driver I/O so CreateFile can complete.
        teardown_diag(&format!(
            "teardown phase=VolumeLock begin (I/O pump) letter={teardown_letter}"
        ));
        let letter_for_lock = teardown_letter;
        let disk_for_lock = gates.expected_disk_number();
        let volume_path_for_lock = gates.volume_device_path().map(str::to_owned);
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = if let Some(path) = volume_path_for_lock {
                WindowsHostState::lock_product_volume_path(
                    &path,
                    letter_for_lock,
                    Some(disk_for_lock),
                )
            } else {
                WindowsHostState::lock_product_volume(letter_for_lock, Some(disk_for_lock))
            };
            let _ = tx.send(result);
        });
        let lock_res = loop {
            match rx.try_recv() {
                Ok(r) => break r,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    break Err(crate::windows_host::HostError::Volume(
                        "lock worker disconnected".into(),
                    ));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }

            if lock_wait_decision(teardown_started.elapsed(), Duration::from_secs(30), false)
                == LockWaitDecision::EnterFailedSafe
            {
                state.healthy = false;
                state.phase = RuntimePhase::FailedSafe;
                row.error_class = Some("teardown_timeout".into());
                row.error_code = Some("30s".into());
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                teardown_diag("FailedSafe: volume lock exceeded 30,000 ms; retaining I/O pump");
                preserve_failed_safe_with_io(
                    "volume lock worker remains in flight",
                    &mut link,
                    &mut dlink,
                    &mut backend,
                    &mut tenant,
                    &mut stream,
                    rx,
                );
            }
            match link.commit_and_fetch(Duration::from_millis(50)) {
                Ok(()) => {
                    let _ = dlink.commit_and_fetch(&mut backend);
                }
                Err(crate::windows_driver::IoctlError::Timeout) => {}
                Err(e) => {
                    teardown_diag(&format!("I/O pump during lock: {e}"));
                }
            }
        };
        if teardown_started.elapsed() >= Duration::from_secs(30) {
            drop(lock_res);
            refuse_stop_online(
                &mut state,
                &mut row,
                &mut evidence,
                &stop,
                "volume_lock_completed_after_30s_budget",
            );
            continue;
        }
        match lock_res {
            Ok(vol) => {
                gates.take_locked(vol);
                teardown_diag("teardown phase=VolumeLock OK");
            }
            Err(e) => {
                teardown_diag(&format!(
                    "teardown refused (code 7) resume Online: volume_lock: {e}"
                ));
                state.phase = RuntimePhase::Online;
                row.begin_event("Online", utc_ms());
                sync_runtime_evidence(&mut row, &state);
                row.error_class = Some("pagefile_safety".into());
                row.error_code = Some("7".into());
                let _ = evidence.append(&row);
                stop.store(false, Ordering::Release);
                continue;
            }
        }

        // Gate B is observed while the exact volume lock is held and while any
        // outstanding miniport request can still be drained. Cache the snapshot
        // so the destructive helper cannot silently re-query a weaker source.
        let gate_b_timeout = Duration::from_secs(30)
            .saturating_sub(teardown_started.elapsed())
            .min(Duration::from_secs(5));
        if gate_b_timeout.is_zero() {
            let _ = gates.unlock_volume();
            refuse_stop_online(
                &mut state,
                &mut row,
                &mut evidence,
                &stop,
                "gate_b_budget_exhausted",
            );
            continue;
        }
        let gate_b = readonly_host_call_with_io_pump(
            &mut link,
            &mut dlink,
            &mut backend,
            gate_b_timeout,
            WindowsHostState::active_pagefiles,
        )
        .and_then(|rows| gates.filter_pagefiles(rows));
        match gate_b {
            Ok(rows) if rows.is_empty() => gates.cache_pagefiles(rows),
            Ok(rows) => {
                let _ = gates.unlock_volume();
                refuse_stop_online(
                    &mut state,
                    &mut row,
                    &mut evidence,
                    &stop,
                    &format!("gate_b_active: {}", rows.join(",")),
                );
                continue;
            }
            Err(e) => {
                let _ = gates.unlock_volume();
                refuse_stop_online(
                    &mut state,
                    &mut row,
                    &mut evidence,
                    &stop,
                    &format!("gate_b_query: {e}"),
                );
                continue;
            }
        }

        // Locked: stop serving, then Gate B → dismount → unregister → destroy → wipe.
        let _ = link.cancel_fetch();
        let mut svc = ServiceState {
            lease: Some(crate::broker_tenant::LeaseState {
                lease: lease.lease,
                bytes: lease.bytes,
            }),
            disk_created: true,
            registered_queue: true,
            online: true,
        };
        let mut phases = Vec::new();
        let mut cfg_teardown = cfg.clone();
        cfg_teardown.volume_letter = teardown_letter;

        // Gate B + rest of destructive path (volume already locked).
        let teardown_result = {
            let mut disk_ctl = LinkDisk {
                link: &mut link,
                unregistered: false,
                destroyed: false,
            };
            let mut wipe = BackendWipe {
                backend: &mut backend,
            };
            teardown_after_lock(
                &cfg_teardown,
                &mut svc,
                &mut disk_ctl,
                &mut wipe,
                &mut gates,
                &mut phases,
            )
        };
        match teardown_result {
            Ok(()) => {
                // The disk and queue no longer reference the backend. Release
                // DeviceMem and verify restoration before LeaseRelease (DT-8).
                let device_mem = backend.into_inner();
                drop(device_mem);
                let free_after = match ctx.mem_info() {
                    Ok((free_after, _)) => free_after,
                    Err(e) => {
                        state.healthy = false;
                        state.phase = RuntimePhase::FailedSafe;
                        state.allocated_bytes = 0;
                        row.error_class = Some("cuda_restore_query".into());
                        row.error_code = Some(e.to_string());
                        sync_runtime_evidence(&mut row, &state);
                        let _ = evidence.append(&row);
                        preserve_failed_safe_lease(
                            "CUDA free restoration query failed after destroy",
                            &mut tenant,
                            &mut stream,
                        );
                    }
                };
                row.free_bytes = free_after as u64;
                if free_after.saturating_add(64 * 1024 * 1024) < free {
                    state.healthy = false;
                    state.phase = RuntimePhase::FailedSafe;
                    state.allocated_bytes = 0;
                    row.error_class = Some("cuda_restore_miss".into());
                    row.error_code = Some(format!("before={free} after={free_after}"));
                    sync_runtime_evidence(&mut row, &state);
                    let _ = evidence.append(&row);
                    preserve_failed_safe_lease(
                        "CUDA free bytes not restored within 64 MiB after destroy",
                        &mut tenant,
                        &mut stream,
                    );
                }
                if let Err(e) = tenant.release(&mut stream) {
                    state.healthy = false;
                    state.phase = RuntimePhase::FailedSafe;
                    state.allocated_bytes = 0;
                    row.error_class = Some("lease_release".into());
                    row.error_code = Some(e.to_string());
                    sync_runtime_evidence(&mut row, &state);
                    let _ = evidence.append(&row);
                    preserve_failed_safe_lease(
                        "destructive teardown completed but lease release failed",
                        &mut tenant,
                        &mut stream,
                    );
                }
                state.phase = RuntimePhase::Stopped;
                state.lease_id = None;
                state.allocated_bytes = 0;
                row.duration_ms = teardown_started.elapsed().as_millis() as u64;
                row.begin_event("Stopped", utc_ms());
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                teardown_diag("Stopped: teardown_after_lock + lease release OK");
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
                    let reason = e.to_string();
                    teardown_diag(&format!(
                        "teardown refused (code 7) resume Online: {reason}; phases={phases:?}"
                    ));
                    state.phase = RuntimePhase::Online;
                    row.begin_event("Online", utc_ms());
                    sync_runtime_evidence(&mut row, &state);
                    row.error_class = Some("pagefile_safety".into());
                    row.error_code = Some("7".into());
                    let _ = evidence.append(&row);
                    stop.store(false, Ordering::Release);
                    continue;
                }
                teardown_diag(&format!("teardown non-code7 failure → FailedSafe: {e}"));
                state.healthy = false;
                state.phase = RuntimePhase::FailedSafe;
                sync_runtime_evidence(&mut row, &state);
                let _ = evidence.append(&row);
                preserve_failed_safe("partial teardown failure");
            }
        }
    }
}

/// Gate B → flush/dismount → unregister → destroy → unlock → wipe.
/// Caller must already hold exclusive volume lock on `gates`.
fn teardown_after_lock(
    cfg: &WinDriveConfig,
    state: &mut ServiceState,
    disk: &mut dyn DiskControl,
    wipe: &mut dyn WipeVram,
    gates: &mut dyn PagefileGates,
    phases: &mut Vec<crate::service::TeardownPhase>,
) -> Result<(), crate::service::ProvisionError> {
    use crate::service::{ProvisionError, TeardownPhase};

    phases.push(TeardownPhase::GateB);
    match gates.active_pagefiles() {
        Ok(pf) if pf.is_empty() => {}
        Ok(pf) => {
            let _ = gates.unlock_volume();
            phases.push(TeardownPhase::ResumeOnline);
            return Err(ProvisionError::PagefileSafety(format!(
                "gate_b_active: {}",
                pf.join(",")
            )));
        }
        Err(e) => {
            let _ = gates.unlock_volume();
            phases.push(TeardownPhase::ResumeOnline);
            return Err(ProvisionError::PagefileSafety(format!("gate_b_query: {e}")));
        }
    }

    phases.push(TeardownPhase::FlushDismount);
    if let Err(e) = gates.flush_and_dismount() {
        let _ = gates.unlock_volume();
        phases.push(TeardownPhase::ResumeOnline);
        return Err(ProvisionError::PagefileSafety(format!(
            "flush_dismount: {e}"
        )));
    }

    if state.registered_queue {
        phases.push(TeardownPhase::Unregister);
        disk.unregister_queue().map_err(ProvisionError::Disk)?;
        state.registered_queue = false;
    }
    if state.disk_created {
        phases.push(TeardownPhase::Destroy);
        disk.destroy_disk().map_err(ProvisionError::Disk)?;
        state.disk_created = false;
    }

    phases.push(TeardownPhase::Unlock);
    gates
        .unlock_volume()
        .map_err(|e| ProvisionError::Disk(format!("unlock: {e}")))?;

    phases.push(TeardownPhase::Wipe);
    wipe.zero().map_err(ProvisionError::Disk)?;

    phases.push(TeardownPhase::Release);
    state.lease = None;
    state.online = false;
    let _ = cfg; // letter already applied by caller
    Ok(())
}

/// Unbuffered ProgramData line for stop/teardown classification (lab + force-kill safe).
fn teardown_diag(msg: &str) {
    use std::io::Write;
    let path = std::path::Path::new(r"C:\ProgramData\RamShared\teardown-diag.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let ts = utc_ms();
        let _ = writeln!(f, "{ts} {msg}");
        let _ = f.flush();
    }
    eprintln!("{msg}");
    let _ = std::io::stderr().flush();
}

fn preserve_failed_safe(reason: &str) -> ! {
    teardown_diag(&format!(
        "FailedSafe: {reason}; preserving disk, allocation, and lease until supervised reboot"
    ));
    loop {
        thread::park_timeout(Duration::from_secs(1));
    }
}

fn refuse_stop_online(
    state: &mut RuntimeState,
    row: &mut RuntimeEvidence,
    evidence: &mut EvidenceWriter,
    stop: &AtomicBool,
    reason: &str,
) {
    teardown_diag(&format!(
        "teardown refused (code 7) resume Online: {reason}"
    ));
    state.phase = RuntimePhase::Online;
    row.begin_event("Online", utc_ms());
    sync_runtime_evidence(row, state);
    row.error_class = Some("pagefile_safety".into());
    row.error_code = Some("7".into());
    let _ = evidence.append(row);
    stop.store(false, Ordering::Release);
}

fn readonly_host_call_with_io_pump<T, M, F>(
    link: &mut WindowsDriverLink,
    dlink: &mut DriverLink<WindowsMappedQueue>,
    backend: &mut VramBackend<M>,
    timeout: Duration,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    M: ramshared_vram::VramMemory,
    F: FnOnce() -> Result<T, crate::windows_host::HostError> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(operation());
    });
    let started = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(result) => return result.map_err(|e| e.to_string()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("read-only host worker disconnected".into());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if started.elapsed() >= timeout {
            return Err(format!(
                "read-only host observation timed out after {} ms",
                timeout.as_millis()
            ));
        }
        match link.commit_and_fetch(Duration::from_millis(50)) {
            Ok(()) => dlink
                .commit_and_fetch(backend)
                .map(|_| ())
                .map_err(|e| format!("I/O pump during host observation: {e}"))?,
            Err(crate::windows_driver::IoctlError::Timeout) => {}
            Err(e) => return Err(format!("I/O pump during host observation: {e}")),
        }
    }
}

fn preserve_failed_safe_with_io<M>(
    reason: &str,
    link: &mut WindowsDriverLink,
    dlink: &mut DriverLink<WindowsMappedQueue>,
    backend: &mut VramBackend<M>,
    tenant: &mut BrokerTenant,
    stream: &mut BrokStream,
    receiver: std::sync::mpsc::Receiver<
        Result<crate::windows_host::LockedVolume, crate::windows_host::HostError>,
    >,
) -> !
where
    M: ramshared_vram::VramMemory,
{
    teardown_diag(&format!(
        "FailedSafe: {reason}; preserving I/O, disk, allocation, and lease until supervised reboot"
    ));
    let mut lock_receiver = Some(receiver);
    let mut last_heartbeat = Instant::now();
    loop {
        if let Some(rx) = lock_receiver.as_ref() {
            match rx.try_recv() {
                Ok(result) => {
                    drop(result);
                    teardown_diag("FailedSafe: late volume-lock result drained and released");
                    lock_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    lock_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        match link.commit_and_fetch(Duration::from_millis(50)) {
            Ok(()) => {
                if let Err(e) = dlink.commit_and_fetch(backend) {
                    teardown_diag(&format!("FailedSafe I/O pump error: {e}"));
                }
            }
            Err(crate::windows_driver::IoctlError::Timeout) => {}
            Err(e) => teardown_diag(&format!("FailedSafe COMMIT error: {e}")),
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(1) {
            if let Err(e) = tenant.heartbeat_psi(stream) {
                teardown_diag(&format!("FailedSafe broker heartbeat error: {e}"));
            } else if let Err(e) = stream.flush() {
                teardown_diag(&format!("FailedSafe broker flush error: {e}"));
            }
            last_heartbeat = Instant::now();
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn preserve_failed_safe_lease(
    reason: &str,
    tenant: &mut BrokerTenant,
    stream: &mut BrokStream,
) -> ! {
    teardown_diag(&format!(
        "FailedSafe: {reason}; allocation is free but lease ownership is retained"
    ));
    loop {
        if let Err(e) = tenant.heartbeat_psi(stream) {
            teardown_diag(&format!("FailedSafe broker heartbeat error: {e}"));
        } else if let Err(e) = stream.flush() {
            teardown_diag(&format!("FailedSafe broker flush error: {e}"));
        }
        thread::sleep(Duration::from_secs(1));
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
    volume_mount_path: Option<std::path::PathBuf>,
    volume_device_path: Option<String>,
    target_serial: String,
    target_size: u64,
    identity_verified: bool,
    expected_disk_number: Option<u32>,
    pagefile_snapshot: Mutex<Option<Vec<String>>>,
}

impl HostGates {
    fn new(
        volume_letter: char,
        volume_mount_path: Option<std::path::PathBuf>,
        serial: &str,
        size_bytes: u64,
    ) -> Result<Self, String> {
        // Validate target parameters (letter/serial/size shape).
        let _ = TeardownTarget::new(volume_letter, serial, size_bytes)?;

        Ok(Self {
            locked: None,
            volume_letter,
            volume_mount_path,
            volume_device_path: None,
            target_serial: serial.to_string(),
            target_size: size_bytes,
            identity_verified: false,
            expected_disk_number: None,
            pagefile_snapshot: Mutex::new(None),
        })
    }

    fn target_serial(&self) -> &str {
        &self.target_serial
    }

    fn target_size(&self) -> u64 {
        self.target_size
    }

    fn accept_observed_identity(
        &mut self,
        observed: crate::service::ObservedVolumeIdentity,
        disk_number: u32,
        volume_device_path: String,
    ) -> Result<(), String> {
        self.target.verify_unique(&[observed])?;
        if !volume_device_path.starts_with(r"\\?\Volume{") || !volume_device_path.ends_with('}') {
            return Err("observed volume device path is invalid".into());
        }
        self.identity_verified = true;
        self.expected_disk_number = Some(disk_number);
        self.volume_device_path = Some(volume_device_path);
        Ok(())
    }

    fn expected_disk_number(&self) -> u32 {
        self.expected_disk_number.unwrap_or(u32::MAX)
    }

    fn volume_device_path(&self) -> Option<&str> {
        self.volume_mount_path
            .as_ref()
            .and(self.volume_device_path.as_deref())
    }

    fn filter_pagefiles(
        &self,
        rows: Vec<crate::windows_host::PagefileIdentity>,
    ) -> Result<Vec<String>, String> {
        rows.into_iter().try_fold(Vec::new(), |mut found, row| {
            if pagefile_may_target_volume(&row.name, self.volume_letter)? {
                found.push(row.name);
            }
            Ok(found)
        })
    }

    fn cache_pagefiles(&self, rows: Vec<String>) {
        if let Ok(mut snapshot) = self.pagefile_snapshot.lock() {
            *snapshot = Some(rows);
        }
    }

    /// Install a volume lock obtained while the I/O pump was still running.
    fn take_locked(&mut self, vol: crate::windows_host::LockedVolume) {
        self.volume_letter = vol.letter;
        self.locked = Some(vol);
    }
}

impl PagefileGates for HostGates {
    fn verify_volume_identity(&self, letter: char) -> Result<(), String> {
        teardown_diag(&format!(
            "teardown phase=Identity letter={letter} serial={} size={}",
            self.target_serial(),
            self.target_size()
        ));
        if letter.to_ascii_uppercase() != self.volume_letter || !self.identity_verified {
            let msg = "live letter-to-disk identity was not verified".to_string();
            teardown_diag(&format!("teardown phase=Identity FAIL: {msg}"));
            return Err(msg);
        }
        teardown_diag(&format!(
            "teardown phase=Identity OK (live exact) serial={} size={}",
            self.target_serial(),
            self.target_size()
        ));
        Ok(())
    }

    fn active_pagefiles(&self) -> Result<Vec<String>, String> {
        // Storage-only DT-8: refuse only pagefiles on the product volume letter,
        // not the system C: pagefile (which is expected on a normal host).
        let letter = self
            .locked
            .as_ref()
            .map(|v| v.letter)
            .unwrap_or(self.volume_letter);
        let gate = if self.locked.is_some() {
            "GateB"
        } else {
            "GateA"
        };
        teardown_diag(&format!(
            "teardown phase={gate} pagefile query letter={letter}"
        ));
        if let Ok(mut snapshot) = self.pagefile_snapshot.lock() {
            if let Some(filtered) = snapshot.take() {
                teardown_diag(&format!(
                    "teardown phase={gate} pagefiles_on_volume={}",
                    filtered.len()
                ));
                return Ok(filtered);
            }
        } else {
            return Err("pagefile snapshot lock poisoned".into());
        }
        WindowsHostState::active_pagefiles()
            .map_err(|e| e.to_string())
            .and_then(|v| self.filter_pagefiles(v))
            .map_err(|e| {
                teardown_diag(&format!("teardown phase={gate} FAIL: {e}"));
                e.to_string()
            })
    }
    fn lock_volume(&mut self, letter: char) -> Result<(), String> {
        self.volume_letter = letter.to_ascii_uppercase();
        if self.locked.is_some() {
            return Ok(());
        }
        teardown_diag(&format!(
            "teardown phase=VolumeLock begin letter={}",
            self.volume_letter
        ));
        // Brief settle so filesystem handles from the last I/O round close.
        thread::sleep(Duration::from_millis(100));
        // Direct lock (no helper-thread timeout). Abandoned CreateFile threads
        // from timed-out attempts wedged the volume; pre-stop harness lock_ok
        // proves CreateFile returns promptly when the stack is not piled up.
        let result = if let Some(path) = self.volume_device_path() {
            WindowsHostState::lock_product_volume_path(
                path,
                self.volume_letter,
                self.expected_disk_number,
            )
        } else {
            WindowsHostState::lock_product_volume(self.volume_letter, self.expected_disk_number)
        };
        match result {
            Ok(vol) => {
                self.locked = Some(vol);
                teardown_diag("teardown phase=VolumeLock OK");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                teardown_diag(&format!("teardown phase=VolumeLock FAIL: {msg}"));
                Err(msg)
            }
        }
    }
    fn unlock_volume(&mut self) -> Result<(), String> {
        // Drop LockedVolume → FSCTL_UNLOCK + CloseHandle (DT-8).
        teardown_diag("teardown phase=Unlock");
        self.locked = None;
        Ok(())
    }
    fn flush_and_dismount(&mut self) -> Result<(), String> {
        teardown_diag("teardown phase=FlushDismount begin");
        let vol = self
            .locked
            .as_ref()
            .ok_or_else(|| "volume is not exclusively locked".to_string())?;
        match WindowsHostState::flush_and_dismount(vol) {
            Ok(()) => {
                teardown_diag("teardown phase=FlushDismount OK");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                teardown_diag(&format!("teardown phase=FlushDismount FAIL: {msg}"));
                Err(msg)
            }
        }
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
