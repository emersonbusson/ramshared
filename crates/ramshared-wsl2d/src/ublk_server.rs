//! RAM backend and I/O service logic for the ublk loop.
//!
//! `serve_request` is pure: given a `Request` and the data buffer, serves against a
//! `BlockBackend` and returns the `result` (bytes `>= 0`, or `-errno`) that the COMMIT
//! must load. `RamBackend` validates the loop without CUDA; `spawn_ublk_worker` is the
//! worker DT-3 (thread owning the backend) that will be used with `VramBackend`.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ramshared_block::{BlockBackend, Command, Request};
use ramshared_cuda::Cuda;
use ramshared_vram::VramMemory;

use crate::backend::RamBackend;
use crate::swap::spawn_swapoff;
use crate::ublk;
use crate::{
    CANARY_BYTES, CANARY_EVERY, Cadence, Canary, CanaryProbe, ResidencyConfig, ResidencySampler,
    Verdict, VramBackend,
};

const EIO: i32 = -5;
const EINVAL: i32 = -22;

/// Serves a ublk `Request` against any [`BlockBackend`] using `buf` (where data
/// lives) and returns the COMMIT `result`: transferred bytes (`>= 0`) or
/// `-errno`. Serves **in-place** in the buffer (no alloc in the hot path — DT-8). `buf` is the
/// tag buffer in the single-threaded loop, or a worker buffer in DT-3.
///
/// In WRITE, `buf` already contains the data (kernel copied from the bio); in READ, the backend
/// populates `buf` and the kernel copies `result` bytes back on COMMIT — which is why
/// `result` must be exactly the bytes served.
pub fn serve_request<B: BlockBackend + ?Sized>(
    req: &Request,
    backend: &mut B,
    buf: &mut [u8],
) -> i32 {
    let len = req.len as usize;
    if len > buf.len() {
        return EINVAL; // request larger than the available buffer
    }

    let served = match req.cmd {
        Command::Read => backend.read_at(req.offset, &mut buf[..len]).map(|()| len),
        Command::Write => backend.write_at(req.offset, &buf[..len]).map(|()| len),
        Command::Flush => backend.flush().map(|()| 0),
        Command::Trim => return 0, // discard: safe no-op in the MVP
        Command::Disc | Command::Unknown(_) => return EINVAL,
    };

    match served {
        Ok(bytes) => i32::try_from(bytes).unwrap_or(EIO),
        Err(_) => EIO,
    }
}

/// Handle of the ublk server thread; `join` waits for the loop to terminate (upon receiving the
/// abort from STOP/DEL_DEV) and returns the `RamBackend` for inspection.
pub struct ServerHandle {
    thread: JoinHandle<io::Result<RamBackend>>,
}

impl ServerHandle {
    pub fn join(self) -> io::Result<RamBackend> {
        match self.thread.join() {
            Ok(result) => result,
            Err(_) => Err(io::Error::other("server thread panicked")),
        }
    }
}

/// Opens `char_path`, creates the `UblkServer` and runs the service loop in its own
/// thread (sole owner of the ring, DT-3). The thread submits FETCH, serves each request
/// against `backend` and re-arms via COMMIT_AND_FETCH; terminates upon receiving the abort
/// (`UBLK_IO_RES_ABORT`) triggered by STOP/DEL_DEV.
pub fn spawn_server(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    backend: RamBackend,
) -> io::Result<ServerHandle> {
    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let server = ramshared_uring::UblkServer::new(char_dev.as_raw_fd(), queue_depth, buf_size)?;

    let thread = thread::spawn(move || {
        // Keeps the char device open while the loop uses the ring (dropped after).
        let _char_dev = char_dev;
        run_server_loop(server, backend)
    });

    Ok(ServerHandle { thread })
}

