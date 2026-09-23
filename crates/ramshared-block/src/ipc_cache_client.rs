//! IPC cache client communicating with the isolated GPU cache worker.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::gpu_cache_worker::{
    FRAME_HEADER_LEN, FrameHeader, MSG_DISABLE_REQ, MSG_DISABLE_RESP, MSG_HANDSHAKE_REQ,
    MSG_HANDSHAKE_RESP, MSG_PROMOTE, MSG_READ_REQ, MSG_READ_RESP, MSG_UPDATE, STATUS_MISS,
    STATUS_OK,
};
use crate::isolated_origin::{BestEffortCache, CacheMutation, CacheRead};
use crate::origin_cache::CacheState;

pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(50);

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
        let _ = self.socket.set_read_timeout(Some(self.read_timeout));
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
        self.state = CacheState::Active;
        Ok(())
    }

    fn fail(&mut self) {
        self.state = CacheState::Unavailable;
    }

    fn send_mutation_frame(&mut self, header: FrameHeader, payload: &[u8]) -> CacheMutation {
        if self.state != CacheState::Active {
            return CacheMutation::Skipped;
        }
        // Non-blocking write to satisfy RF-2 and DT-2
        let _ = self.socket.set_nonblocking(true);
        let encoded_header = header.encode();
        let res = self.socket.write_all(&encoded_header).and_then(|()| {
            if !payload.is_empty() {
                self.socket.write_all(payload)
            } else {
                Ok(())
            }
        });
        let _ = self.socket.set_nonblocking(false);

        match res {
            Ok(()) => {
                self.cached_bytes = self
                    .cached_bytes
                    .saturating_add(payload.len() as u64)
                    .min(self.target_bytes);
                CacheMutation::Accepted
            }
            Err(_) => {
                self.fail();
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
            self.fail();
            return CacheRead::Failed;
        }

        let mut hdr_buf = [0u8; FRAME_HEADER_LEN];
        if self.socket.read_exact(&mut hdr_buf).is_err() {
            self.fail();
            return CacheRead::Failed;
        }

        let resp = FrameHeader::decode(&hdr_buf);
        if resp.msg_type != MSG_READ_RESP || resp.correlation_id != self.seq {
            self.fail();
            return CacheRead::Failed;
        }

        match resp.status {
            STATUS_OK => {
                if resp.payload_len as usize != destination.len() {
                    self.fail();
                    return CacheRead::Failed;
                }
                if self.socket.read_exact(destination).is_err() {
                    self.fail();
                    return CacheRead::Failed;
                }
                CacheRead::Hit
            }
            STATUS_MISS => CacheRead::Miss,
            _ => {
                self.fail();
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

        let _ = self.socket.set_read_timeout(Some(self.read_timeout));
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
    fn update_and_promote_are_non_blocking() {
        let (client_sock, _worker_sock) = UnixStream::pair().expect("socketpair failed");
        let mut client = IpcCacheClient::new(client_sock, Duration::from_millis(50), 1024 * 1024);

        let start = Instant::now();
        let payload = vec![0xAB; 4096];
        let update_outcome = client.update(0, &payload);
        let promote_outcome = client.promote(4096, &payload);

        assert!(start.elapsed() < Duration::from_millis(50));
        assert_eq!(update_outcome, CacheMutation::Accepted);
        assert_eq!(promote_outcome, CacheMutation::Accepted);
        assert_eq!(client.cached_bytes(), 8192);
    }
}
