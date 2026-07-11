//! ramshared-block — NBD fixed-newstyle protocol + I/O model of the VRAM tier.
//!
//! SPEC: `SPECv3-WSL2.md` §8 (I/O atomicity) and §10.1 (NBD backend).
//! Also hosts [`VramBackend`] (windows-swap-driver ITEM-2 / DT-6).
//!
//! Core **testable without root**: parse/encode of the NBD wire, the trait
//! [`BlockBackend`] and the map of inflight blocks ([`Inflight`], §8.1). The wiring of
//! `/dev/nbdX` (ioctl `NBD_SET_SOCK`/`NBD_DO_IT`) is a separate module (requires
//! root + device) — this lib is only the protocol and logic.
#![forbid(unsafe_code)]

pub mod handshake;
pub mod inflight;
pub mod protocol;
pub mod request;
pub mod sparse_vram;
pub mod vram_backend;

pub use handshake::{HandshakeError, server_handshake};
pub use inflight::Inflight;
pub use protocol::{Command, ProtocolError, Request, encode_simple_reply, parse_request};
pub use request::{BlockBackend, IoError, ServeOutcome, serve};
pub use sparse_vram::{
    DEFAULT_CHUNK_MIB, SparseVramBackend, chunk_bytes_from_env, idle_free_secs_from_env,
    prealloc_enabled,
};
pub use vram_backend::VramBackend;
