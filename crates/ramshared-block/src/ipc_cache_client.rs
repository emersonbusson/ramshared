//! IPC cache client communicating with the isolated GPU cache worker.

use std::io::{self, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::gpu_cache_worker::{
    FRAME_HEADER_LEN, FrameHeader, MAX_IPC_PAYLOAD_BYTES, MSG_DISABLE_REQ, MSG_DISABLE_RESP,
    MSG_HANDSHAKE_REQ, MSG_HANDSHAKE_RESP, MSG_HEARTBEAT_REQ, MSG_HEARTBEAT_RESP, MSG_PROMOTE,
    MSG_READ_REQ, MSG_READ_RESP, MSG_UPDATE, STATUS_MISS, STATUS_OK,
};
use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead, MAX_CACHE_MUTATION_BYTES};
use crate::origin_cache::CacheState;
use ramshared_vram::{
    GpuBudgetTelemetry, MAX_WORKER_TELEMETRY_PAYLOAD_BYTES, WorkerCacheTelemetry,
    WorkerTelemetryEnvelope,
};

pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(50);
/// Handshake allows extra time for the worker to initialize CUDA/Vulkan
/// contexts before the first frame is served (SPEC: DT-2, NFR-1).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Disable/teardown uses a longer timeout to accommodate GPU context cleanup.
/// SPEC: DT-5 (5s bounded supervisor teardown).
pub const DISABLE_TIMEOUT: Duration = Duration::from_secs(5);
/// Heartbeat payload ceiling (DT-8). An envelope larger than this is rejected
/// whole — never truncated into a plausible-looking sample.
const MAX_GPU_BUDGET_PAYLOAD_BYTES: u32 = MAX_WORKER_TELEMETRY_PAYLOAD_BYTES as u32;

/// Largest payload a single cache mutation frame may carry. One frame is one
/// nonblocking send: the origin path must never block on the GPU worker, so a
/// logical mutation larger than this is split by the caller into one frame per
/// slice (see `AuthoritativeOriginBackend::update_cache`).
pub const MAX_MUTATION_FRAME_DATA_BYTES: usize = MAX_CACHE_MUTATION_BYTES;

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

fn try_write_frame(socket: &mut UnixStream, frame: &[u8]) -> io::Result<()> {
    socket.set_nonblocking(true)?;
    let write_result = match socket.write(frame) {
        Ok(written) if written == frame.len() => Ok(()),
        Ok(written) => Err(io::Error::new(
            ErrorKind::WriteZero,
            format!("IPC mutation frame was only partially queued ({written} bytes)"),
        )),
        Err(error) => Err(error),
    };
    let restore_result = socket.set_nonblocking(false);
    write_result.and(restore_result)
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
}

impl IpcCacheClient {
    pub fn new(socket: UnixStream, read_timeout: Duration, target_bytes: u64) -> Self {
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
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }

    pub fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        if self.state != CacheState::Active {
            return Err("GPU cache worker is unavailable");
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
        // Assemble the complete frame before the single nonblocking send.
        let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
        frame.extend_from_slice(&header.encode());
        frame.extend_from_slice(payload);

        // Mutations are optional. If the complete frame cannot be queued in a
        // nonblocking attempt, fail closed and discard the stream.
        if try_write_frame(&mut self.socket, &frame).is_err() {
            self.fail("nonblocking mutation frame write failed");
            return CacheMutation::Failed;
        }

        // Accepted bytes are queued, not evidence of GPU allocation.
        self.cached_bytes = 0;
        self.gpu_budget = None;
        self.cache_telemetry = None;
        CacheMutation::Accepted
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
        if write_all_until(&mut self.socket, &req.encode(), deadline).is_err() {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }

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

    #[test]
    fn saturated_mutation_socket_does_not_block_origin_thread() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(250), 1024 * 1024);
        let payload = vec![0xCD; 1024 * 1024];

        let start = Instant::now();
        let outcome = client.update(0, &payload);
        let elapsed = start.elapsed();

        assert_eq!(outcome, CacheMutation::Failed);
        assert_eq!(client.state(), CacheState::Unavailable);
        assert!(
            elapsed < Duration::from_millis(100),
            "a blocked mutation consumed the origin thread for {elapsed:?}"
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
        worker_sock.read_exact(&mut request).expect("heartbeat request");
        let request = FrameHeader::decode(&request).expect("valid request header");
        let response = FrameHeader {
            msg_type: MSG_HEARTBEAT_RESP,
            status: STATUS_OK,
            correlation_id: request.correlation_id,
            offset: 1024 * 1024,
            payload_len: payload.len() as u32,
            aux,
        };
        worker_sock.write_all(&response.encode()).expect("heartbeat header");
        worker_sock.write_all(&payload).expect("heartbeat body");
    }

    fn sample_cache_telemetry(logical_cached_bytes: u64, sampled_at_unix_ms: u64) -> WorkerCacheTelemetry {
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
        let mut unknown = WorkerTelemetryEnvelope::new(
            1_000,
            None,
            Some(sample_cache_telemetry(4096, 1_000)),
        );
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
        let envelope = WorkerTelemetryEnvelope::new(
            1_000,
            None,
            Some(sample_cache_telemetry(LOGICAL, 1_000)),
        );
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
        let trait_cache =
            BestEffortCache::cache_telemetry(&client).expect("trait cache telemetry");
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
