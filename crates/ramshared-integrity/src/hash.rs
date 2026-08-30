//! Block hashing (FNV-1a 64) + pre-allocated checksum table (SPEC §8.1).
//! **Not cryptographic** — meant for detecting memory corruption and torn reads, not security.

use std::error::Error;
use std::fmt;

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
    OutOfBounds {
        idx: usize,
    },
}

impl fmt::Display for ChecksumMismatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch { idx, expected, computed } => {
                write!(
                    f,
                    "checksum mismatch at block {idx}: expected {expected:#x}, got {computed:#x}"
                )
            }
            Self::OutOfBounds { idx } => {
                write!(f, "block index {idx} out of bounds")
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

    /// Records the hash of a written block. Returns `false` if `idx` is out of bounds.
    pub fn record(&mut self, idx: usize, data: &[u8]) -> bool {
        let Some(slot) = self.sums.get_mut(idx) else {
            return false;
        };
        *slot = Some(block_hash(data));
        true
    }

    /// Verifies the read block against the recorded hash.
    /// Returns `Ok(true)` if it matches, `Ok(false)` if never written.
    /// Returns `Err(ChecksumMismatchError)` on mismatch or out of bounds.
    pub fn verify(&self, idx: usize, data: &[u8]) -> Result<bool, ChecksumMismatchError> {
        let Some(slot) = self.sums.get(idx) else {
            return Err(ChecksumMismatchError::OutOfBounds { idx });
        };
        let Some(expected) = slot else {
            return Ok(false);
        };
        let computed = block_hash(data);
        if *expected != computed {
            return Err(ChecksumMismatchError::Mismatch {
                idx,
                expected: *expected,
                computed,
            });
        }
        Ok(true)
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
        assert_eq!(t.verify(3, &data), Ok(true));
    }

    #[test]
    fn table_detects_corruption() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        t.record(3, &data);
        let mut corrupt = data.clone();
        corrupt[0] ^= 0xff;
        let Err(err) = t.verify(3, &corrupt) else {
            panic!("expected error on corruption");
        };
        assert_eq!(
            err,
            ChecksumMismatchError::Mismatch {
                idx: 3,
                expected: block_hash(&data),
                computed: block_hash(&corrupt),
            }
        );
    }

    #[test]
    fn unwritten_block_is_none_oob_is_invalid() {
        let mut t = ChecksumTable::new(2);
        assert_eq!(t.verify(0, &[0u8; 4096]), Ok(false)); // never written
        let Err(err) = t.verify(99, &[0u8; 4096]) else {
            panic!("expected out of bounds error");
        };
        assert_eq!(err, ChecksumMismatchError::OutOfBounds { idx: 99 });
        assert!(!t.record(99, &[0u8; 4096]));
    }
}
