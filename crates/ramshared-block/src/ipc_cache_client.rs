//! IPC cache client communicating with the isolated GPU cache worker.

use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::gpu_cache_worker::{
    FRAME_HEADER_LEN, FrameHeader, MAX_IPC_PAYLOAD_BYTES, MSG_DISABLE_REQ, MSG_DISABLE_RESP,
    MSG_HANDSHAKE_REQ, MSG_HANDSHAKE_RESP, MSG_HEARTBEAT_REQ, MSG_HEARTBEAT_RESP,
    MSG_INVALIDATE_REQ, MSG_PROMOTE, MSG_READ_REQ, MSG_READ_RESP, MSG_UPDATE, STATUS_MISS,
    STATUS_OK,
};
use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead, MAX_CACHE_MUTATION_BYTES};
use crate::origin_cache::CacheState;
use ramshared_vram::{
    GpuBudgetTelemetry, MAX_WORKER_TELEMETRY_PAYLOAD_BYTES, WorkerCacheTelemetry,
    WorkerTelemetryEnvelope,
};

pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(50);
/// Handshake covers worker **process startup**, not the IPC round-trip: the
/// child must dlopen CUDA/NVML, create a context, and complete WDDM budget
/// revalidation before it can answer. Measured 2.86 s at load 12 and past the
/// old 5 s bound by load ~42, so the startup budget is separate from the 5 s
/// teardown bound (SPEC: DT-2). Reads/heartbeats keep NFR-1's 50 ms.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Disable/teardown uses a longer timeout to accommodate GPU context cleanup.
/// SPEC: DT-5 (5s bounded supervisor teardown).
pub const DISABLE_TIMEOUT: Duration = Duration::from_secs(5);
/// Minimum spacing between heartbeat round-trips. The serve loop ticks once
/// per NBD request; without this, telemetry would issue one IPC round-trip per
/// request and compete with data frames on the shared ordered stream (DT-1).
pub const MIN_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
/// Quiet time after a write-mirror frame before a cache read is issued again.
///
/// SPEC DT-1 puts mutations and reads on one ordered socket with no control
/// lane. Write mirrors are split into 64 KiB nonblocking frames that the
/// worker must drain sequentially; a read queued behind a burst waits for the
/// whole backlog and can miss NFR-1's 50 ms bound, which fail-closes the
/// cache. Promotes from the read path itself are exempt so a cold read stream
/// can populate the cache without suppressing its own future hits.
pub const READ_DRAIN_GRACE: Duration = Duration::from_millis(50);
/// Quiet time after any mutation frame before a heartbeat is issued.
///
/// Heartbeats are telemetry and may wait out a longer drain than a data read:
/// a heartbeat that collides with a mutation burst is the exact failure that
/// permanently revoked the cache under write load (worker EPIPE after the
/// parent's fail-closed shutdown).
pub const HEARTBEAT_DRAIN_GRACE: Duration = Duration::from_millis(200);

/// Parent-side scheduling for round-trip requests on the shared ordered
/// stream (DT-1). Production uses [`PacingPolicy::default`] so telemetry and
/// cache reads never queue behind a mutation burst. Protocol cycle tests use
/// [`PacingPolicy::immediate`] to drive the wire without waiting out the
/// drain graces.
#[derive(Clone, Copy, Debug)]
pub struct PacingPolicy {
    pub read_drain_grace: Duration,
    pub heartbeat_drain_grace: Duration,
    pub min_heartbeat_interval: Duration,
}

impl Default for PacingPolicy {
    fn default() -> Self {
        Self {
            read_drain_grace: READ_DRAIN_GRACE,
            heartbeat_drain_grace: HEARTBEAT_DRAIN_GRACE,
            min_heartbeat_interval: MIN_HEARTBEAT_INTERVAL,
        }
    }
}

impl PacingPolicy {
    /// Issue every round-trip immediately. Used by protocol cycle tests that
    /// drive a worker already drained of mutations.
    pub const fn immediate() -> Self {
        Self {
            read_drain_grace: Duration::ZERO,
            heartbeat_drain_grace: Duration::ZERO,
            min_heartbeat_interval: Duration::ZERO,
        }
    }
}
/// Heartbeat payload ceiling (DT-8). An envelope larger than this is rejected
/// whole — never truncated into a plausible-looking sample.
const MAX_GPU_BUDGET_PAYLOAD_BYTES: u32 = MAX_WORKER_TELEMETRY_PAYLOAD_BYTES as u32;

/// Largest payload a single cache mutation frame may carry. One frame is one
/// nonblocking send: the origin path must never block on the GPU worker, so a
/// logical mutation larger than this is split by the caller into one frame per
/// slice (see `AuthoritativeOriginBackend::update_cache`).
pub const MAX_MUTATION_FRAME_DATA_BYTES: usize = MAX_CACHE_MUTATION_BYTES;

/// Ceiling on parent-side unwritten mutation bytes (DT-10). The socket can be
/// backpressured while the worker drains `handle_update`; the queue absorbs the
/// burst without stalling origin I/O and without abandoning a mid-frame write.
/// An `Update` that cannot enter the queue revokes the cache; a `Promote` is
/// simply not cached.
pub const MAX_PENDING_MUTATION_BYTES: usize = 4 * 1024 * 1024;

fn deadline_after(timeout: Duration) -> io::Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "IPC deadline overflow"))
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration.is_zero() {
        Err(io::Error::new(ErrorKind::TimedOut, "IPC deadline expired"))
    } else {
        Ok(duration)
    }
}

fn read_exact_until(
    socket: &mut UnixStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !buffer.is_empty() {
        socket.set_read_timeout(Some(remaining(deadline)?))?;
        match socket.read(buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "IPC stream closed",
                ));
            }
            Ok(read) => buffer = &mut buffer[read..],
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_all_until(
    socket: &mut UnixStream,
    mut buffer: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    while !buffer.is_empty() {
        socket.set_write_timeout(Some(remaining(deadline)?))?;
        match socket.write(buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "IPC stream made no progress",
                ));
            }
            Ok(written) => buffer = &buffer[written..],
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Outcome of offering a complete mutation frame to the pending queue.
enum MutationQueue {
    /// The frame is committed to delivery: fully on the wire, or whole in the
    /// pending queue behind earlier frames.
    Queued,
    /// The queue is full and the frame was not stored. The caller must not
    /// pretend the mutation will reach the worker.
    Dropped,
    /// The peer is gone. Only a deterministic write error produces this;
    /// backpressure is [`MutationQueue::Queued`] or [`MutationQueue::Dropped`].
    PeerGone(io::Error),
}

