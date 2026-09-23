//! IPC cache client communicating with the isolated GPU cache worker.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::gpu_cache_worker::{
    FRAME_HEADER_LEN, FrameHeader, MSG_DISABLE_REQ, MSG_DISABLE_RESP, MSG_HANDSHAKE_REQ,
    MSG_HANDSHAKE_RESP, MSG_HEARTBEAT_REQ, MSG_HEARTBEAT_RESP, MSG_PROMOTE, MSG_READ_REQ,
    MSG_READ_RESP, MSG_UPDATE, STATUS_MISS, STATUS_OK,
};
use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead};
use crate::origin_cache::CacheState;

pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(50);
/// Handshake allows extra time for the worker to initialize CUDA/Vulkan
/// contexts before the first frame is served (SPEC: DT-2, NFR-1).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Disable/teardown uses a longer timeout to accommodate GPU context cleanup.
/// SPEC: DT-5 (5s bounded supervisor teardown).
pub const DISABLE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct IpcCacheClient {
    socket: UnixStream,
    read_timeout: Duration,
    state: CacheState,
    cached_bytes: u64,
    target_bytes: u64,
    seq: u64,
}

impl IpcCacheClient {
    pub fn new(socket: UnixStream, read_timeout: Duration, target_bytes: u64) -> Self {
        let _ = socket.set_read_timeout(Some(read_timeout));
        let _ = socket.set_write_timeout(Some(read_timeout));
        Self {
            socket,
            read_timeout,
            state: CacheState::Active,
            cached_bytes: 0,
            target_bytes,
            seq: 0,
        }
    }

    pub fn perform_handshake(&mut self) -> Result<(), String> {
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_HANDSHAKE_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset: 0,
            payload_len: 0,
            aux: 0,
        };
        let _ = self.socket.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
        self.socket
            .write_all(&req.encode())
            .map_err(|e| format!("handshake write error: {e}"))?;

        let mut buf = [0u8; FRAME_HEADER_LEN];
        self.socket
            .read_exact(&mut buf)
            .map_err(|e| format!("handshake read error: {e}"))?;

        let resp = FrameHeader::decode(&buf);
        if resp.msg_type != MSG_HANDSHAKE_RESP || resp.correlation_id != self.seq {
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
    }

    pub fn refresh_cached_bytes(&mut self) -> Result<u64, &'static str> {
        if self.state != CacheState::Active {
            return Err("GPU cache worker is unavailable");
        }
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
        if self.socket.write_all(&req.encode()).is_err()
            || self.socket.read_exact(&mut buf).is_err()
        {
            self.fail("heartbeat I/O failed");
            return Err("GPU cache worker heartbeat timed out");
        }
        let resp = FrameHeader::decode(&buf);
        if resp.msg_type != MSG_HEARTBEAT_RESP
            || resp.correlation_id != self.seq
            || resp.status != STATUS_OK
            || resp.offset != self.target_bytes
        {
            self.fail("heartbeat response mismatched");
            return Err("GPU cache worker heartbeat mismatched");
        }
        self.cached_bytes = (resp.aux as u64) << 10;
        Ok(self.cached_bytes)
    }

    fn send_mutation_frame(&mut self, header: FrameHeader, payload: &[u8]) -> CacheMutation {
        if self.state != CacheState::Active {
            return CacheMutation::Skipped;
        }
        // Assemble one frame and send it within the socket write deadline.
        let encoded_header = header.encode();
        let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
        frame.extend_from_slice(&encoded_header);
        frame.extend_from_slice(payload);

        // SOCK_STREAM may accept only part of a large write even when the
        // worker is healthy. write_all completes the frame while the bounded
        // write timeout prevents an unresponsive worker from stalling NBD.
        match self.socket.write_all(&frame) {
            Ok(()) => {
                // Accepted bytes are queued, not evidence of GPU allocation.
                self.cached_bytes = 0;
                CacheMutation::Accepted
            }
            Err(_) => {
                // A timed-out write may have sent a prefix. Discard the stream.
                self.fail("bounded mutation frame write failed");
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
        self.seq = self.seq.saturating_add(1);
        let req = FrameHeader {
            msg_type: MSG_READ_REQ,
            status: STATUS_OK,
            correlation_id: self.seq,
            offset,
            payload_len: 0,
            aux: destination.len() as u32,
        };

        let _ = self.socket.set_read_timeout(Some(self.read_timeout));
        if self.socket.write_all(&req.encode()).is_err() {
            self.fail("read request write failed");
            return CacheRead::Failed;
        }

        let mut hdr_buf = [0u8; FRAME_HEADER_LEN];
        if self.socket.read_exact(&mut hdr_buf).is_err() {
            self.fail("read response timed out");
            return CacheRead::Failed;
        }

        let resp = FrameHeader::decode(&hdr_buf);
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
                if self.socket.read_exact(destination).is_err() {
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

        let _ = self.socket.set_read_timeout(Some(DISABLE_TIMEOUT));
        if self.socket.write_all(&req.encode()).is_err() {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }

        let mut hdr_buf = [0u8; FRAME_HEADER_LEN];
        if self.socket.read_exact(&mut hdr_buf).is_err() {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }

        let resp = FrameHeader::decode(&hdr_buf);
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
}