fn run_server_loop(
    mut server: ramshared_uring::UblkServer,
    mut backend: RamBackend,
) -> io::Result<RamBackend> {
    server.submit_initial_fetch()?;

    loop {
        let completions = server.drain();
        if completions.is_empty() {
            thread::sleep(Duration::from_micros(200));
            continue;
        }

        for completion in completions {
            if completion.result == ublk::UBLK_IO_RES_ABORT {
                return Ok(backend); // teardown: STOP/DEL_DEV aborted the FETCH
            }
            if completion.result < 0 {
                return Err(io::Error::other(format!(
                    "FETCH falhou: {}",
                    completion.result
                )));
            }

            // result == UBLK_IO_RES_OK (0): there is a request ready at the tag.
            let iod = ublk::IoDesc::from_ne_bytes(server.io_desc_bytes(completion.tag))
                .ok_or_else(|| io::Error::other("io-desc invalido no mmap"))?;
            let result = match iod.to_block_request(completion.tag) {
                Ok(req) => serve_request(&req, &mut backend, server.buffer_mut(completion.tag)),
                Err(_) => EINVAL, // ublk op without safe equivalence
            };
            server.commit_and_fetch(completion.tag, result)?;
        }
    }
}

/// Reply from worker DT-3 to the ring owner. `buf` is the buffer yielded by the ring
/// owner, returned for recycling (pool without alloc in the hot path — DT-8). When
/// `is_read` and `result >= 0`, `buf` carries the `result` read bytes that the ring owner
/// copies to the tag buffer before `commit_and_fetch`.
#[derive(Clone, Debug)]
pub struct WorkerReply {
    pub qid: u16,
    pub tag: u16,
    pub result: i32,
    pub buf: Vec<u8>,
    pub is_read: bool,
}

/// Handle of the worker thread DT-3; `join` waits for the worker to terminate (when the `IoWork`
/// channel closes) and returns the backend.
pub struct WorkerHandle<B> {
    thread: JoinHandle<B>,
}

impl<B> WorkerHandle<B> {
    pub fn join(self) -> io::Result<B> {
        self.thread
            .join()
            .map_err(|_| io::Error::other("ublk worker panicked"))
    }
}

/// Starts worker DT-3: the thread owning `backend` (the only one touching VRAM/CUDA).
/// Receives `IoWork` through the channel, serves against the `backend` and returns `WorkerReply`.
/// Terminates when `work_rx` closes (the ring owner dropped) or `reply_tx` breaks.
pub fn spawn_ublk_worker<B: BlockBackend + Send + 'static>(
    mut backend: B,
    work_rx: Receiver<ublk::IoWork>,
    reply_tx: Sender<WorkerReply>,
) -> WorkerHandle<B> {
    let thread = thread::spawn(move || {
        worker_loop(&mut backend, work_rx, reply_tx);
        backend
    });
    WorkerHandle { thread }
}

fn worker_loop<B: BlockBackend>(
    backend: &mut B,
    work_rx: Receiver<ublk::IoWork>,
    reply_tx: Sender<WorkerReply>,
) {
    while let Ok(mut work) = work_rx.recv() {
        // `payload` is the buffer yielded by the ring owner, already sized to `req.len`:
        // in WRITE it carries data from the bio; in READ the backend populates it. The worker
        // serves in-place and returns the same buffer — no alloc here (DT-8).
        let result = serve_request(&work.req, backend, &mut work.payload);
        let is_read = work.req.cmd == Command::Read;

        let reply = WorkerReply {
            qid: work.qid,
            tag: work.tag,
            result,
            buf: work.payload,
            is_read,
        };
        if reply_tx.send(reply).is_err() {
            break; // ring owner dropped
        }
    }
}

const RING_CHAN_CAP: usize = 64;

/// Handle of the DT-3 server (ring owner + worker). `join` waits for the ring owner
/// to terminate (on abort of STOP/DEL_DEV), which closes the channel and terminates the worker,
/// and returns the backend.
pub struct ServerHandleDt3<B> {
    ring: JoinHandle<io::Result<()>>,
    worker: WorkerHandle<B>,
}

impl<B> ServerHandleDt3<B> {
    pub fn join(self) -> io::Result<B> {
        self.ring
            .join()
            .map_err(|_| io::Error::other("ring owner panicked"))??;
        self.worker.join()
    }
}

