/// Defines the scenario or pattern for the memory stress test payload.
///
/// Provides variants for `Sequential`, `Random`, and `Mixed` patterns.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum StressScenario {
    Sequential,
    Random,
    #[default]
    Mixed,
}

impl std::str::FromStr for StressScenario {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sequential" => Ok(Self::Sequential),
            "random" => Ok(Self::Random),
            "mixed" => Ok(Self::Mixed),
            _ => Err(format!("unknown scenario: {}", s)),
        }
    }
}

impl std::fmt::Display for StressScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sequential => write!(f, "Sequential"),
            Self::Random => write!(f, "Random"),
            Self::Mixed => write!(f, "Mixed"),
        }
    }
}

/// Generates a new chunk of memory populated with the requested scenario pattern.
///
/// # Arguments
/// * `scenario` - The `StressScenario` to use.
/// * `current_target` - The current target percentage.
/// * `num_bytes` - The number of bytes to allocate.
pub fn generate_chunk(scenario: StressScenario, current_target: u64, num_bytes: usize) -> Vec<u8> {
    let mut slice = vec![0u8; num_bytes];
    match scenario {
        StressScenario::Sequential => {
            for i in (0..num_bytes).step_by(4096) {
                let base = (current_target as u8).wrapping_add((i & 0xFF) as u8);
                for offset in (0..4096).step_by(128) {
                    slice[i + offset] = base.wrapping_add((offset as u8) ^ 0xA5);
                }
            }
        }
        StressScenario::Random => {
            for i in (0..num_bytes).step_by(4096) {
                let base = (current_target as u8).wrapping_add((i & 0xFF) as u8);
                for offset in (0..4096).step_by(128) {
                    let rand_val = (base as u32)
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223)
                        .wrapping_add(offset as u32 ^ 0x3C);
                    slice[i + offset] = (rand_val & 0xFF) as u8;
                }
            }
        }
        StressScenario::Mixed => {
            for i in (0..num_bytes).step_by(4096) {
                let base = (current_target as u8).wrapping_add((i & 0xFF) as u8);
                for offset in (0..4096).step_by(128) {
                    if (i / 4096) % 2 == 0 {
                        slice[i + offset] = base.wrapping_add((offset as u8) ^ 0xA5);
                    } else {
                        let rand_val = (base as u32)
                            .wrapping_mul(1664525)
                            .wrapping_add(1013904223)
                            .wrapping_add(offset as u32 ^ 0x3C);
                        slice[i + offset] = (rand_val & 0xFF) as u8;
                    }
                }
            }
        }
    }
    slice
}

/// In-place modification of a chunk of memory with the requested scenario pattern.
///
/// # Arguments
/// * `scenario` - The `StressScenario` to use.
/// * `target_chunk` - The memory slice to mutate.
/// * `cycle` - The current cycle number.
pub fn modify_chunk(scenario: StressScenario, target_chunk: &mut [u8], cycle: usize) {
    let chunk_len = target_chunk.len();
    let limit = chunk_len.min(16 * 1024 * 1024);
    match scenario {
        StressScenario::Sequential => {
            for offset in (0..limit).step_by(16384) {
                target_chunk[offset] = (cycle as u8).wrapping_add((offset & 0xFF) as u8);
            }
        }
        StressScenario::Random => {
            for offset in (0..limit).step_by(16384) {
                let rand_val = (cycle as u32)
                    .wrapping_mul(1664525)
                    .wrapping_add(1013904223)
                    .wrapping_add((offset & 0xFF) as u32);
                target_chunk[offset] = (rand_val & 0xFF) as u8;
            }
        }
        StressScenario::Mixed => {
            for offset in (0..limit).step_by(16384) {
                if (offset / 16384) % 2 == 0 {
                    target_chunk[offset] = (cycle as u8).wrapping_add((offset & 0xFF) as u8);
                } else {
                    let rand_val = (cycle as u32)
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223)
                        .wrapping_add((offset & 0xFF) as u32);
                    target_chunk[offset] = (rand_val & 0xFF) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scenario_correctly() {
        assert_eq!(
            "sequential"
                .parse::<StressScenario>()
                .unwrap_or_else(|_| panic!("Failed to parse scenario")),
            StressScenario::Sequential
        );
        assert_eq!(
            "RANDOM"
                .parse::<StressScenario>()
                .unwrap_or_else(|_| panic!("Failed to parse scenario")),
            StressScenario::Random
        );
        assert_eq!(
            "MiXeD"
                .parse::<StressScenario>()
                .unwrap_or_else(|_| panic!("Failed to parse scenario")),
            StressScenario::Mixed
        );
        assert!("unknown".parse::<StressScenario>().is_err());
    }

    #[test]
    fn generates_chunks_safely() {
        let seq = generate_chunk(StressScenario::Sequential, 10, 8192);
        assert_eq!(seq.len(), 8192);
        let rnd = generate_chunk(StressScenario::Random, 10, 8192);
        assert_eq!(rnd.len(), 8192);
        let mix = generate_chunk(StressScenario::Mixed, 10, 8192);
        assert_eq!(mix.len(), 8192);
    }

    #[test]
    fn modifies_chunks_safely() {
        let mut seq = vec![0u8; 8192];
        modify_chunk(StressScenario::Sequential, &mut seq, 5);

        let mut rnd = vec![0u8; 8192];
        modify_chunk(StressScenario::Random, &mut rnd, 5);

        let mut mix = vec![0u8; 8192];
        modify_chunk(StressScenario::Mixed, &mut mix, 5);
    }
}
