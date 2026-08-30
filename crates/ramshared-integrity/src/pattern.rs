//! Reproducible test patterns indexed by block number (SPEC §14.2 `test-integrity`).
//! Deterministic: `verify_block` regenerates the expected pattern without keeping state.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pattern {
    Zero,
    Sequential,
    Random,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatternError {
    InvalidStride { stride: usize, page_size: usize },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternError::InvalidStride { stride, page_size } => {
                write!(f, "pattern scanning stride ({stride}) does not evenly divide memory page size ({page_size})")
            }
        }
    }
}

impl std::error::Error for PatternError {}

/// Fills `buf` with the deterministic pattern matching block index `idx`.
pub fn fill_block(buf: &mut [u8], idx: u64, kind: Pattern) -> Result<(), PatternError> {
    let stride = buf.len();
    if stride == 0 || 4096 % stride != 0 {
        return Err(PatternError::InvalidStride { stride, page_size: 4096 });
    }

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
    Ok(())
}

/// Returns `true` if `buf` matches the expected pattern for block index `idx`.
pub fn verify_block(buf: &[u8], idx: u64, kind: Pattern) -> Result<bool, PatternError> {
    let stride = buf.len();
    if stride == 0 || 4096 % stride != 0 {
        return Err(PatternError::InvalidStride { stride, page_size: 4096 });
    }

    let mut expected = vec![0u8; buf.len()];
    fill_block(&mut expected, idx, kind)?;
    Ok(expected == buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_then_verify_round_trips() {
        for kind in [Pattern::Zero, Pattern::Sequential, Pattern::Random] {
            let mut buf = vec![0u8; 4096];
            fill_block(&mut buf, 42, kind).expect("valid stride");
            assert!(verify_block(&buf, 42, kind).expect("valid stride"), "{kind:?}");
        }
    }

    #[test]
    fn corruption_breaks_verify() {
        let mut buf = vec![0u8; 4096];
        fill_block(&mut buf, 7, Pattern::Random).expect("valid stride");
        buf[1234] ^= 0x01;
        assert!(!verify_block(&buf, 7, Pattern::Random).expect("valid stride"));
    }

    #[test]
    fn different_blocks_differ_and_wrong_index_fails() {
        let mut a = vec![0u8; 4096];
        let mut b = vec![0u8; 4096];
        fill_block(&mut a, 1, Pattern::Random).expect("valid stride");
        fill_block(&mut b, 2, Pattern::Random).expect("valid stride");
        assert_ne!(a, b); // pattern differs by block index
        assert!(!verify_block(&a, 2, Pattern::Random).expect("valid stride")); // wrong index verification fails
    }

    #[test]
    fn invalid_stride_fails() {
        let mut buf = vec![0u8; 1000];
        let err = fill_block(&mut buf, 1, Pattern::Zero).expect_err("should fail");
        assert_eq!(err, PatternError::InvalidStride { stride: 1000, page_size: 4096 });

        let err2 = verify_block(&buf, 1, Pattern::Zero).expect_err("should fail");
        assert_eq!(err2, PatternError::InvalidStride { stride: 1000, page_size: 4096 });
    }
}