/// Starts the ublk server in the DT-3 architecture: a **ring owner** thread (owner of the
/// `UblkServer`) that drains CQEs, sends `IoWork` to the **worker** (thread owning the
/// `backend`, the only one touching VRAM/CUDA), and completes via `COMMIT_AND_FETCH` with the
/// returned data. Works with any `BlockBackend` (RAM or VRAM).
pub fn spawn_server_dt3<B: BlockBackend + Send + 'static>(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    backend: B,
) -> io::Result<ServerHandleDt3<B>> {
    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let server = ramshared_uring::UblkServer::new(char_dev.as_raw_fd(), queue_depth, buf_size)?;

    let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(RING_CHAN_CAP);
    let (reply_tx, reply_rx) = mpsc::channel::<WorkerReply>();
    let worker = spawn_ublk_worker(backend, work_rx, reply_tx);

    let ring = thread::spawn(move || {
        // The char device remains open while the ring lives; `work_tx` drops upon returning
        // (terminates the worker).
        let _char_dev = char_dev;
        run_ring_owner(server, queue_depth, buf_size, work_tx, reply_rx)
    });

    Ok(ServerHandleDt3 { ring, worker })
}

fn run_ring_owner(
    mut server: ramshared_uring::UblkServer,
    queue_depth: u16,
    buf_size: usize,
    work_tx: SyncSender<ublk::IoWork>,
    reply_rx: Receiver<WorkerReply>,
) -> io::Result<()> {
    server.submit_initial_fetch()?;

    // Pool of recycled buffers (DT-8): pre-warms `queue_depth` buffers of
    // `buf_size`. Each request takes one from the pool on dispatch and returns it on COMMIT —
    // zero malloc/free in the hot path under steady state. The pool never empties because the number
    // of requests inflight is limited to `queue_depth` (pool.len() + in_flight == qd).
    let mut buf_pool: Vec<Vec<u8>> = (0..queue_depth).map(|_| vec![0u8; buf_size]).collect();

    let mut in_flight = 0u32;
    loop {
        if in_flight > 0 {
            // There is a request inflight: blocks on the worker's reply (no poll/spin).
            match reply_rx.recv() {
                Ok(reply) => {
                    in_flight -= 1;
                    commit_reply(&mut server, reply, &mut buf_pool)?;
                }
                Err(_) => return Err(io::Error::other("worker encerrou inesperadamente")),
            }
            // Drains additional replies already ready, without blocking.
            while let Ok(reply) = reply_rx.try_recv() {
                in_flight -= 1;
                commit_reply(&mut server, reply, &mut buf_pool)?;
            }
        } else {
            // Idle: blocks until the next CQE (request served or abort).
            for completion in server.wait_and_drain()? {
                if completion.result == ublk::UBLK_IO_RES_ABORT {
                    return Ok(()); // teardown: STOP/DEL_DEV aborted the FETCH
                }
                if completion.result < 0 {
                    return Err(io::Error::other(format!(
                        "FETCH falhou: {}",
                        completion.result
                    )));
                }
                if dispatch_request(&mut server, completion.tag, &work_tx, &mut buf_pool)? {
                    in_flight += 1;
                }
            }
        }
    }
}

/// Copies READ data (if any) to the tag buffer, completes via COMMIT, and
/// returns the buffer to the pool (without dealloc — preserves capacity).
fn commit_reply(
    server: &mut ramshared_uring::UblkServer,
    reply: WorkerReply,
    buf_pool: &mut Vec<Vec<u8>>,
) -> io::Result<()> {
    if reply.is_read && reply.result >= 0 {
        let n = usize::try_from(reply.result).unwrap_or(0);
        let tag_buf = server.buffer_mut(reply.tag);
        let n = n.min(reply.buf.len()).min(tag_buf.len());
        tag_buf[..n].copy_from_slice(&reply.buf[..n]);
    }
    server.commit_and_fetch(reply.tag, reply.result)?;
    // Recycles the buffer: clears the len but preserves capacity (no free).
    let mut buf = reply.buf;
    buf.clear();
    buf_pool.push(buf);
    Ok(())
}