/// Drains as much of `pending` as the socket accepts without blocking.
/// Returns `Ok(())` only when `pending` is empty.
fn drain_pending(socket: &mut UnixStream, pending: &mut Vec<u8>) -> io::Result<()> {
    while !pending.is_empty() {
        match socket.write(pending) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "IPC stream made no progress",
                ));
            }
            Ok(written) => {
                pending.drain(..written);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == ErrorKind::TimedOut => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Offers `frame` to the stream, storing any unwritten tail in `pending`.
///
/// The socket is nonblocking for the duration so origin I/O can never stall
/// behind a full socket buffer (DT-2/DT-10). A short write is not an error: the
/// remainder waits in `pending` and is completed by a later drain, which keeps
/// the byte stream framed. Only a deterministic peer error is fatal.
fn queue_frame(socket: &mut UnixStream, pending: &mut Vec<u8>, frame: &[u8]) -> MutationQueue {
    if let Err(error) = socket.set_nonblocking(true) {
        return MutationQueue::PeerGone(error);
    }
    let result = queue_frame_nonblocking(socket, pending, frame);
    if let Err(error) = socket.set_nonblocking(false)
        && matches!(result, MutationQueue::Queued)
    {
        return MutationQueue::PeerGone(error);
    }
    result
}

fn queue_frame_nonblocking(
    socket: &mut UnixStream,
    pending: &mut Vec<u8>,
    frame: &[u8],
) -> MutationQueue {
    match drain_pending(socket, pending) {
        Ok(()) => {}
        Err(error) => return MutationQueue::PeerGone(error),
    }
    if !pending.is_empty() {
        // An earlier frame is still draining. The stream is mid-frame, so a new
        // frame can only join the queue; it must not be interleaved on the wire.
        return if pending.len().saturating_add(frame.len()) > MAX_PENDING_MUTATION_BYTES {
            MutationQueue::Dropped
        } else {
            pending.extend_from_slice(frame);
            MutationQueue::Queued
        };
    }
    let mut rest = frame;
    while !rest.is_empty() {
        match socket.write(rest) {
            Ok(0) => {
                return MutationQueue::PeerGone(io::Error::new(
                    ErrorKind::WriteZero,
                    "IPC stream made no progress",
                ));
            }
            Ok(written) => rest = &rest[written..],
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
            {
                break;
            }
            Err(error) => return MutationQueue::PeerGone(error),
        }
    }
    if !rest.is_empty() {
        if rest.len() > MAX_PENDING_MUTATION_BYTES {
            return MutationQueue::Dropped;
        }
        pending.extend_from_slice(rest);
    }
    MutationQueue::Queued
}

/// Completes any unwritten mutation bytes without blocking.
///
/// `Ok(true)` means the stream is framed again (nothing pending) and a request
/// may be written. `Ok(false)` means the queue still holds a mid-frame tail: a
/// cache read must degrade to a miss rather than corrupt the stream (DT-10).
fn try_drain_pending(socket: &mut UnixStream, pending: &mut Vec<u8>) -> io::Result<bool> {
    if pending.is_empty() {
        return Ok(true);
    }
    socket.set_nonblocking(true)?;
    let drain_result = drain_pending(socket, pending);
    let restore_result = socket.set_nonblocking(false);
    drain_result.and(restore_result)?;
    Ok(pending.is_empty())
}

pub struct IpcCacheClient {
    socket: UnixStream,
    read_timeout: Duration,
    timeouts_configured: bool,
    state: CacheState,
    cached_bytes: u64,
    target_bytes: u64,
    gpu_budget: Option<GpuBudgetTelemetry>,
    cache_telemetry: Option<WorkerCacheTelemetry>,
    seq: u64,
    /// Last write-mirror (`MSG_UPDATE`) queue stamp, used to defer cache reads
    /// while the worker drains a write burst.
    last_update_at: Option<Instant>,
    /// Last mutation stamp of any kind (update or promote), used to defer
    /// heartbeats while the worker drains.
    last_mutation_at: Option<Instant>,
    /// Last successful heartbeat, used to space telemetry round-trips.
    last_heartbeat_at: Option<Instant>,
    /// Unwritten mutation-frame bytes (DT-10). Non-empty means the byte stream
    /// is mid-frame: a request would corrupt it, so reads degrade to a miss
    /// until the queue drains.
    pending_frame: Vec<u8>,
    pacing: PacingPolicy,
}

impl IpcCacheClient {
    pub fn new(socket: UnixStream, read_timeout: Duration, target_bytes: u64) -> Self {
        Self::with_pacing(socket, read_timeout, target_bytes, PacingPolicy::default())
    }

    pub fn with_pacing(
        socket: UnixStream,
        read_timeout: Duration,
        target_bytes: u64,
        pacing: PacingPolicy,
    ) -> Self {
        let timeouts_configured = !read_timeout.is_zero()
            && socket.set_read_timeout(Some(read_timeout)).is_ok()
            && socket.set_write_timeout(Some(read_timeout)).is_ok();
        if !timeouts_configured {
            eprintln!("[ramsharedd] isolated GPU cache unavailable: IPC timeout setup failed");
        }
        Self {
            socket,
            read_timeout,
            timeouts_configured,
            state: if timeouts_configured {
                CacheState::Active
            } else {
                CacheState::Unavailable
            },
            cached_bytes: 0,
            target_bytes,
            gpu_budget: None,
            cache_telemetry: None,
            seq: 0,
            last_update_at: None,
            last_mutation_at: None,
            last_heartbeat_at: None,
            pending_frame: Vec::new(),
            pacing,
        }
    }

    pub fn perform_handshake(&mut self) -> Result<(), String> {
        if !self.timeouts_configured || self.state != CacheState::Active {
            return Err("IPC timeout configuration is unavailable".to_string());
        }
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_HANDSHAKE_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset: 0,
            payload_len: 0,
            aux: 0,
        };
        let deadline = match deadline_after(HANDSHAKE_TIMEOUT) {
            Ok(deadline) => deadline,
            Err(error) => {
                self.fail("handshake deadline setup failed");
                return Err(format!("handshake deadline setup failed: {error}"));
            }
        };
        if let Err(error) = write_all_until(&mut self.socket, &req.encode(), deadline) {
            self.fail("handshake write failed");
            return Err(format!("handshake write error: {error}"));
        }

        let mut buf = [0u8; FRAME_HEADER_LEN];
        if let Err(error) = read_exact_until(&mut self.socket, &mut buf, deadline) {
            self.fail("handshake read failed");
            return Err(format!("handshake read error: {error}"));
        }
        if let Err(error) = self
            .socket
            .set_read_timeout(Some(self.read_timeout))
            .and_then(|()| self.socket.set_write_timeout(Some(self.read_timeout)))
        {
            self.fail("steady-state read timeout restore failed");
            return Err(format!("handshake timeout restore failed: {error}"));
        }

        let Some(resp) = FrameHeader::decode(&buf) else {
            self.fail("handshake response header malformed");
            return Err("invalid handshake response header".to_string());
        };
        if resp.msg_type != MSG_HANDSHAKE_RESP || resp.correlation_id != self.seq {
            self.fail("handshake response mismatched");
            return Err("invalid handshake response".to_string());
        }
        if resp.offset > 0 {
            self.target_bytes = resp.offset;
        }
        self.cached_bytes = (resp.aux as u64) << 10;
        // target_bytes == 0 means the worker has no VRAM provider (GAP-6):
        // report Unavailable so telemetry and cascade gates see the truth.
        if self.target_bytes == 0 {
            self.state = CacheState::Unavailable;
        } else {
            self.state = CacheState::Active;
        }
        Ok(())
    }

    fn fail(&mut self, reason: &'static str) {
        eprintln!("[ramsharedd] isolated GPU cache unavailable: {reason}");
        self.state = CacheState::Unavailable;
        self.cached_bytes = 0;
        self.gpu_budget = None;
        self.cache_telemetry = None;
        self.pending_frame.clear();
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }

    /// True while the worker is likely still draining a mutation burst, so a
    /// round-trip request would queue behind it and miss NFR-1's bound.
    fn write_mirrors_are_draining(&self) -> bool {
        self.last_update_at
            .is_some_and(|at| at.elapsed() < self.pacing.read_drain_grace)
    }

    fn mutations_are_draining(&self) -> bool {
        self.last_mutation_at
            .is_some_and(|at| at.elapsed() < self.pacing.heartbeat_drain_grace)
    }

    fn heartbeat_is_due(&self) -> bool {
        self.last_heartbeat_at
            .is_none_or(|at| at.elapsed() >= self.pacing.min_heartbeat_interval)
    }

    pub fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        if self.state != CacheState::Active {
            return Err("GPU cache worker is unavailable");
        }
        // A heartbeat request written mid-frame would corrupt the stream
        // (DT-10), and telemetry is optional: skip the round-trip and report
        // the last confirmed sample while the queue drains.
        match try_drain_pending(&mut self.socket, &mut self.pending_frame) {
            Ok(true) => {}
            Ok(false) => return Ok(self.cached_bytes),
            Err(_) => {
                self.fail("mutation frame drain failed");
                return Err("GPU cache worker mutation backlog could not drain");
            }
        }
        // Telemetry is optional and must never sit in a mutation backlog
        // (DT-1). Skip the round-trip and report the last confirmed sample:
        // occupancy only moves when the worker answers, and a skipped beat is
        // cheaper than the fail-closed revocation a 50 ms timeout used to
        // trigger under write load.
        if !self.heartbeat_is_due() || self.mutations_are_draining() {
            return Ok(self.cached_bytes);
        }
        let deadline = match deadline_after(self.read_timeout) {
            Ok(deadline) => deadline,
            Err(_) => {
                self.fail("heartbeat deadline setup failed");
                return Err("GPU cache worker heartbeat deadline could not be configured");
            }
        };
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_HEARTBEAT_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset: 0,
            payload_len: 0,
            aux: 0,
        };
        let mut buf = [0u8; FRAME_HEADER_LEN];
        if write_all_until(&mut self.socket, &req.encode(), deadline).is_err()
            || read_exact_until(&mut self.socket, &mut buf, deadline).is_err()
        {
            self.fail("heartbeat I/O failed");
            return Err("GPU cache worker heartbeat timed out");
        }
        let Some(resp) = FrameHeader::decode(&buf) else {
            self.fail("heartbeat response header malformed");
            return Err("GPU cache worker heartbeat header malformed");
        };
        if resp.msg_type != MSG_HEARTBEAT_RESP
            || resp.correlation_id != self.seq
            || resp.status != STATUS_OK
            || resp.offset != self.target_bytes
        {
            self.fail("heartbeat response mismatched");
            return Err("GPU cache worker heartbeat mismatched");
        }
        // Physical occupancy stays in the frame header fields and is never
        // re-derived from the envelope (DT-8).
        self.cached_bytes = (resp.aux as u64) << 10;
        if resp.payload_len > MAX_GPU_BUDGET_PAYLOAD_BYTES {
            self.gpu_budget = None;
            self.cache_telemetry = None;
            self.fail("heartbeat worker telemetry exceeded its size limit");
            return Err("GPU cache worker heartbeat telemetry exceeded its limit");
        }
        if resp.payload_len == 0 {
            self.gpu_budget = None;
            self.cache_telemetry = None;
        } else {
            let mut payload = vec![0; resp.payload_len as usize];
            if read_exact_until(&mut self.socket, &mut payload, deadline).is_err() {
                self.fail("heartbeat worker telemetry was truncated");
                return Err("GPU cache worker heartbeat telemetry was truncated");
            }
            // Unknown envelope versions and malformed samples are **omitted**,
            // not accepted and not turned into a client failure: the physical
            // header fields above are already authoritative.
            match WorkerTelemetryEnvelope::from_bounded_payload(&payload) {
                Some(envelope) => {
                    self.gpu_budget = envelope
                        .budget
                        .filter(|telemetry| telemetry.schema_version == 1);
                    self.cache_telemetry = envelope.cache;
                }
                None => {
                    self.gpu_budget = None;
                    self.cache_telemetry = None;
                }
            }
        }
        self.last_heartbeat_at = Some(Instant::now());
        Ok(self.cached_bytes)
    }

    /// Adapter budget from the last heartbeat (its own schema, unchanged).
    pub fn gpu_budget_telemetry(&self) -> Option<&GpuBudgetTelemetry> {
        self.gpu_budget.as_ref()
    }

    /// Cache occupancy and codec health from the last heartbeat.
    ///
    /// Logical cache bytes are cache occupancy in the worker's address space —
    /// never guest or host RAM (DT-8).
    pub fn cache_telemetry(&self) -> Option<&WorkerCacheTelemetry> {
        self.cache_telemetry.as_ref()
    }

    fn send_mutation_frame(&mut self, header: FrameHeader, payload: &[u8]) -> CacheMutation {
        if self.state != CacheState::Active {
            return CacheMutation::Skipped;
        }
        if payload.len() > MAX_MUTATION_FRAME_DATA_BYTES {
            self.fail("cache mutation exceeds nonblocking frame limit");
            return CacheMutation::Failed;
        }
        // Assemble the complete frame before offering it to the queue.
        let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
        frame.extend_from_slice(&header.encode());
        frame.extend_from_slice(payload);

        match queue_frame(&mut self.socket, &mut self.pending_frame, &frame) {
            MutationQueue::Queued => {}
            MutationQueue::Dropped => {
                // DT-10/DT-11: a dropped `Promote` only means "not cached",
                // so a later read is a miss — the correct answer. A dropped
                // `Update` would leave pre-write bytes in the worker that a
                // later Hit could serve, so it is converted into
                // `MSG_INVALIDATE` for exactly that range instead of
                // revoking the whole cache.
                if header.msg_type == MSG_UPDATE {
                    return self.convert_dropped_update_to_invalidate(header.offset, payload.len());
                }
                return CacheMutation::Skipped;
            }
            MutationQueue::PeerGone(error) => {
                // Surface the errno: a deterministic peer error is the only
                // remaining reason a mutation send fails closed (DT-10), and
                // operators need to tell EPIPE from a bad descriptor.
                eprintln!("[ramsharedd] mutation frame write error: {error}");
                self.fail("nonblocking mutation frame write failed");
                return CacheMutation::Failed;
            }
        }

        // Accepted bytes are queued, not evidence of GPU allocation: keep the
        // last confirmed occupancy sample and let the next heartbeat replace
        // it. Zeroing here would make every mutation report an empty cache.
        let now = Instant::now();
        self.last_mutation_at = Some(now);
        if header.msg_type == MSG_UPDATE {
            self.last_update_at = Some(now);
        }
        self.gpu_budget = None;
        self.cache_telemetry = None;
        CacheMutation::Accepted
    }

    /// DT-11: replace a write-mirror that could not be queued with an
    /// `MSG_INVALIDATE` for exactly its range.
    ///
    /// The invalidate joins the same pending queue, so it is ordered after
    /// the frames already in flight and the worker drops only pre-write
    /// coverage. If even the invalidate cannot be queued, or the peer is
    /// gone, nothing can stop a stale Hit and the cache revokes.
    fn convert_dropped_update_to_invalidate(
        &mut self,
        offset: u64,
        range_len: usize,
    ) -> CacheMutation {
        self.seq = self.seq.saturating_add(1);
        let invalidate = FrameHeader {
            msg_type: MSG_INVALIDATE_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset,
            payload_len: 0,
            aux: range_len as u32,
        };
        match queue_frame(
            &mut self.socket,
            &mut self.pending_frame,
            &invalidate.encode(),
        ) {
            MutationQueue::Queued => {
                self.last_mutation_at = Some(Instant::now());
                self.gpu_budget = None;
                self.cache_telemetry = None;
                // The write-mirror did not land: the honest outcome is "not
                // cached", with the stale range already invalidated.
                CacheMutation::Skipped
            }
            MutationQueue::Dropped => {
                self.fail("mutation backlog overflowed and invalidate could not be queued");
                CacheMutation::Failed
            }
            MutationQueue::PeerGone(error) => {
                eprintln!("[ramsharedd] invalidate frame write error: {error}");
                self.fail("nonblocking invalidate frame write failed");
                CacheMutation::Failed
            }
        }
    }
}

