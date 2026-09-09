//! Reproducible test patterns indexed by block number (SPEC §14.2 `test-integrity`).
//! Deterministic: `verify_block` regenerates the expected pattern without keeping state.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pattern {
    Zero,
    Sequential,
    Random,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityError {
    CorruptedMemory { offset: usize, bit_flip_mask: u8 },
    InvalidStride { stride: usize, page_size: usize },
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntegrityError::CorruptedMemory {
                offset,
                bit_flip_mask,
            } => {
                write!(
                    f,
                    "corrupted memory at offset {offset}: bit flip mask {bit_flip_mask:#04x}"
                )
            }
            IntegrityError::InvalidStride { stride, page_size } => {
                write!(
                    f,
                    "pattern scanning stride ({stride}) does not evenly divide memory page size ({page_size})"
                )
            }
        }
    }
}

impl std::error::Error for IntegrityError {}

/// Fills `buf` with the deterministic pattern matching block index `idx`.
pub fn fill_block(buf: &mut [u8], idx: u64, kind: Pattern) {
    match kind {
        Pattern::Zero => buf.iter_mut().for_each(|b| *b = 0),
        Pattern::Sequential => {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (idx.wrapping_add(i as u64) & 0xff) as u8;
            }
        }
        Pattern::Random => {
            // xorshift64 seeded by block index (reproducible, but unique per block).
            let mut s = idx.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
            for b in buf.iter_mut() {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                *b = (s & 0xff) as u8;
            }
        }
    }
}

/// Returns `Ok(())` if `buf` matches the expected pattern for block index `idx`, or an `IntegrityError` otherwise.
pub fn verify_block(buf: &[u8], idx: u64, kind: Pattern) -> Result<(), IntegrityError> {
    let page_size = 4096;
    let stride = buf.len();
    #[allow(clippy::manual_is_multiple_of)]
    if stride == 0 || page_size % stride != 0 {
        return Err(IntegrityError::InvalidStride { stride, page_size });
    }

    let mut expected = vec![0u8; stride];
    fill_block(&mut expected, idx, kind);
    for (offset, (&actual, &exp)) in buf.iter().zip(expected.iter()).enumerate() {
        if actual != exp {
            return Err(IntegrityError::CorruptedMemory {
                offset,
                bit_flip_mask: actual ^ exp,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_stride_is_rejected() {
        assert_eq!(
            verify_block(&[], 42, Pattern::Zero),
            Err(IntegrityError::InvalidStride {
                stride: 0,
                page_size: 4096
            })
        );
        let odd_buf = vec![0u8; 1000];
        assert_eq!(
            verify_block(&odd_buf, 42, Pattern::Zero),
            Err(IntegrityError::InvalidStride {
                stride: 1000,
                page_size: 4096
            })
        );
    }

    #[test]
    fn fill_then_verify_round_trips() {
        for kind in [Pattern::Zero, Pattern::Sequential, Pattern::Random] {
            let mut buf = vec![0u8; 4096];
            fill_block(&mut buf, 42, kind);
            assert!(verify_block(&buf, 42, kind).is_ok(), "{kind:?}");
        }
    }

    #[test]
    fn test_pattern_error_display_formats_correctly() {
        let err_corr = IntegrityError::CorruptedMemory {
            offset: 42,
            bit_flip_mask: 0xab,
        };
        assert_eq!(
            err_corr.to_string(),
            "corrupted memory at offset 42: bit flip mask 0xab"
        );

        let err_stride = IntegrityError::InvalidStride {
            stride: 123,
            page_size: 4096,
        };
        assert_eq!(
            err_stride.to_string(),
            "pattern scanning stride (123) does not evenly divide memory page size (4096)"
        );
    }

    #[test]
    fn corruption_breaks_verify() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, 7, Pattern::Random);
        buf[1234] ^= 0x01;
        assert_eq!(
            verify_block(&buf, 7, Pattern::Random),
            Err(IntegrityError::CorruptedMemory {
                offset: 1234,
                bit_flip_mask: 0x01,
            })
        );
    }

    #[test]
    fn different_blocks_differ_and_wrong_index_fails() {
        let mut a = vec![0u8; 4096];
        let mut b = vec![0u8; 4096];
        fill_block(&mut a, 1, Pattern::Random);
        fill_block(&mut b, 2, Pattern::Random);
        assert_ne!(a, b); // pattern differs by block index
        assert!(verify_block(&a, 2, Pattern::Random).is_err()); // wrong index verification fails
    }

    #[test]
    fn test_pattern_generation_known_seed_ok() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, 0, Pattern::Sequential);
        assert_eq!(buf[0], 0);
        assert_eq!(buf[1], 1);
        assert_eq!(buf[255], 255);
        assert_eq!(buf[256], 0); // wraps around
    }

    #[test]
    fn test_pattern_boundary_zero_ok() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, 0, Pattern::Zero);
        assert!(buf.iter().all(|&b| b == 0));
        assert!(verify_block(&buf, 0, Pattern::Zero).is_ok());
    }

    #[test]
    fn test_pattern_boundary_max_ok() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, u64::MAX, Pattern::Random);
        assert!(verify_block(&buf, u64::MAX, Pattern::Random).is_ok());

        let mut buf_seq = vec![0u8; 4096];
        fill_block(&mut buf_seq, u64::MAX, Pattern::Sequential);
        assert!(verify_block(&buf_seq, u64::MAX, Pattern::Sequential).is_ok());
        assert_eq!(buf_seq[0], 255);
        assert_eq!(buf_seq[1], 0);
    }

    #[test]
    fn test_pattern_misaligned_err() {
        // page_size % stride != 0
        let odd_buf = vec![0u8; 123];
        assert_eq!(
            verify_block(&odd_buf, 42, Pattern::Zero),
            Err(IntegrityError::InvalidStride {
                stride: 123,
                page_size: 4096
            })
        );
    }
}
