//! Authoritative origin I/O with a bounded, revocable cache boundary.
//!
//! The origin never depends on a cache response for correctness. Cache reads
//! have a hard deadline; cache mutations are non-blocking and any queue,
//! transport, protocol, or timeout fault permanently revokes that client.

pub mod origin_tracker;
pub mod origin_policy;

#[cfg(test)]
mod origin_tests;

pub use origin_tracker::{
    CacheRead, CacheMutation, IsolatedCacheRequest, IsolatedCacheControl, IsolatedCacheWorker, BestEffortCache, DisabledCache, BoundedCacheClient, isolated_cache_channel
};
pub use origin_policy::AuthoritativeOriginBackend;
