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
    /// Invalid buffer length (must be 4096 or 65536).
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

    /// Records the hash of a written block. Returns an error if `idx` is out of bounds or data is invalid.
    pub fn record(&mut self, idx: usize, data: &[u8]) -> Result<(), ChecksumMismatchError> {
        let len = data.len();
        if len != 4096 && len != 65536 {
            return Err(ChecksumMismatchError::InvalidBufferLength { len });
        }
        let slot = self.sums.get_mut(idx).ok_or(ChecksumMismatchError::OutOfBounds { idx })?;
        *slot = Some(block_hash(data));
        Ok(())
    }

    /// Verifies the read block against the recorded hash.
    /// Returns `Ok(())` if valid or unwritten. Returns an error if mismatched or bounds check fails.
    pub fn verify(&self, idx: usize, data: &[u8]) -> Result<(), ChecksumMismatchError> {
        let len = data.len();
        if len != 4096 && len != 65536 {
            return Err(ChecksumMismatchError::InvalidBufferLength { len });
        }
        let slot = self.sums.get(idx).ok_or(ChecksumMismatchError::OutOfBounds { idx })?;
        let expected = match slot {
            Some(e) => *e,
            None => return Ok(()),
        };
        let computed = block_hash(data);
        if expected != computed {
            return Err(ChecksumMismatchError::Mismatch {
                idx,
                expected,
                computed,
            });
        }
        Ok(())
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
        assert_eq!(t.record(3, &data), Ok(()));
        assert_eq!(t.verify(3, &data), Ok(()));
    }

    #[test]
    fn table_detects_corruption() {
        let mut t = ChecksumTable::new(8);
        let data = vec![0xABu8; 4096];
        assert_eq!(t.record(3, &data), Ok(()));
        let mut corrupt = data.clone();
        corrupt[0] ^= 0xff;
        assert!(matches!(t.verify(3, &corrupt), Err(ChecksumMismatchError::Mismatch { .. })));
    }

    #[test]
    fn unwritten_block_is_none_oob_is_invalid() {
        let mut t = ChecksumTable::new(2);
        assert_eq!(t.verify(0, &[0u8; 4096]), Ok(())); // never written
        assert!(matches!(t.verify(99, &[0u8; 4096]), Err(ChecksumMismatchError::OutOfBounds { .. })));
        assert!(matches!(t.record(99, &[0u8; 4096]), Err(ChecksumMismatchError::OutOfBounds { .. })));
    }

    #[test]
    fn table_rejects_empty_and_wrong_length_data() {
        let mut t = ChecksumTable::new(8);
        assert!(matches!(t.record(3, &[]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));
        assert!(matches!(t.record(3, &[0u8; 123]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));
        assert!(matches!(t.record(3, &[0u8; 512]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));

        assert!(matches!(t.verify(3, &[]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));
        assert!(matches!(t.verify(3, &[0u8; 123]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));
        assert!(matches!(t.verify(3, &[0u8; 512]), Err(ChecksumMismatchError::InvalidBufferLength { .. })));
    }

    #[test]
    fn table_supports_power_of_two_block_sizes() {
        let mut t = ChecksumTable::new(8);
        let data_65536 = vec![0xEFu8; 65536];

        assert_eq!(t.record(2, &data_65536), Ok(()));
        assert_eq!(t.verify(2, &data_65536), Ok(()));
    }
}