/// Reads the io-desc of `tag`, takes a recycled buffer from the pool sized to `len`
/// (copying the WRITE payload from the tag buffer), and sends it to the worker. Returns `true`
/// if work was sent, `false` if it rejected the request (already completed with error; the
/// buffer, if it was taken from the pool, goes back to it).
fn dispatch_request(
    server: &mut ramshared_uring::UblkServer,
    tag: u16,
    work_tx: &SyncSender<ublk::IoWork>,
    buf_pool: &mut Vec<Vec<u8>>,
) -> io::Result<bool> {
    let iod = ublk::IoDesc::from_ne_bytes(server.io_desc_bytes(tag))
        .ok_or_else(|| io::Error::other("io-desc invalido no mmap"))?;
    let req = match iod.to_block_request(tag) {
        Ok(req) => req,
        Err(_) => {
            server.commit_and_fetch(tag, -22)?; // EINVAL (no buffer taken from the pool)
            return Ok(false);
        }
    };

    // Takes a recycled buffer and sizes it to `len`. `unwrap_or_default` only allocates during
    // warming (empty pool); in steady state, the pre-warming guarantees one is available.
    let len = req.len as usize;
    let mut buf = buf_pool.pop().unwrap_or_default();
    buf.clear();
    buf.resize(len, 0);

    // WRITE: kernel already copied bio->tag buffer; passes it in the yielded buffer.
    if req.cmd == Command::Write {
        let tag_buf = server.buffer_mut(tag);
        if len <= tag_buf.len() {
            buf.copy_from_slice(&tag_buf[..len]);
        } else {
            buf_pool.push(buf); // returns to pool before rejecting
            server.commit_and_fetch(tag, -22)?; // EINVAL
            return Ok(false);
        }
    }

    let work = ublk::IoWork {
        qid: 0,
        tag,
        buffer_addr: 0,
        req,
        payload: buf,
    };
    work_tx
        .send(work)
        .map_err(|_| io::Error::other("worker encerrou inesperadamente"))?;
    Ok(true)
}

/// Converts CUDA error to `io::Error` for the worker thread `Result`.
fn cuda_to_io(e: ramshared_cuda::CudaError) -> io::Error {
    io::Error::other(format!("CUDA: {e}"))
}

/// Handle of the DT-3 server served by VRAM (ring owner + worker owning the CUDA stack).
pub struct ServerHandleDt3Vram {
    ring: JoinHandle<io::Result<()>>,
    worker: JoinHandle<io::Result<()>>,
}

impl ServerHandleDt3Vram {
    pub fn join(self) -> io::Result<()> {
        self.ring
            .join()
            .map_err(|_| io::Error::other("ring owner panicked"))??;
        self.worker
            .join()
            .map_err(|_| io::Error::other("vram worker panicked"))?
    }
}

/// Like [`spawn_server_dt3`], but the worker serves from **VRAM**: it creates the
/// stack `Cuda`/`Context`/`DeviceMem`/`VramBackend` **on its own thread** (the
/// CUDA context has thread affinity and `VramBackend` is not `Send`/`'static`)
/// and runs the loop there. `vram_bytes` is the GPU allocation size; `block_size` the
/// logical block size.
pub fn spawn_server_dt3_vram(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    vram_bytes: usize,
    block_size: u32,
) -> io::Result<ServerHandleDt3Vram> {
    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let server = ramshared_uring::UblkServer::new(char_dev.as_raw_fd(), queue_depth, buf_size)?;

    let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(RING_CHAN_CAP);
    let (reply_tx, reply_rx) = mpsc::channel::<WorkerReply>();

    let worker = thread::spawn(move || -> io::Result<()> {
        // The entire CUDA stack lives in this thread (context affinity).
        let cuda = Cuda::load().map_err(cuda_to_io)?;
        let device = cuda.device(0).map_err(cuda_to_io)?;
        let ctx = cuda.create_context(&device).map_err(cuda_to_io)?;
        let mut mem = ctx.alloc(vram_bytes).map_err(cuda_to_io)?;
        mem.zero().map_err(cuda_to_io)?;
        let mut backend = VramBackend::new(mem, block_size);
        worker_loop(&mut backend, work_rx, reply_tx);
        Ok(())
    });

    let ring = thread::spawn(move || {
        let _char_dev = char_dev;
        run_ring_owner(server, queue_depth, buf_size, work_tx, reply_rx)
    });

    Ok(ServerHandleDt3Vram { ring, worker })
}

