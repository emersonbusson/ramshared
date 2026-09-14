//! JSON-lines wire format of the agent↔broker protocol (RF-B1 / DT-1).
//!
//! One JSON object per line (`\n`, UTF-8). Low-rate control-plane (~1 msg/s/tenant),
//! debuggable with `nc`/`jq` (ADR-0005). The codec enforces line cap [`MAX_LINE_BYTES`] **before**
//! allocating (anti-DoS, mirrors NBD handshake `MAX_OPT_LEN`).

pub mod codec;
pub mod msg;

pub use codec::{MAX_LINE_BYTES, ProtocolError, read_msg, write_msg};
pub use msg::{Msg, NbdEndpoint, PROTO_VERSION, SliceIo, SwapEntry, TenantMem, TenantStatus, VersionHeader};

#[cfg(test)]
mod tests;
