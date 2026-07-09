//! ramshared-integrity — Block integrity verification (SPEC §8.1, §14.2).
//!
//! For `--debug-checksum` mode: Fast non-cryptographic hashing + pre-allocated
//! checksum table indexed by block number (detects corruption/torn reads in VRAM)
//! and reproducible patterns for `test-integrity`. Pure logic, no root required.
#![forbid(unsafe_code)]

pub mod hash;
pub mod pattern;

pub use hash::{ChecksumTable, block_hash};
pub use pattern::{Pattern, fill_block, verify_block};
