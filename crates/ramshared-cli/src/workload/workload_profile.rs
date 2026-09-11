use serde::{Deserialize, Serialize};

pub const GIB_BYTES: u64 = 1024 * 1024 * 1024;
pub const MIB_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkloadBudget {
    pub control_plane_reserve_bytes: u64,
    pub memory_high_bytes: u64,
    pub memory_max_bytes: u64,
}

pub fn calculate_budget(
    memory_total_bytes: u64,
    _memory_available_bytes: u64,
) -> Result<WorkloadBudget, String> {
    if memory_total_bytes == 0 {
        return Err("invalid memory snapshot".into());
    }
    let reserve = (memory_total_bytes.div_ceil(4)).max(4 * GIB_BYTES);
    let memory_max = memory_total_bytes.saturating_sub(reserve);
    if memory_max < GIB_BYTES {
        return Err(format!(
            "less than 1 GiB remains after the {} MiB control-plane reserve",
            reserve >> 20
        ));
    }
    let high_gap = (memory_total_bytes.div_ceil(10)).max(GIB_BYTES);
    Ok(WorkloadBudget {
        control_plane_reserve_bytes: reserve,
        memory_high_bytes: memory_max.saturating_sub(high_gap),
        memory_max_bytes: memory_max,
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkloadClass {
    Interactive,
    Build,
    BrowserTest,
    Batch,
}

impl WorkloadClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "interactive" => Some(Self::Interactive),
            "build" => Some(Self::Build),
            "browser-test" => Some(Self::BrowserTest),
            "batch" => Some(Self::Batch),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Build => "build",
            Self::BrowserTest => "browser-test",
            Self::Batch => "batch",
        }
    }

    fn default_memory_mib(self) -> u64 {
        match self {
            Self::Interactive => 2 * 1024,
            Self::Build => 6 * 1024,
            Self::BrowserTest => 4 * 1024,
            Self::Batch => 8 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadRequest {
    pub class: WorkloadClass,
    pub memory_max_bytes: u64,
    pub command: Vec<String>,
}

pub fn parse_run_args(args: &[String]) -> Result<WorkloadRequest, String> {
    let mut class = None;
    let mut requested_mib = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--class" => {
                index += 1;
                class = Some(
                    args.get(index)
                        .and_then(|value| WorkloadClass::parse(value))
                        .ok_or("--class must be interactive|build|browser-test|batch")?,
                );
            }
            "--memory-max" => {
                index += 1;
                requested_mib = Some(
                    args.get(index)
                        .ok_or("--memory-max requires MiB")?
                        .parse::<u64>()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or("--memory-max must be a positive MiB value")?,
                );
            }
            "--" => {
                let class = class.ok_or("--class is required")?;
                let command = args[index + 1..].to_vec();
                if command.is_empty() {
                    return Err("a command is required after --".into());
                }
                let memory_mib = requested_mib.unwrap_or_else(|| class.default_memory_mib());
                return Ok(WorkloadRequest {
                    class,
                    memory_max_bytes: memory_mib
                        .checked_mul(MIB_BYTES)
                        .ok_or("--memory-max overflow")?,
                    command,
                });
            }
            _ => {
                return Err(
                    "usage: ramshared run --class <class> [--memory-max MiB] -- <command>".into(),
                );
            }
        }
        index += 1;
    }
    Err("usage: ramshared run --class <class> [--memory-max MiB] -- <command>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_profile_parsers_cover_legitimate_and_refusal_paths() {
        assert!(calculate_budget(0, 0).is_err());
        assert!(calculate_budget(4 * GIB_BYTES, 0).is_err());

        for (class, expected_mib) in [
            ("interactive", 2048),
            ("build", 6144),
            ("browser-test", 4096),
            ("batch", 8192),
        ] {
            let request = parse_run_args(&[
                "--class".into(),
                class.into(),
                "--".into(),
                "/bin/true".into(),
            ])
            .unwrap_or_else(|_| panic!("Failed to parse"));
            assert_eq!(request.memory_max_bytes, expected_mib * MIB_BYTES);
            assert_eq!(request.class.as_str(), class);
        }
        for args in [
            vec![],
            vec!["--class".into(), "unknown".into()],
            vec!["--memory-max".into(), "0".into()],
            vec!["--class".into(), "build".into(), "--".into()],
            vec!["--unexpected".into()],
        ] {
            assert!(parse_run_args(&args).is_err());
        }
    }
}