impl BestEffortCache for IpcCacheClient {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead {
        if self.state != CacheState::Active {
            return CacheRead::Miss;
        }
        if destination.len() > MAX_IPC_PAYLOAD_BYTES {
            return CacheRead::Miss;
        }
        // A read issued while a mutation frame is still draining would be
        // concatenated into that incomplete frame and corrupt both (DT-10).
        // Origin is authoritative, so a miss is the correct answer and the
        // cache stays alive for the next quiet window.
        match try_drain_pending(&mut self.socket, &mut self.pending_frame) {
            Ok(true) => {}
            Ok(false) => return CacheRead::Miss,
            Err(_) => {
                self.fail("mutation frame drain failed");
                return CacheRead::Failed;
            }
        }
        // A read queued behind a write-mirror burst waits for the whole
        // backlog and would miss NFR-1's 50 ms bound, which fail-closes the
        // cache. Origin is authoritative, so a deferred read is a miss — the
        // correct answer — and the cache stays alive for the next quiet
        // window. Promotes from the read path itself never set this gate.
        if self.write_mirrors_are_draining() {
            return CacheRead::Miss;
        }
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_READ_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset,
            payload_len: 0,
            aux: destination.len() as u32,
        };

        let deadline = match deadline_after(self.read_timeout) {
            Ok(deadline) => deadline,
            Err(_) => {
                self.fail("read deadline setup failed");
                return CacheRead::Failed;
            }
        };
        if write_all_until(&mut self.socket, &req.encode(), deadline).is_err() {
            self.fail("read request write failed");
            return CacheRead::Failed;
        }

