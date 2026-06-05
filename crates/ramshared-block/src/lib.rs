//! ramshared-block — protocolo NBD fixed-newstyle + modelo de I/O do tier VRAM.
//!
//! SPEC: `SPECv3-WSL2.md` §8 (atomicidade de I/O) e §10.1 (backend NBD).
//!
//! Núcleo **testável sem root**: parse/encode do wire NBD, a trait
//! [`BlockBackend`] e o mapa de blocos em voo ([`Inflight`], §8.1). A fiação do
//! `/dev/nbdX` (ioctl `NBD_SET_SOCK`/`NBD_DO_IT`) é um módulo separado (precisa de
//! root + device) — esta lib é só o protocolo e a lógica.
#![forbid(unsafe_code)]

pub mod handshake;
pub mod inflight;
pub mod protocol;
pub mod request;

pub use handshake::{HandshakeError, server_handshake};
pub use inflight::Inflight;
pub use protocol::{Command, ProtocolError, Request, encode_simple_reply, parse_request};
pub use request::{BlockBackend, IoError, ServeOutcome, serve};