/// Handle of the DT-3 VRAM server **with residency** (canary §9 + probe §9.4 inside the
/// worker). Besides `join`, exposes `demote_count` — how many DEMOTE verdicts the
/// canary emitted (observable without real swap).
pub struct ServerHandleDt3VramResidency {
    ring: JoinHandle<io::Result<()>>,
    worker: JoinHandle<io::Result<()>>,
    demotes: Arc<AtomicU32>,
}

impl ServerHandleDt3VramResidency {
    /// Number of DEMOTEs emitted by the canary so far (latency §9 + probe §9.4).
    pub fn demote_count(&self) -> u32 {
        self.demotes.load(Ordering::Relaxed)
    }

    pub fn join(self) -> io::Result<()> {
        self.ring
            .join()
            .map_err(|_| io::Error::other("ring owner panicked"))??;
        self.worker
            .join()
            .map_err(|_| io::Error::other("vram residency worker panicked"))?
    }
}

/// Loop of the worker DT-3 VRAM **with residency**, generic over the VRAM backend
/// (`M: VramMemory`): serves each request measuring serve-only latency (canary §9),
/// probes content/free in cadence (§9.4) and, upon a DEMOTE verdict, triggers `swapoff`
/// in a separate thread (Discipline 3). The free-floor is supplied via `mem_free` — decoupled
/// from the backend: CUDA passes `|| ctx.mem_info()`, a future Vulkan provider will pass its own.
/// In teardown (DT-17) waits (bounded) for the swapoff in flight and zeroes VRAM + canary.
///
/// Runs **entirely on the calling thread** (context affinity): `backend`,
/// `probe` and the `mem_free` closure borrow the thread-affine context, which lives in the
/// caller until this function returns.
#[allow(clippy::too_many_arguments)] // 8 cohesive args (worker DT-3); same as run_broker
fn serve_ublk_residency<M: VramMemory, F: Fn() -> Option<u64>>(
    mut backend: VramBackend<M>,
    mut probe: CanaryProbe<M>,
    mem_free: F,
    work_rx: Receiver<ublk::IoWork>,
    reply_tx: Sender<WorkerReply>,
    swap_dev: &str,
    residency: ResidencyConfig,
    demotes: Arc<AtomicU32>,
) -> io::Result<()> {
    // Residency state (mirrors the NBD worker of main.rs).
    let mut canary: Option<Canary> = None;
    let mut baseline: Vec<u64> = Vec::new();
    let mut sampler = ResidencySampler::new(residency);
    let mut cadence = Cadence::new(CANARY_EVERY);
    let mut demoted = false;
    let mut demote_rx: Option<Receiver<bool>> = None;

    while let Ok(mut work) = work_rx.recv() {
        let touches_vram = matches!(work.req.cmd, Command::Read | Command::Write);

        // serve-only (DT-16): times only the VRAM op, not the queue wait.
        let t0 = Instant::now();
        let result = serve_request(&work.req, &mut backend, &mut work.payload);
        let lat_us = u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);
        let is_read = work.req.cmd == Command::Read;
        let reply = WorkerReply {
            qid: work.qid,
            tag: work.tag,
            result,
            buf: work.payload,
            is_read,
        };
        if reply_tx.send(reply).is_err() {
            break; // ring owner dropped
        }

        // Non-blocking poll of the ongoing DEMOTE swapoff (re-arms if it fails).
        if let Some(rx) = demote_rx.take() {
            match rx.try_recv() {
                Ok(true) => demoted = true,
                Ok(false) => {} // failed: canary re-arms (demote_rx becomes None)
                Err(std::sync::mpsc::TryRecvError::Empty) => demote_rx = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }

        // Canary §9 (serve-only latency) — primary trigger.
        if touches_vram && !demoted && demote_rx.is_none() {
            match canary.as_mut() {
                None => {
                    if let Some(med) = calculate_baseline(&mut baseline, lat_us) {
                        canary = Some(Canary::new(residency, med));
                    }
                }
                Some(c) => {
                    // free=u64::MAX on purpose: the signal here is latency; free/
                    // content come from the probe §9.4 below.
                    if let Verdict::Demote(_) = c.sample(lat_us, true, u64::MAX) {
                        demotes.fetch_add(1, Ordering::Relaxed);
                        demote_rx = Some(spawn_swapoff(swap_dev));
                    }
                }
            }
        }

        // Dedicated probe §9.4 (content/free in cadence) with hysteresis.
        if touches_vram && !demoted && demote_rx.is_none() && cadence.tick() {
            let content = probe.check_content().ok();
            let free = mem_free();
            if let Verdict::Demote(_) = sampler.sample(content, free) {
                demotes.fetch_add(1, Ordering::Relaxed);
                demote_rx = Some(spawn_swapoff(swap_dev));
            }
        }
    }

    // Teardown DT-17: waits (bounded) for the swapoff in flight, zeroes VRAM + canary.
    if let Some(rx) = demote_rx.take() {
        let _ = rx.recv_timeout(Duration::from_secs(5));
    }
    let _ = backend.zero();
    let _ = probe.zero();
    Ok(())
}