        let mut hdr_buf = [0u8; FRAME_HEADER_LEN];
        if read_exact_until(&mut self.socket, &mut hdr_buf, deadline).is_err() {
            self.fail("read response timed out");
            return CacheRead::Failed;
        }

        let Some(resp) = FrameHeader::decode(&hdr_buf) else {
            self.fail("read response header malformed");
            return CacheRead::Failed;
        };
        if resp.msg_type != MSG_READ_RESP || resp.correlation_id != self.seq {
            self.fail("read response identity mismatched");
            return CacheRead::Failed;
        }
        // Sync authoritative cached_bytes from the worker (aux carries KiB).
        self.cached_bytes = (resp.aux as u64) << 10;

        match resp.status {
            STATUS_OK => {
                if resp.payload_len as usize != destination.len() {
                    self.fail("read response payload length mismatched");
                    return CacheRead::Failed;
                }
                if read_exact_until(&mut self.socket, destination, deadline).is_err() {
                    self.fail("read response payload timed out");
                    return CacheRead::Failed;
                }
                CacheRead::Hit
            }
            STATUS_MISS => CacheRead::Miss,
            _ => {
                self.fail("read response status failed");
                CacheRead::Failed
            }
        }
    }

    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_UPDATE,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset,
            payload_len: data.len() as u32,
            aux: 0,
        };
        self.send_mutation_frame(req, data)
    }

    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_PROMOTE,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset,
            payload_len: data.len() as u32,
            aux: 0,
        };
        self.send_mutation_frame(req, data)
    }

    fn disable(&mut self) -> CacheMutation {
        if matches!(self.state, CacheState::Off | CacheState::Unavailable) {
            return CacheMutation::Skipped;
        }
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_DISABLE_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset: 0,
            payload_len: 0,
            aux: 0,
        };

        let deadline = match deadline_after(DISABLE_TIMEOUT) {
            Ok(deadline) => deadline,
            Err(_) => {
                self.state = CacheState::Stuck;
                return CacheMutation::Failed;
            }
        };
        // Teardown must observe mutation order: append the disable frame behind
        // anything still queued and flush the whole backlog (DT-10).
        self.pending_frame.extend_from_slice(&req.encode());
        if write_all_until(&mut self.socket, &self.pending_frame, deadline).is_err() {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }
        self.pending_frame.clear();

        let mut hdr_buf = [0u8; FRAME_HEADER_LEN];
        if read_exact_until(&mut self.socket, &mut hdr_buf, deadline).is_err() {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }

        let Some(resp) = FrameHeader::decode(&hdr_buf) else {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        };
        if resp.msg_type == MSG_DISABLE_RESP && resp.status == STATUS_OK {
            self.state = CacheState::Off;
            self.cached_bytes = 0;
            CacheMutation::Accepted
        } else {
            self.state = CacheState::Stuck;
            CacheMutation::Failed
        }
    }

    fn state(&self) -> CacheState {
        self.state
    }

    fn cached_bytes(&self) -> u64 {
        if self.state == CacheState::Active {
            self.cached_bytes
        } else {
            0
        }
    }

    fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        IpcCacheClient::refresh_cached_bytes(self)
    }

    fn target_bytes(&self) -> u64 {
        self.target_bytes
    }

    fn gpu_budget_telemetry(&self) -> Option<&GpuBudgetTelemetry> {
        IpcCacheClient::gpu_budget_telemetry(self)
    }

    fn cache_telemetry(&self) -> Option<&WorkerCacheTelemetry> {
        IpcCacheClient::cache_telemetry(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::time::Instant;

    /// Pushes real `MSG_UPDATE` frames until the socket cannot accept a whole
    /// one, so the client lands in the DT-10 backpressured state. Saturation is
    /// the frames themselves — never filler — so every byte the peer can read
    /// is frame data and the wire is never contaminated.
    fn saturate_with_updates(client: &mut IpcCacheClient) -> usize {
        let payload = vec![0x5A; MAX_MUTATION_FRAME_DATA_BYTES];
        let mut sent = 0;
        while client.pending_frame.is_empty() {
            assert_eq!(
                client.update(sent as u64, &payload),
                CacheMutation::Accepted
            );
            sent += 1;
            assert!(sent < 512, "socket never backpressured after {sent} frames");
        }
        sent
    }

    /// Queues further updates while the backlog has room for one more frame.
    /// Leaves the client exactly one frame below [`MAX_PENDING_MUTATION_BYTES`].
    fn queue_updates_until_one_frame_from_full(client: &mut IpcCacheClient) -> usize {
        let payload = vec![0x5A; MAX_MUTATION_FRAME_DATA_BYTES];
        let frame_len = FRAME_HEADER_LEN + MAX_MUTATION_FRAME_DATA_BYTES;
        let mut queued = 0;
        while client.pending_frame.len() + frame_len <= MAX_PENDING_MUTATION_BYTES {
            assert_eq!(
                client.update(queued as u64, &payload),
                CacheMutation::Accepted
            );
            queued += 1;
            assert!(queued < 512, "backlog never approached the cap");
        }
        queued
    }

    /// Tops the pending backlog up to exactly `target` bytes using real
    /// `MSG_UPDATE` frames — never filler — so a test can choose how much
    /// headroom is left for a DT-11 invalidate.
    fn fill_pending_to(client: &mut IpcCacheClient, target: usize) {
        assert!(target <= MAX_PENDING_MUTATION_BYTES);
        while client.pending_frame.len() < target {
            let room = target - client.pending_frame.len();
            assert!(
                room >= FRAME_HEADER_LEN,
                "cannot land on target with {room} bytes left"
            );
            let frame_len = room.min(FRAME_HEADER_LEN + MAX_MUTATION_FRAME_DATA_BYTES);
            let payload = vec![0x5A; frame_len - FRAME_HEADER_LEN];
            assert_eq!(
                client.update(0, &payload),
                CacheMutation::Accepted,
                "a fill step must queue while under the cap"
            );
        }
        assert_eq!(client.pending_frame.len(), target);
    }

    #[test]
    fn read_timeout_falls_back_cleanly() {
        let (client_sock, _hung_worker) = UnixStream::pair().expect("socketpair failed");
        // Use a short read timeout for the test to avoid slowing down CI
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(20), 1024 * 1024);
        assert_eq!(client.state(), CacheState::Active);

        let mut buf = [0u8; 128];
        let outcome = client.read(0, &mut buf);

        // Hung worker causes timeout -> marks unavailable and returns Failed
        assert_eq!(outcome, CacheRead::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);

        // Subsequent reads immediately return Miss without waiting
        let start = Instant::now();
        let second_outcome = client.read(0, &mut buf);
        assert_eq!(second_outcome, CacheRead::Miss);
        assert!(start.elapsed() < Duration::from_millis(5));
    }

    #[test]
    fn trickled_response_cannot_extend_the_absolute_read_deadline() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let timeout = Duration::from_millis(30);
        let worker = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            worker_sock.read_exact(&mut request).unwrap();
            let mut written = 0;
            for _ in 0..FRAME_HEADER_LEN {
                if worker_sock.write_all(&[0]).is_err() {
                    break;
                }
                written += 1;
                std::thread::sleep(Duration::from_millis(8));
            }
            written
        });
        let mut client = IpcCacheClient::new(client_sock, timeout, 1024 * 1024);

        let start = Instant::now();
        let outcome = client.read(0, &mut [0u8; 16]);
        let elapsed = start.elapsed();

        assert_eq!(outcome, CacheRead::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);
        assert!(
            elapsed < Duration::from_millis(180),
            "30ms cache read exceeded absolute deadline by too much: {elapsed:?}"
        );
        assert!(worker.join().unwrap() < FRAME_HEADER_LEN);
    }

    #[test]
    fn socket_disconnect_marks_unavailable() {
        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        assert_eq!(client.state(), CacheState::Active);

        // Abrupt worker crash closes socket
        drop(worker_sock);

        let mut buf = [0u8; 128];
        let outcome = client.read(0, &mut buf);
        assert_eq!(outcome, CacheRead::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);
        assert_eq!(client.cached_bytes(), 0);

        // Mutations on disconnected client return Skipped
        let mut_outcome = client.update(0, &[1, 2, 3]);
        assert_eq!(mut_outcome, CacheMutation::Skipped);
    }

    #[test]
    fn small_update_and_promote_complete_within_the_deadline() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        let start = Instant::now();
        let payload = vec![0xAB; 4096];
        let update_outcome = client.update(0, &payload);
        let promote_outcome = client.promote(4096, &payload);

        assert!(start.elapsed() < Duration::from_millis(50));
        assert_eq!(update_outcome, CacheMutation::Accepted);
        assert_eq!(promote_outcome, CacheMutation::Accepted);
        assert_eq!(
            client.cached_bytes(),
            0,
            "queued bytes are not physical allocations"
        );
    }

    /// A saturated socket must not consume the origin thread and must not
    /// revoke the cache: backpressure queues frames (DT-10), it is not a
    /// broken peer.
    #[test]
    fn saturated_mutation_socket_does_not_block_origin_thread() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(250), 1024 * 1024);
        saturate_with_updates(&mut client);
        let payload = vec![0xCD; MAX_MUTATION_FRAME_DATA_BYTES];

        let start = Instant::now();
        for round in 0..8 {
            assert_eq!(
                client.update(round as u64, &payload),
                CacheMutation::Accepted
            );
        }
        let elapsed = start.elapsed();

        assert_eq!(
            client.state(),
            CacheState::Active,
            "backpressure must not revoke the cache"
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "a blocked mutation consumed the origin thread for {elapsed:?}"
        );
    }

    /// A short write is not a fatal error. The unwritten tail waits in the
    /// pending queue and is completed by a later drain, so the byte stream
    /// stays framed and the cache stays alive (DT-10).
    #[test]
    fn partial_mutation_write_completes_the_frame_and_keeps_the_cache_active() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        let whole_frames = saturate_with_updates(&mut client);
        assert!(
            !client.pending_frame.is_empty(),
            "saturation must leave an unwritten tail, not an abandoned frame"
        );
        assert!(
            client.pending_frame.len() < FRAME_HEADER_LEN + MAX_MUTATION_FRAME_DATA_BYTES,
            "the tail must be a fragment of one frame"
        );
        assert_eq!(
            client.state(),
            CacheState::Active,
            "a partial write must not revoke the cache"
        );

        // Interleave peer reads with client drains until the tail completes.
        worker_sock.set_nonblocking(true).unwrap();
        let frame_len = FRAME_HEADER_LEN + MAX_MUTATION_FRAME_DATA_BYTES;
        // Every accepted update is one whole frame: the last is split across
        // the wire and the pending tail, so the drained stream is exactly
        // `whole_frames` frames long.
        let expected = whole_frames * frame_len;
        let mut collected = Vec::with_capacity(expected);
        let mut buffer = [0u8; 8192];
        while collected.len() < expected {
            match worker_sock.read(&mut buffer) {
                Ok(0) => panic!("peer closed before the mutation frame completed"),
                Ok(read) => collected.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    try_drain_pending(&mut client.socket, &mut client.pending_frame)
                        .expect("drain must not hit a hard error");
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => panic!("peer read failed: {error}"),
            }
        }

        assert_eq!(
            client.pending_frame.len(),
            0,
            "the pending tail must drain to nothing"
        );
        assert_eq!(client.state(), CacheState::Active);
        // Saturation used only `MSG_UPDATE` frames of one fixed payload length,
        // so the stream must be a clean run of whole frames with no holes.
        assert_eq!(collected.len() % frame_len, 0);
        for (index, frame) in collected.chunks_exact(frame_len).enumerate() {
            let mut header_bytes = [0u8; FRAME_HEADER_LEN];
            header_bytes.copy_from_slice(&frame[..FRAME_HEADER_LEN]);
            let header = FrameHeader::decode(&header_bytes).expect("valid header");
            assert_eq!(header.msg_type, MSG_UPDATE);
            assert_eq!(header.correlation_id as usize, index + 1);
            assert_eq!(header.payload_len as usize, MAX_MUTATION_FRAME_DATA_BYTES);
            assert_eq!(
                &frame[FRAME_HEADER_LEN..],
                &[0x5A; MAX_MUTATION_FRAME_DATA_BYTES]
            );
        }
    }

    /// A write-mirror that cannot reach the worker must never be silently
    /// dropped: the worker would keep pre-write bytes and a later Hit would
    /// serve them. While the queue has room the update is queued (DT-10).
    #[test]
    fn backpressured_update_is_queued_never_dropped() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        let payload = vec![0x11; MAX_MUTATION_FRAME_DATA_BYTES];
        let frame_len = FRAME_HEADER_LEN + MAX_MUTATION_FRAME_DATA_BYTES;
        let before = client.pending_frame.len();

        for round in 0..3 {
            assert_eq!(
                client.update(round as u64, &payload),
                CacheMutation::Accepted,
                "update {round} must be queued while the backlog has room"
            );
        }
        assert_eq!(client.state(), CacheState::Active);
        assert_eq!(
            client.pending_frame.len(),
            before + 3 * frame_len,
            "every accepted update must sit whole in the pending queue"
        );
    }

    /// A dropped `Promote` is not stale data: the range is simply not cached
    /// and a later read is a miss. It must not revoke the cache (DT-10).
    #[test]
    fn backpressured_promote_degrades_to_not_cached() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        let payload = vec![0x22; MAX_MUTATION_FRAME_DATA_BYTES];
        queue_updates_until_one_frame_from_full(&mut client);

        assert_eq!(
            client.promote(0, &payload),
            CacheMutation::Skipped,
            "a promote that cannot be queued is simply not cached"
        );
        assert_eq!(
            client.state(),
            CacheState::Active,
            "a dropped promote must not revoke the cache"
        );
    }

    /// A request written while a mutation frame is mid-stream would corrupt
    /// both frames. The read degrades to a miss — origin is authoritative, so
    /// a miss is the correct answer — and the cache stays alive (DT-10).
    #[test]
    fn cache_read_is_a_miss_while_a_mutation_frame_drains() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        let pending_before = client.pending_frame.len();
        assert!(pending_before > 0);

        let start = Instant::now();
        assert_eq!(client.read(0, &mut [0u8; 16]), CacheRead::Miss);
        assert!(
            start.elapsed() < Duration::from_millis(10),
            "a draining read must not wait on IPC"
        );
        assert_eq!(
            client.state(),
            CacheState::Active,
            "degrading to a miss must keep the cache alive"
        );
        assert_eq!(
            client.pending_frame.len(),
            pending_before,
            "a deferred read must leave the mutation backlog untouched"
        );
    }

    /// The pending backlog is bounded. An `Update` that cannot enter it is
    /// converted to `MSG_INVALIDATE` (DT-11) and the queue never exceeds the
    /// cap — the conversion is not a licence to grow past it.
    #[test]
    fn pending_mutation_backlog_is_bounded() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        fill_pending_to(&mut client, MAX_PENDING_MUTATION_BYTES - FRAME_HEADER_LEN);
        let payload = vec![0x44; MAX_MUTATION_FRAME_DATA_BYTES];

        assert_eq!(
            client.update(0, &payload),
            CacheMutation::Skipped,
            "a write-mirror that cannot be queued is not silently accepted"
        );
        assert_eq!(
            client.pending_frame.len(),
            MAX_PENDING_MUTATION_BYTES,
            "the invalidate fits exactly in the remaining headroom and never grows the cap"
        );
        assert!(client.pending_frame.len() <= MAX_PENDING_MUTATION_BYTES);
        assert_eq!(
            client.state(),
            CacheState::Active,
            "the converted invalidate keeps the session alive (DT-11)"
        );
    }

    /// A backpressured `Update` is converted into `MSG_INVALIDATE` for exactly
    /// its range and the cache session stays `Active` (DT-11). The worker
    /// therefore drops pre-write coverage there and a later read is a miss,
    /// without throwing away every unrelated hot range.
    #[test]
    fn backpressured_update_invalidates_its_range_and_stays_active() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        // Leave exactly one header of headroom: the dropped 64 KiB write-mirror
        // cannot fit, but its 32-byte invalidate can.
        fill_pending_to(&mut client, MAX_PENDING_MUTATION_BYTES - FRAME_HEADER_LEN);
        let payload = vec![0x44; MAX_MUTATION_FRAME_DATA_BYTES];
        let before = client.pending_frame.len();

        assert_eq!(
            client.update(0x2000, &payload),
            CacheMutation::Skipped,
            "a write-mirror that cannot be queued is not cached"
        );
        assert_eq!(
            client.state(),
            CacheState::Active,
            "the invalidate keeps the cache session alive (DT-11)"
        );
        assert_eq!(
            client.pending_frame.len(),
            before + FRAME_HEADER_LEN,
            "only the 32-byte invalidate may join the queue"
        );

        let mut header_bytes = [0u8; FRAME_HEADER_LEN];
        header_bytes.copy_from_slice(&client.pending_frame[before..before + FRAME_HEADER_LEN]);
        let header = FrameHeader::decode(&header_bytes).expect("valid invalidate header");
        assert_eq!(header.msg_type, MSG_INVALIDATE_REQ);
        assert_eq!(
            header.offset, 0x2000,
            "the invalidate covers the dropped range start"
        );
        assert_eq!(
            header.aux as usize, MAX_MUTATION_FRAME_DATA_BYTES,
            "the invalidate covers the dropped range length"
        );
        assert_eq!(header.payload_len, 0, "an invalidate carries no payload");
    }

    /// When even the `MSG_INVALIDATE` cannot be queued, nothing can stop a
    /// stale Hit — the cache must revoke (DT-11).
    #[test]
    fn invalidate_that_cannot_queue_revokes_the_cache() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        saturate_with_updates(&mut client);
        // Exactly full: neither the write-mirror nor its invalidate fits.
        fill_pending_to(&mut client, MAX_PENDING_MUTATION_BYTES);
        let payload = vec![0x44; MAX_MUTATION_FRAME_DATA_BYTES];

        assert_eq!(
            client.update(0x2000, &payload),
            CacheMutation::Failed,
            "an unqueueable invalidate must be refused, not dropped"
        );
        assert_eq!(
            client.state(),
            CacheState::Unavailable,
            "when nothing can stop a stale Hit the cache revokes (DT-11)"
        );
    }

    #[test]
    fn oversize_mutation_disables_cache_without_touching_ipc() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        worker_sock.set_nonblocking(true).unwrap();
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        let payload = vec![0xEE; MAX_MUTATION_FRAME_DATA_BYTES + 1];

        assert_eq!(client.update(0, &payload), CacheMutation::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);
        let mut byte = [0u8; 1];
        match worker_sock.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            other => panic!("oversize mutation unexpectedly reached IPC: {other:?}"),
        }
    }

    #[test]
    fn oversized_cache_read_is_miss_before_frame_send() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        worker_sock.set_nonblocking(true).unwrap();
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        let mut destination = vec![0u8; 16 * 1024 * 1024 + 1];

        let start = Instant::now();
        assert_eq!(client.read(0, &mut destination), CacheRead::Miss);
        assert!(start.elapsed() < Duration::from_millis(10));
        assert_eq!(client.state(), CacheState::Active);
        let mut byte = [0u8; 1];
        assert_eq!(
            worker_sock.read(&mut byte).unwrap_err().kind(),
            ErrorKind::WouldBlock,
            "an oversize cache read must never reach the worker"
        );
    }

    #[test]
    fn invalid_timeout_configuration_disables_cache_before_io() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::ZERO, 1024 * 1024);

        assert_eq!(client.state(), CacheState::Unavailable);
        assert!(client.perform_handshake().is_err());
        assert_eq!(client.read(0, &mut [0u8; 16]), CacheRead::Miss);
    }

    /// A mutation must not zero the last confirmed occupancy sample.
    #[test]
    fn mutation_preserves_last_confirmed_occupancy() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        // Handshake response reports 64 KiB already cached.
        client.cached_bytes = 64 << 10;

        assert_eq!(client.update(0, &[1, 2, 3]), CacheMutation::Accepted);
        assert_eq!(
            client.cached_bytes(),
            64 << 10,
            "a queued mutation is not evidence of allocation and must not clear the sample"
        );
    }

    /// A heartbeat issued into a write-mirror backlog would queue behind the
    /// frames and miss NFR-1's 50 ms bound, which fail-closes the cache. The
    /// client skips the round-trip and reports the last confirmed sample.
    #[test]
    fn heartbeat_is_deferred_while_write_mirrors_drain() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        worker_sock.set_nonblocking(true).unwrap();
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        client.cached_bytes = 64 << 10;

        assert_eq!(client.update(0, &[1, 2, 3]), CacheMutation::Accepted);
        let start = Instant::now();
        assert_eq!(client.refresh_cached_bytes(), Ok(64 << 10));
        assert!(start.elapsed() < Duration::from_millis(10));
        assert_eq!(client.state(), CacheState::Active);
        // Only the mutation frame reached the worker; no heartbeat followed.
        let drained = drain_available(&mut worker_sock);
        assert!(!drained.is_empty(), "the mutation frame must be queued");
        assert_eq!(
            worker_sock.read(&mut [0u8; 1]).unwrap_err().kind(),
            ErrorKind::WouldBlock,
            "a heartbeat must not be queued into a write-mirror backlog"
        );
    }

    /// Telemetry round-trips are spaced so the serve loop cannot issue one
    /// heartbeat per NBD request.
    #[test]
    fn heartbeat_is_spaced_to_one_per_interval() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            let served = std::cell::Cell::new(0u32);
            while served.get() < 2 {
                let mut request = [0u8; FRAME_HEADER_LEN];
                match worker_sock.read_exact(&mut request) {
                    Ok(()) => {}
                    Err(_) => break,
                }
                let request = FrameHeader::decode(&request).expect("valid request header");
                let response = FrameHeader {
                    msg_type: MSG_HEARTBEAT_RESP,
                    status: STATUS_OK,
                    correlation_id: request.correlation_id,
                    offset: 1024 * 1024,
                    payload_len: 0,
                    aux: 64,
                };
                if worker_sock.write_all(&response.encode()).is_err() {
                    break;
                }
                served.set(served.get() + 1);
            }
            served.get()
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(client.refresh_cached_bytes(), Ok(64 << 10));
        // Second call inside the interval is a no-op returning the last sample.
        assert_eq!(client.refresh_cached_bytes(), Ok(64 << 10));
        assert_eq!(client.state(), CacheState::Active);
        // Dropping the client closes the socket so the stub worker unblocks
        // even though the spaced heartbeat never sent a second request.
        drop(client);
        assert_eq!(
            worker.join().unwrap(),
            1,
            "only one heartbeat round-trip may reach the worker per interval"
        );
    }

    /// A cache read queued behind a write-mirror burst would miss NFR-1's
    /// bound and revoke the cache. It is deferred as a miss instead.
    #[test]
    fn cache_read_is_deferred_while_write_mirrors_drain() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        worker_sock.set_nonblocking(true).unwrap();
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(client.update(0, &[1, 2, 3]), CacheMutation::Accepted);
        let mut destination = [0u8; 16];
        let start = Instant::now();
        assert_eq!(client.read(0, &mut destination), CacheRead::Miss);
        assert!(start.elapsed() < Duration::from_millis(10));
        assert_eq!(client.state(), CacheState::Active);
        // Drain the mutation frame; no read request may follow it.
        let drained = drain_available(&mut worker_sock);
        assert!(!drained.is_empty());
        assert_eq!(
            worker_sock.read(&mut [0u8; 1]).unwrap_err().kind(),
            ErrorKind::WouldBlock,
            "a cache read must not be queued into a write-mirror backlog"
        );
    }

    /// Promotes come from the read path itself and must not suppress the very
    /// reads that produce them, or a cold read stream would never hit.
    #[test]
    fn promote_does_not_defer_subsequent_reads() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        worker_sock.set_nonblocking(true).unwrap();
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(client.promote(0, &[1, 2, 3]), CacheMutation::Accepted);
        // The read is still issued: promotes never gate the read path. The
        // stub worker never answers, so the read hits the 50 ms deadline and
        // fail-closes — proving the request left the client (a deferred read
        // returns Miss without any I/O).
        let mut destination = [0u8; 16];
        assert_eq!(client.read(0, &mut destination), CacheRead::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);
        let drained = drain_available(&mut worker_sock);
        assert!(
            drained.len() > FRAME_HEADER_LEN,
            "both the promote frame and the read request must reach the worker"
        );
    }

    /// Reads whatever a nonblocking peer socket has already buffered.
    fn drain_available(socket: &mut UnixStream) -> Vec<u8> {
        let mut collected = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            match socket.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => collected.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        collected
    }

    #[test]
    fn oversized_heartbeat_telemetry_fails_closed() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            worker_sock.read_exact(&mut request).unwrap();
            let request = FrameHeader::decode(&request).expect("valid request header");
            let response = FrameHeader {
                msg_type: MSG_HEARTBEAT_RESP,
                status: STATUS_OK,
                correlation_id: request.correlation_id,
                offset: 1024 * 1024,
                payload_len: MAX_GPU_BUDGET_PAYLOAD_BYTES + 1,
                aux: 64,
            };
            worker_sock.write_all(&response.encode()).unwrap();
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(
            client.refresh_cached_bytes(),
            Err("GPU cache worker heartbeat telemetry exceeded its limit")
        );
        assert_eq!(client.state(), CacheState::Unavailable);
        assert_eq!(client.cached_bytes(), 0);
        worker.join().unwrap();
    }

    /// Serves one heartbeat response carrying `payload` after reading the
    /// request, so the client sees a well-formed frame with a caller-chosen body.
    fn serve_heartbeat_payload(mut worker_sock: UnixStream, payload: Vec<u8>, aux: u32) {
        let mut request = [0u8; FRAME_HEADER_LEN];
        worker_sock
            .read_exact(&mut request)
            .expect("heartbeat request");
        let request = FrameHeader::decode(&request).expect("valid request header");
        let response = FrameHeader {
            msg_type: MSG_HEARTBEAT_RESP,
            status: STATUS_OK,
            correlation_id: request.correlation_id,
            offset: 1024 * 1024,
            payload_len: payload.len() as u32,
            aux,
        };
        worker_sock
            .write_all(&response.encode())
            .expect("heartbeat header");
        worker_sock.write_all(&payload).expect("heartbeat body");
    }

    fn sample_cache_telemetry(
        logical_cached_bytes: u64,
        sampled_at_unix_ms: u64,
    ) -> WorkerCacheTelemetry {
        use ramshared_vram::{CodecCapability, CodecState, CodecTelemetry};

        WorkerCacheTelemetry {
            schema_version: 1,
            sampled_at_unix_ms,
            codec: CodecTelemetry::new(CodecCapability::Available, CodecState::Ready, None),
            logical_cached_bytes,
            physical_cache_slab_bytes: 2 * 1024 * 1024,
            codec_workspace_bytes: 0,
            compressed_payload_bytes: 128,
            raw_payload_bytes: 64,
            metadata_bytes: 32,
            raw_bypass_bytes: 2048,
            codec_integrity_errors: 0,
            codec_decode_errors: 0,
            codec_timeouts: 0,
        }
    }

    #[test]
    fn telemetry_envelope_rejects_unknown_version_or_oversize() {
        // Unknown envelope version: the sample is omitted, never accepted and
        // never turned into a client failure. Physical header fields survive.
        let mut unknown =
            WorkerTelemetryEnvelope::new(1_000, None, Some(sample_cache_telemetry(4096, 1_000)));
        unknown.schema_version = 99;
        let unknown_payload = serde_json::to_vec(&unknown).unwrap();

        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            serve_heartbeat_payload(worker_sock, unknown_payload, 64);
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        assert_eq!(client.refresh_cached_bytes(), Ok(64 << 10));
        assert_eq!(
            client.cache_telemetry(),
            None,
            "an unknown envelope version must not be accepted as cache telemetry"
        );
        assert_eq!(client.gpu_budget_telemetry(), None);
        assert_eq!(
            client.state(),
            CacheState::Active,
            "an unknown envelope version is omitted, not a transport failure"
        );
        worker.join().unwrap();

        // Oversize: rejected whole. The client fails closed and never truncates
        // an oversize payload into a plausible-looking sample.
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            worker_sock.read_exact(&mut request).unwrap();
            let request = FrameHeader::decode(&request).expect("valid request header");
            let response = FrameHeader {
                msg_type: MSG_HEARTBEAT_RESP,
                status: STATUS_OK,
                correlation_id: request.correlation_id,
                offset: 1024 * 1024,
                payload_len: MAX_GPU_BUDGET_PAYLOAD_BYTES + 1,
                aux: 64,
            };
            worker_sock.write_all(&response.encode()).unwrap();
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        assert!(client.refresh_cached_bytes().is_err());
        assert_eq!(client.state(), CacheState::Unavailable);
        assert_eq!(client.cache_telemetry(), None);
        worker.join().unwrap();
    }

    #[test]
    fn logical_bytes_never_replace_physical_cached_bytes() {
        // The envelope reports 1 GiB of logical cache occupancy. The frame
        // header reports 64 KiB of physical cached bytes. They must never be
        // swapped: `cached_bytes()` is physical and comes from `aux` alone.
        use crate::isolated_origin::BestEffortCache;

        const LOGICAL: u64 = 1 << 30;
        let envelope =
            WorkerTelemetryEnvelope::new(1_000, None, Some(sample_cache_telemetry(LOGICAL, 1_000)));
        let payload = envelope.to_bounded_payload().expect("envelope fits");

        let (client_sock, worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            serve_heartbeat_payload(worker_sock, payload, 64);
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);
        // Refresh through the trait surface the daemon uses, so the forwards
        // stay exercised and cannot silently re-derive either figure.
        assert_eq!(
            BestEffortCache::refresh_cached_bytes(&mut client),
            Ok(64 << 10)
        );

        assert_eq!(
            client.cached_bytes(),
            64 << 10,
            "physical cached bytes must stay the frame-header value"
        );
        let cache = client.cache_telemetry().expect("cache telemetry present");
        assert_eq!(cache.logical_cached_bytes, LOGICAL);
        assert_ne!(
            cache.logical_cached_bytes,
            client.cached_bytes(),
            "logical cache occupancy must never be reported as physical cached bytes"
        );

        // The same separation holds on every read-only trait forward.
        assert_eq!(BestEffortCache::cached_bytes(&client), 64 << 10);
        assert_eq!(BestEffortCache::target_bytes(&client), 1024 * 1024);
        let trait_cache = BestEffortCache::cache_telemetry(&client).expect("trait cache telemetry");
        assert_eq!(trait_cache.logical_cached_bytes, LOGICAL);
        assert_ne!(
            trait_cache.logical_cached_bytes,
            BestEffortCache::cached_bytes(&client)
        );
        assert!(BestEffortCache::gpu_budget_telemetry(&client).is_none());

        worker.join().unwrap();
    }

    #[test]
    fn heartbeat_telemetry_truncation_fails_closed() {
        // The worker advertises a payload and then never sends it. The client
        // must fail closed rather than accept a partial sample.
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            worker_sock.read_exact(&mut request).unwrap();
            let request = FrameHeader::decode(&request).expect("valid request header");
            let response = FrameHeader {
                msg_type: MSG_HEARTBEAT_RESP,
                status: STATUS_OK,
                correlation_id: request.correlation_id,
                offset: 1024 * 1024,
                payload_len: 128,
                aux: 64,
            };
            worker_sock.write_all(&response.encode()).unwrap();
            // Body deliberately omitted.
            drop(worker_sock);
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(
            client.refresh_cached_bytes(),
            Err("GPU cache worker heartbeat telemetry was truncated")
        );
        assert_eq!(client.state(), CacheState::Unavailable);
        assert_eq!(client.cache_telemetry(), None);
        worker.join().unwrap();
    }

    #[test]
    fn heartbeat_mismatched_correlation_fails_closed() {
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let worker = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            worker_sock.read_exact(&mut request).unwrap();
            let request = FrameHeader::decode(&request).expect("valid request header");
            let response = FrameHeader {
                msg_type: MSG_HEARTBEAT_RESP,
                status: STATUS_OK,
                correlation_id: request.correlation_id.wrapping_add(1),
                offset: 1024 * 1024,
                payload_len: 0,
                aux: 64,
            };
            worker_sock.write_all(&response.encode()).unwrap();
        });
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        assert_eq!(
            client.refresh_cached_bytes(),
            Err("GPU cache worker heartbeat mismatched")
        );
        assert_eq!(client.state(), CacheState::Unavailable);
        assert_eq!(client.cache_telemetry(), None);
        worker.join().unwrap();
    }

    #[test]
    fn codec_timeout_falls_back_to_origin() {
        use crate::isolated_origin::AuthoritativeOriginBackend;
        use crate::origin_cache::OriginStorage;
        use crate::{BlockBackend, IoError};

        struct MemoryOrigin(Vec<u8>);
        impl OriginStorage for MemoryOrigin {
            fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, IoError> {
                let start = off as usize;
                let end = (start + buf.len()).min(self.0.len());
                if start >= self.0.len() {
                    return Ok(0);
                }
                buf[..end - start].copy_from_slice(&self.0[start..end]);
                Ok(end - start)
            }
            fn write_at(&mut self, off: u64, data: &[u8]) -> Result<usize, IoError> {
                let start = off as usize;
                let end = start + data.len();
                if end > self.0.len() {
                    self.0.resize(end, 0);
                }
                self.0[start..end].copy_from_slice(data);
                Ok(data.len())
            }
            fn sync_data(&mut self) -> Result<(), IoError> {
                Ok(())
            }
        }

        // A worker that accepts the read request and never answers: the decoder
        // is stuck in a driver call that DT-3 explicitly cannot preempt.
        let (client_sock, mut worker_sock) = UnixStream::pair().expect("socketpair failed");
        let stalled = std::thread::spawn(move || {
            let mut request = [0u8; FRAME_HEADER_LEN];
            if worker_sock.read_exact(&mut request).is_ok() {
                // Hold the stream open and never respond.
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let client = IpcCacheClient::new(client_sock, Duration::from_millis(30), 1024 * 1024);
        let mut backend =
            AuthoritativeOriginBackend::new(MemoryOrigin(b"origin!!".to_vec()), client, 8, 4)
                .expect("origin backend");

        let start = Instant::now();
        let mut destination = [0u8; 8];
        backend.read_at(0, &mut destination).expect("origin read");
        let elapsed = start.elapsed();

        // The origin is authoritative: the reader gets the durable bytes, and
        // the stalled cache path did not extend the cache read deadline.
        assert_eq!(&destination, b"origin!!");
        assert!(
            elapsed < Duration::from_millis(150),
            "a stalled decoder must fall back to the origin within the cache deadline, not hang ({elapsed:?})"
        );
        assert_eq!(backend.telemetry().fallback_reads, 1);
        stalled.join().unwrap();
    }
}
