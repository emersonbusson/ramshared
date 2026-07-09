//! ramshared-broker — protocol (JSON-lines) + model + policy of the Memory Broker arbiter.
//!
//! SPEC: `docs/specs/no-milestone/memory-broker/SPEC.md` ITEM-3/ITEM-4 (RF-B1, RF-B2, RF-B3, RF-L1; DT-1).
//!
//! **Pure library, testable without network/root/GPU**: model types ([`model`]), JSON-lines codec
//! ([`protocol`], DT-1), and — in ITEM-4 — the slice map and the arbiter (injected clock). The
//! plumbing (sockets, worker, IO) lives in the `ramsharedd` daemon (crate `ramshared-wsl2d`, ITEM-8).
#![forbid(unsafe_code)]

pub mod arbiter;
pub mod model;
pub mod protocol;
pub mod slices;
