use std::fs;
use std::process::Command;

const GIB_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkloadBudget {
    pub control_plane_reserve_bytes: u64,
    pub memory_high_bytes: u64,
    pub memory_max_bytes: u64,
}

pub fn calculate_budget(
    memory_total_bytes: u64,
    memory_available_bytes: u64,
) -> Result<WorkloadBudget, String> {
    if memory_total_bytes == 0 || memory_available_bytes > memory_total_bytes {
        return Err("invalid memory snapshot".into());
    }
    let reserve = (memory_total_bytes.div_ceil(5)).max(2 * GIB_BYTES);
    let available_after_reserve = memory_available_bytes.saturating_sub(reserve);
    let distro_cap = memory_total_bytes.saturating_mul(3) / 4;
    let memory_max = available_after_reserve.min(distro_cap);
    if memory_max < GIB_BYTES {
        return Err(format!(
            "less than 1 GiB remains after the {} MiB control-plane reserve",
            reserve >> 20
        ));
    }
    Ok(WorkloadBudget {
        control_plane_reserve_bytes: reserve,
        memory_high_bytes: memory_max.saturating_mul(9) / 10,
        memory_max_bytes: memory_max,
    })
}

pub fn parse_run_args(args: &[String]) -> Result<Vec<String>, String> {
    match args {
        [profile_flag, profile, boundary, command @ ..]
            if profile_flag == "--profile"
                && profile == "safe"
                && boundary == "--"
                && !command.is_empty() =>
        {
            Ok(command.to_vec())
        }
        _ => Err("usage: ramshared run --profile safe -- <command> [args...]".into()),
    }
}

fn parse_meminfo(text: &str) -> Result<(u64, u64), String> {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (name, rest) = line.split_once(':')?;
            (name == key)
                .then(|| rest.split_whitespace().next()?.parse::<u64>().ok())
                .flatten()
        })
    };
    let total_kib = value("MemTotal").ok_or("MemTotal missing from /proc/meminfo")?;
    let available_kib = value("MemAvailable").ok_or("MemAvailable missing from /proc/meminfo")?;
    Ok((
        total_kib.saturating_mul(1024),
        available_kib.saturating_mul(1024),
    ))
}

fn build_runner_args(unit: String, budget: WorkloadBudget) -> Vec<String> {
    let memory_high = format!("MemoryHigh={}", budget.memory_high_bytes);
    let memory_max = format!("MemoryMax={}", budget.memory_max_bytes);
    vec![
        "--scope".to_string(),
        "--collect".to_string(),
        "--quiet".to_string(),
        "--unit".to_string(),
        unit,
        "--property".to_string(),
        "Slice=ramshared-workloads.slice".to_string(),
        "--property".to_string(),
        memory_high,
        "--property".to_string(),
        memory_max,
        "--property".to_string(),
        "TasksMax=4096".to_string(),
        "--".to_string(),
    ]
}

trait ScopeRunner {
    fn run(&self, runner_args: &[String], command: &[String]) -> Result<Option<i32>, String>;
}

struct SystemScopeRunner;

impl ScopeRunner for SystemScopeRunner {
    fn run(&self, runner_args: &[String], command: &[String]) -> Result<Option<i32>, String> {
        Command::new("systemd-run")
            .args(runner_args)
            .args(command)
            .status()
            .map(|status| status.code())
            .map_err(|error| format!("failed to start managed systemd scope: {error}"))
    }
}

fn run_with_meminfo(
    args: &[String],
    meminfo: &str,
    runner: &dyn ScopeRunner,
) -> Result<(), String> {
    let command = parse_run_args(args)?;
    let (total, available) = parse_meminfo(meminfo)?;
    let budget = calculate_budget(total, available)?;
    let unit = format!("ramshared-workload-{}", std::process::id());
    let runner_args = build_runner_args(unit, budget);
    match runner.run(&runner_args, &command)? {
        Some(0) => Ok(()),
        Some(code) => Err(format!("managed workload exited with status {code}")),
        None => Err("managed workload exited with a signal".into()),
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let meminfo = fs::read_to_string("/proc/meminfo").map_err(|error| error.to_string())?;
    run_with_meminfo(args, &meminfo, &SystemScopeRunner)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    struct FakeRunner {
        code: Option<i32>,
    }

    impl ScopeRunner for FakeRunner {
        fn run(&self, runner_args: &[String], command: &[String]) -> Result<Option<i32>, String> {
            assert!(runner_args.iter().any(|arg| arg == "MemoryMax=4294967296"));
            assert_eq!(command, ["/bin/true"]);
            Ok(self.code)
        }
    }

    #[test]
    fn managed_scope_preserves_control_plane_reserve() {
        let budget = calculate_budget(10 * GIB, 8 * GIB).expect("safe budget");
        assert_eq!(budget.control_plane_reserve_bytes, 2 * GIB);
        assert_eq!(budget.memory_max_bytes, 6 * GIB);
        assert_eq!(budget.memory_high_bytes, 6 * GIB * 9 / 10);
    }

    #[test]
    fn managed_scope_refuses_when_less_than_one_gib_remains() {
        let error = calculate_budget(10 * GIB, 2 * GIB + GIB / 2).unwrap_err();
        assert!(error.contains("less than 1 GiB"), "{error}");
    }

    #[test]
    fn safe_profile_requires_an_explicit_command_boundary() {
        let command = parse_run_args(&[
            "--profile".into(),
            "safe".into(),
            "--".into(),
            "make".into(),
            "test".into(),
        ])
        .expect("valid managed command");
        assert_eq!(command, vec!["make", "test"]);
        assert!(parse_run_args(&["make".into(), "test".into()]).is_err());
        assert!(
            parse_run_args(&[
                "--profile".into(),
                "unsafe".into(),
                "--".into(),
                "make".into()
            ])
            .is_err()
        );
    }

    #[test]
    fn managed_scope_never_combines_scope_with_wait() {
        let budget = calculate_budget(10 * GIB, 8 * GIB).expect("safe budget");
        let args = build_runner_args("ramshared-workload-test".into(), budget);
        assert!(args.iter().any(|argument| argument == "--scope"));
        assert!(!args.iter().any(|argument| argument == "--wait"));
    }

    #[test]
    fn managed_scope_propagates_success_exit_and_signal() {
        let args = [
            "--profile".into(),
            "safe".into(),
            "--".into(),
            "/bin/true".into(),
        ];
        let meminfo = "MemTotal: 8388608 kB\nMemAvailable: 6291456 kB\n";
        assert!(run_with_meminfo(&args, meminfo, &FakeRunner { code: Some(0) }).is_ok());
        let status = run_with_meminfo(&args, meminfo, &FakeRunner { code: Some(7) })
            .expect_err("nonzero workload must fail");
        assert!(status.contains("status 7"), "{status}");
        let signal = run_with_meminfo(&args, meminfo, &FakeRunner { code: None })
            .expect_err("signal-terminated workload must fail");
        assert!(signal.contains("signal"), "{signal}");
    }

    #[test]
    fn managed_scope_refuses_incomplete_meminfo() {
        let error = parse_meminfo("MemTotal: 1024 kB\n").expect_err("missing available memory");
        assert!(error.contains("MemAvailable"), "{error}");
    }
}