/// Helper function to calculate the baseline median from the latency samples.
/// Once enough samples (16) are collected, it returns the median value.
fn calculate_baseline(baseline: &mut Vec<u64>, lat_us: u64) -> Option<u64> {
    baseline.push(lat_us);
    if baseline.len() >= 16 {
        baseline.sort_unstable();
        let med = baseline[baseline.len() / 2].max(1);
        Some(med)
    } else {
        None
    }
}

/// Like [`spawn_server_dt3_vram`], but the worker (owner of the CUDA context) **also runs
/// the residency machine** (Option 1 of PRD `ublk-daemon-integration`): measures
/// serve-only latency (canary §9), probes content/free in cadence (§9.4) and, upon
/// DEMOTE verdict, triggers `swapoff(swap_dev)` in a separate thread (Discipline 3).
/// Everything on the worker thread — no cross-thread CUDA calls (context affinity).
pub fn spawn_server_dt3_vram_with_residency(
    char_path: impl AsRef<Path>,
    queue_depth: u16,
    buf_size: usize,
    vram_bytes: usize,
    block_size: u32,
    swap_dev: String,
    residency: ResidencyConfig,
) -> io::Result<ServerHandleDt3VramResidency> {
    let char_dev = OpenOptions::new().read(true).write(true).open(char_path)?;
    let server = ramshared_uring::UblkServer::new(char_dev.as_raw_fd(), queue_depth, buf_size)?;

    let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(RING_CHAN_CAP);
    let (reply_tx, reply_rx) = mpsc::channel::<WorkerReply>();

    let demotes = Arc::new(AtomicU32::new(0));
    let demotes_worker = Arc::clone(&demotes);

    let worker = thread::spawn(move || -> io::Result<()> {
        // The entire CUDA stack + the canary region live in this thread (context affinity).
        let cuda = Cuda::load().map_err(cuda_to_io)?;
        let device = cuda.device(0).map_err(cuda_to_io)?;
        let ctx = cuda.create_context(&device).map_err(cuda_to_io)?;
        let mut mem = ctx.alloc(vram_bytes).map_err(cuda_to_io)?;
        mem.zero().map_err(cuda_to_io)?;
        let backend = VramBackend::new(mem, block_size);
        // Dedicated canary region (§9.4): separated from swap, not addressable by I/O.
        let canary_region = ctx.alloc(CANARY_BYTES).map_err(cuda_to_io)?;
        let probe = CanaryProbe::new(canary_region);
        // The free-floor comes from the CUDA context (`mem_info`); the residency loop is
        // generic over the VRAM backend (RF-G1) — a future Vulkan provider reuses
        // `serve_ublk_residency` with its own `mem_free`.
        serve_ublk_residency(
            backend,
            probe,
            || ctx.mem_info().ok().map(|(f, _)| f as u64),
            work_rx,
            reply_tx,
            &swap_dev,
            residency,
            demotes_worker,
        )
    });

    let ring = thread::spawn(move || {
        let _char_dev = char_dev;
        run_ring_owner(server, queue_depth, buf_size, work_tx, reply_rx)
    });

    Ok(ServerHandleDt3VramResidency {
        ring,
        worker,
        demotes,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod residency_tests {
    use super::*;
    use ramshared_vram::VramError;

    /// **Fake VRAM backend in RAM (`Vec<u8>`): exercises the generic loop
    /// [`serve_ublk_residency`] (serve + §9.4 + teardown) **without GPU/ublk/root** — safe
    /// on WSL2. The real e2e with CUDA+ublk is the `#[ignore]` `dt3_vram_residency_*` (gated).
    struct FakeVram(Vec<u8>);

    impl FakeVram {
        fn new(len: usize) -> Self {
            Self(vec![0u8; len])
        }
    }

    impl VramMemory for FakeVram {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn zero(&mut self) -> Result<(), VramError> {
            self.0.iter_mut().for_each(|b| *b = 0);
            Ok(())
        }

        fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(dst.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: dst.len() as u64,
                    size: self.0.len() as u64,
                })?;
            dst.copy_from_slice(&self.0[off..end]);
            Ok(())
        }

        fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError> {
            let off = off as usize;
            let end = off
                .checked_add(src.len())
                .filter(|&e| e <= self.0.len())
                .ok_or(VramError::OutOfRange {
                    off: off as u64,
                    len: src.len() as u64,
                    size: self.0.len() as u64,
                })?;
            self.0[off..end].copy_from_slice(src);
            Ok(())
        }
    }

    const BS: u32 = 4096;

    fn read_work(tag: u16) -> ublk::IoWork {
        ublk::IoWork {
            qid: 0,
            tag,
            buffer_addr: 0,
            req: Request {
                flags: 0,
                cmd: Command::Read,
                handle: tag as u64,
                offset: 0,
                len: BS,
            },
            payload: vec![0u8; BS as usize],
        }
    }

    /// Directs `n` Reads through the generic loop with the given `mem_free` and returns the number of
    /// DEMOTEs. `_reply_rx` remains alive until join (otherwise `reply_tx.send` fails and the loop
    /// breaks early); `work_tx` is dropped to terminate the loop. Without real swap: the
    /// non-existent `swap_dev` makes `swapoff` fail without side effects.
    fn run_loop(cfg: ResidencyConfig, mem_free: fn() -> Option<u64>, n: u16) -> u32 {
        let (work_tx, work_rx) = mpsc::sync_channel::<ublk::IoWork>(4);
        let (reply_tx, _reply_rx) = mpsc::channel::<WorkerReply>();
        let demotes = Arc::new(AtomicU32::new(0));
        let backend = VramBackend::new(FakeVram::new((BS as usize) * 8), BS);
        let probe = CanaryProbe::new(FakeVram::new(CANARY_BYTES));
        let demotes_t = Arc::clone(&demotes);

        let worker = thread::spawn(move || {
            serve_ublk_residency(
                backend,
                probe,
                mem_free,
                work_rx,
                reply_tx,
                "/dev/ramshared-no-such-swap",
                cfg,
                demotes_t,
            )
        });

        for tag in 0..n {
            work_tx.send(read_work(tag)).expect("enfileira work");
        }
        drop(work_tx); // terminates the loop
        worker.join().expect("worker join").expect("serve ok");
        demotes.load(Ordering::Relaxed)
    }

    #[test]
    fn demote_fires_when_free_below_floor() {
        // high latency_mult -> the canary §9 never fires (fake serve is ~0us); the DEMOTE
        // comes from the probe §9.4 (free=0 < free_floor). consecutive=1 -> 1 degraded sample.
        let cfg = ResidencyConfig {
            latency_mult: 4096,
            consecutive: 1,
            free_floor_bytes: 1 << 30, // 1 GiB; free=0 fica abaixo
        };
        // >=64 reads for the cadence §9.4 (CANARY_EVERY) to fire at least once.
        let demotes = run_loop(cfg, || Some(0), 80);
        assert!(
            demotes >= 1,
            "esperava >=1 DEMOTE por free<floor, obteve {demotes}"
        );
    }

    #[test]
    fn no_demote_when_healthy() {
        // abundant free + fast serve -> no trigger (§9 nor §9.4).
        let cfg = ResidencyConfig {
            latency_mult: 4096,
            consecutive: 1,
            free_floor_bytes: 0,
        };
        let demotes = run_loop(cfg, || Some(u64::MAX), 80);
        assert_eq!(demotes, 0, "should not have DEMOTE with healthy VRAM");
    }
}
