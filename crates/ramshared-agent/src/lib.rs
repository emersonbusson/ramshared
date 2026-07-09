//! RamShared agent (tenant) library: pressure collection (PSI), swap execution
//! over NBD, and session watchdog. The pure logic (parsing `/proc`, assembling argv,
//! watchdog window) lives here and is covered by unit tests; `main.rs` only
//! wires these pieces together with sockets/threads (DT-27).
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md (ITEM-9). Without `unsafe`.
#![forbid(unsafe_code)]

pub mod psi;
pub mod swap;
pub mod watchdog;
