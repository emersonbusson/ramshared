//! Bounded three-offset CUDA probe planning (SPEC windows-storport-cuda-vram DT-3).
//!
//! Pure offset/pattern logic is unit-tested; hardware path is E2E-only.

/// Probe pattern size (4 KiB).
pub const PROBE_PATTERN_LEN: usize = 4096;

/// Maximum plausible total GPU memory (1 TiB).
pub const MAX_TOTAL_MEMORY_BYTES: usize = 1024 * 1024 * 1024 * 1024;

/// Plan three 4 KiB-aligned offsets: `0`, `align_down(size/2, 4096)`, `size-4096`.
///
/// Requires `size >= 3 * 4096` so the three positions are distinct on the 64 MiB floor.
pub fn plan_probe_offsets(size: usize) -> Result<[usize; 3], ProbePlanError> {
    if size < PROBE_PATTERN_LEN * 3 {
        return Err(ProbePlanError::TooSmall { size });
    }
    if !size.is_multiple_of(PROBE_PATTERN_LEN) {
        return Err(ProbePlanError::Unaligned { size });
    }
    let mid = align_down(size / 2, PROBE_PATTERN_LEN);
    let last = size - PROBE_PATTERN_LEN;
    if mid == 0 || mid == last || mid + PROBE_PATTERN_LEN > size {
        return Err(ProbePlanError::NonDistinct { size, mid, last });
    }
    Ok([0, mid, last])
}

/// Deterministic 4 KiB pattern for an offset (distinct per position).
pub fn pattern_for_offset(offset: usize) -> [u8; PROBE_PATTERN_LEN] {
    let mut pat = [0u8; PROBE_PATTERN_LEN];
    let seed = (offset as u32).wrapping_mul(0x9E37_79B9);
    for (i, b) in pat.iter_mut().enumerate() {
        *b = ((seed as usize).wrapping_add(i).wrapping_mul(131) % 251) as u8;
    }
    // Embed offset low bytes so mid/last patterns differ even if seed collides.
    pat[0] = (offset & 0xff) as u8;
    pat[1] = ((offset >> 8) & 0xff) as u8;
    pat[2] = ((offset >> 16) & 0xff) as u8;
    pat[3] = ((offset >> 24) & 0xff) as u8;
    pat
}

fn align_down(v: usize, align: usize) -> usize {
    v / align * align
}

/// Validates GPU hardware capabilities and total memory against physical sanity limits.
pub fn validate_hardware_specs(
    major: i32,
    minor: i32,
    total_memory: usize,
) -> Result<(), ProbePlanError> {
    if !(1..=99).contains(&major) || !(0..=99).contains(&minor) {
        return Err(ProbePlanError::InvalidComputeCapability { major, minor });
    }
    if total_memory == 0 || total_memory > MAX_TOTAL_MEMORY_BYTES {
        return Err(ProbePlanError::InvalidTotalMemory { size: total_memory });
    }
    Ok(())
}

/// Errors from pure probe planning (no CUDA).
#[derive(Debug, PartialEq, Eq)]
pub enum ProbePlanError {
    TooSmall {
        size: usize,
    },
    Unaligned {
        size: usize,
    },
    NonDistinct {
        size: usize,
        mid: usize,
        last: usize,
    },
    InvalidComputeCapability {
        major: i32,
        minor: i32,
    },
    InvalidTotalMemory {
        size: usize,
    },
}

impl std::fmt::Display for ProbePlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbePlanError::TooSmall { size } => {
                write!(f, "probe size {size} too small for three 4 KiB patterns")
            }
            ProbePlanError::Unaligned { size } => {
                write!(f, "probe size {size} not 4 KiB aligned")
            }
            ProbePlanError::NonDistinct { size, mid, last } => {
                write!(
                    f,
                    "probe offsets not distinct size={size} mid={mid} last={last}"
                )
            }
            ProbePlanError::InvalidComputeCapability { major, minor } => {
                write!(f, "invalid compute capability major={major} minor={minor}")
            }
            ProbePlanError::InvalidTotalMemory { size } => {
                write!(f, "invalid total memory size {size}")
            }
        }
    }
}

impl std::error::Error for ProbePlanError {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn plan_offsets_for_64mib() {
        let size = 64 * 1024 * 1024;
        let [a, b, c] = plan_probe_offsets(size).unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, size / 2);
        assert_eq!(c, size - 4096);
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn patterns_differ_across_offsets() {
        let p0 = pattern_for_offset(0);
        let p1 = pattern_for_offset(32 * 1024 * 1024);
        let p2 = pattern_for_offset(64 * 1024 * 1024 - 4096);
        assert_ne!(p0, p1);
        assert_ne!(p1, p2);
        assert_ne!(p0, p2);
    }

    #[test]
    fn reject_small_size() {
        assert!(matches!(
            plan_probe_offsets(4096),
            Err(ProbePlanError::TooSmall { .. })
        ));
    }

    #[test]
    fn reject_invalid_compute_capability() {
        assert!(matches!(
            validate_hardware_specs(0, 0, 1024),
            Err(ProbePlanError::InvalidComputeCapability { major: 0, minor: 0 })
        ));
        assert!(matches!(
            validate_hardware_specs(100, 0, 1024),
            Err(ProbePlanError::InvalidComputeCapability {
                major: 100,
                minor: 0
            })
        ));
        assert!(matches!(
            validate_hardware_specs(1, -1, 1024),
            Err(ProbePlanError::InvalidComputeCapability {
                major: 1,
                minor: -1
            })
        ));
        assert!(validate_hardware_specs(8, 9, 1024).is_ok());
    }

    #[test]
    fn reject_invalid_total_memory() {
        assert!(matches!(
            validate_hardware_specs(8, 9, 0),
            Err(ProbePlanError::InvalidTotalMemory { size: 0 })
        ));
        assert!(matches!(
            validate_hardware_specs(8, 9, MAX_TOTAL_MEMORY_BYTES + 1),
            Err(ProbePlanError::InvalidTotalMemory { size: _ })
        ));
        assert!(validate_hardware_specs(8, 9, 1024).is_ok());
    }
}
