//! Block hashing (FNV-1a 64) + pre-allocated checksum table (SPEC §8.1).
//! **Not cryptographic** — meant for detecting memory corruption and torn reads, not security.

use std::error::Error;
use std::fmt;

pub const DEFAULT_BLOCK_SIZE: usize = 4096;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit hash over block bytes.
pub fn block_hash(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Semantic error for block verification failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumMismatchError {
    /// The computed hash does not match the expected hash.
    Mismatch {
        idx: usize,
        expected: u64,
        computed: u64,
    },
    /// The block index is out of physical bounds.
    OutOfBounds { idx: usize },
    /// Invalid buffer length (must be non-empty and power-of-two >= 512).
    InvalidBufferLength { len: usize },
}

impl fmt::Display for ChecksumMismatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch {
                idx,
                expected,
                computed,
            } => {
                write!(
                    f,
                    "checksum mismatch at block {idx}: expected {expected:#x}, got {computed:#x}"
                )
            }
            Self::OutOfBounds { idx } => {
                write!(f, "block index {idx} out of bounds")
            }
            Self::InvalidBufferLength { len } => {
                write!(f, "invalid checksum buffer length {len}")
            }
        }
    }
}

impl Error for ChecksumMismatchError {}

/// Checksum table indexed by block number, **pre-allocated** (prevents allocations in the hot path,
/// SPEC §8). `None` indicates the block has not been written yet.
pub struct ChecksumTable {
    sums: Vec<Option<u64>>,
}

impl ChecksumTable {
    pub fn new(n_blocks: usize) -> Self {
        Self {
            sums: vec![None; n_blocks],
        }
    }

    /// Records the hash of a written block. Returns `false` if `idx` is out of bounds or data is invalid.
    pub fn record(&mut self, idx: usize, data: &[u8]) -> bool {
        if data.is_empty() || data.len() < 512 || !data.len().is_power_of_two() {
            return false;
        }
        let Some(slot) = self.sums.get_mut(idx) else {
            return false;
        };
        *slot = Some(block_hash(data));
        true
    }

    /// Verifies the read block against the recorded hash.
    /// `None` = never written (ok); `Some(true)` = matches; `Some(false)` =
    /// mismatch (corruption/torn read) -> the caller returns an I/O error.
    pub fn verify(&self, idx: usize, data: &[u8]) -> Option<bool> {
        if data.is_empty() || data.len() < 512 || !data.len().is_power_of_two() {
            return Some(false);
        }
        let Some(slot) = self.sums.get(idx) else {
            return Some(false); // out of bounds = invalid
        };
        let Some(expected) = slot else {
            return None;
        };
        Some(*expected == block_hash(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_data_same_hash_diff_data_diff_hash() {
        let a = vec![1u8; 4096];
        let mut b = a.clone();
        assert_eq!(block_hash(&a), block_hash(&b));
        b[2048] ^= 0x01;
        assert_ne!(block_hash(&a), block_hash(&b));
    }

    #[test]
    fn table_records_and_verifies() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        assert!(t.record(3, &data));
        assert_eq!(t.verify(3, &data), Some(true));
    }

    #[test]
    fn table_detects_corruption() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        t.record(3, &data);
        let mut corrupt = data.clone();
        corrupt[0] ^= 0xff;
        assert_eq!(t.verify(3, &corrupt), Some(false));
    }

    #[test]
    fn unwritten_block_is_none_oob_is_invalid() {
        let mut t = ChecksumTable::new(2);
        assert_eq!(t.verify(0, &[0u8; 4096]), None); // never written
        assert_eq!(t.verify(99, &[0u8; 4096]), Some(false)); // out of bounds
        assert!(!t.record(99, &[0u8; 4096]));
    }

    #[test]
    fn table_rejects_empty_and_wrong_length_data() {
        let mut t = ChecksumTable::new(8);
        assert!(!t.record(3, &[])); // empty
        assert!(!t.record(3, &[0u8; 123])); // not power of two / < 512

        assert_eq!(t.verify(3, &[]), Some(false));
        assert_eq!(t.verify(3, &[0u8; 123]), Some(false));
    }

    #[test]
    fn table_supports_power_of_two_block_sizes() {
        let mut t = ChecksumTable::new(8);
        let data_512 = vec![0xCDu8; 512];
        let data_65536 = vec![0xEFu8; 65536];

        assert!(t.record(1, &data_512));
        assert_eq!(t.verify(1, &data_512), Some(true));

        assert!(t.record(2, &data_65536));
        assert_eq!(t.verify(2, &data_65536), Some(true));
    }
}
