//! Authoritative origin I/O with a bounded, revocable cache boundary.
//!
//! The origin never depends on a cache response for correctness. Cache reads
//! have a hard deadline; cache mutations are non-blocking and any queue,
//! transport, protocol, or timeout fault permanently revokes that client.

use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use crate::origin_cache::CacheState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheRead {
    Hit,
    Miss,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheMutation {
    Accepted,
    Skipped,
    Failed,
}

/// Cache-side data messages never carry the origin file handle.
#[derive(Debug)]
pub enum IsolatedCacheRequest {
    Read {
        offset: u64,
        len: usize,
        reply: SyncSender<Result<Option<Vec<u8>>, String>>,
    },
    Update {
        offset: u64,
        data: Vec<u8>,
    },
    Promote {
        offset: u64,
        data: Vec<u8>,
    },
}

/// Revocation uses a dedicated lane so a saturated data queue cannot conceal
/// release. The worker acknowledges only after its cache resources are gone.
#[derive(Debug)]
pub enum IsolatedCacheControl {
    Disable {
        reply: SyncSender<Result<(), String>>,
    },
}

pub struct IsolatedCacheWorker {
    pub requests: Receiver<IsolatedCacheRequest>,
    pub control: Receiver<IsolatedCacheControl>,
}

pub trait BestEffortCache {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead;
    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation;
    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation;
    fn disable(&mut self) -> CacheMutation;
    fn state(&self) -> CacheState;

    fn cached_bytes(&self) -> u64 {
        0
    }

    fn target_bytes(&self) -> u64 {
        0
    }
}

/// Fail-closed cache used until a separately supervised GPU worker is wired.
#[derive(Default)]
pub struct DisabledCache;

impl BestEffortCache for DisabledCache {
    fn read(&mut self, _offset: u64, _destination: &mut [u8]) -> CacheRead {
        CacheRead::Miss
    }

    fn update(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
        CacheMutation::Skipped
    }

    fn promote(&mut self, _offset: u64, _data: &[u8]) -> CacheMutation {
        CacheMutation::Skipped
    }

    fn disable(&mut self) -> CacheMutation {
        CacheMutation::Accepted
    }

    fn state(&self) -> CacheState {
        CacheState::Unavailable
    }
}

/// Bounded client for an isolated cache worker. It never performs a blocking
/// send. Reads and release acknowledgements wait for `read_timeout` at most.
pub struct BoundedCacheClient {
    requests: SyncSender<IsolatedCacheRequest>,
    control: SyncSender<IsolatedCacheControl>,
    read_timeout: Duration,
    state: CacheState,
}

pub fn isolated_cache_channel(
    capacity: usize,
    read_timeout: Duration,
) -> (BoundedCacheClient, IsolatedCacheWorker) {
    let (requests, receiver) = sync_channel(capacity);
    let (control, control_receiver) = sync_channel(1);
    (
        BoundedCacheClient {
            requests,
            control,
            read_timeout,
            state: CacheState::Active,
        },
        IsolatedCacheWorker {
            requests: receiver,
            control: control_receiver,
        },
    )
}

impl BoundedCacheClient {
    fn fail(&mut self) -> CacheMutation {
        self.state = CacheState::Unavailable;
        CacheMutation::Failed
    }

    fn send_mutation(&mut self, request: IsolatedCacheRequest) -> CacheMutation {
        if self.state != CacheState::Active {
            return CacheMutation::Skipped;
        }
        match self.requests.try_send(request) {
            Ok(()) => CacheMutation::Accepted,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => self.fail(),
        }
    }
}

impl BestEffortCache for BoundedCacheClient {
    fn read(&mut self, offset: u64, destination: &mut [u8]) -> CacheRead {
        if self.state != CacheState::Active {
            return CacheRead::Miss;
        }
        let (reply, response) = sync_channel(1);
        let request = IsolatedCacheRequest::Read {
            offset,
            len: destination.len(),
            reply,
        };
        if self.requests.try_send(request).is_err() {
            self.fail();
            return CacheRead::Failed;
        }
        match response.recv_timeout(self.read_timeout) {
            Ok(Ok(Some(bytes))) if bytes.len() == destination.len() => {
                destination.copy_from_slice(&bytes);
                CacheRead::Hit
            }
            Ok(Ok(None)) => CacheRead::Miss,
            Ok(Ok(Some(_)))
            | Ok(Err(_))
            | Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.fail();
                CacheRead::Failed
            }
        }
    }

    fn update(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.send_mutation(IsolatedCacheRequest::Update {
            offset,
            data: data.to_vec(),
        })
    }

    fn promote(&mut self, offset: u64, data: &[u8]) -> CacheMutation {
        self.send_mutation(IsolatedCacheRequest::Promote {
            offset,
            data: data.to_vec(),
        })
    }

    fn disable(&mut self) -> CacheMutation {
        if self.state == CacheState::Off {
            return CacheMutation::Skipped;
        }
        let (reply, acknowledgement) = sync_channel(1);
        if self
            .control
            .try_send(IsolatedCacheControl::Disable { reply })
            .is_err()
        {
            self.state = CacheState::Stuck;
            return CacheMutation::Failed;
        }
        match acknowledgement.recv_timeout(self.read_timeout) {
            Ok(Ok(())) => {
                self.state = CacheState::Off;
                CacheMutation::Accepted
            }
            Ok(Err(_))
            | Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.state = CacheState::Stuck;
                CacheMutation::Failed
            }
        }
    }

    fn state(&self) -> CacheState {
        self.state
    }
}
