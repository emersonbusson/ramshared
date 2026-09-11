
use std::path::Path;
use std::time::Duration;

pub fn parse_nvidia_smi_free_bytes(output: &str) -> Option<u64> {
    let first = output.lines().find(|line| !line.trim().is_empty())?.trim();
    let token = first
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .find(|part| !part.is_empty())?;
    let mib = token.parse::<u64>().ok()?;
    mib.checked_mul(1024 * 1024)
}

pub fn global_gpu_free_bytes_with<A, R>(
    programs: &[&str],
    timeout: Duration,
    mut available: A,
    mut run: R,
) -> Option<u64>
where
    A: FnMut(&str) -> bool,
    R: FnMut(&str, &[&str], Duration) -> Option<String>,
{
    const ARGS: &[&str] = &["--query-gpu=memory.free", "--format=csv,noheader,nounits"];
    for program in programs {
        if !available(program) {
            continue;
        }
        if let Some(output) = run(program, ARGS, timeout)
            && let Some(bytes) = parse_nvidia_smi_free_bytes(&output)
        {
            return Some(bytes);
        }
    }
    None
}

pub fn global_gpu_free_bytes_from_nvidia_smi(timeout: Duration) -> Option<u64> {
    global_gpu_free_bytes_with(
        &["/usr/lib/wsl/lib/nvidia-smi", "nvidia-smi"],
        timeout,
        |program| !program.starts_with('/') || Path::new(program).exists(),
        crate::command_stdout_with_timeout,
    )
}

pub fn observe_global_free_floor(
    free_bytes: Option<u64>,
    floor_bytes: u64,
    committed_bytes: u64,
    streak: &mut u32,
    required: u32,
) -> bool {
    if committed_bytes == 0 {
        *streak = 0;
        return false;
    }
    if free_bytes.is_some_and(|free| free < floor_bytes) {
        *streak = streak.saturating_add(1);
        *streak >= required.max(1)
    } else {
        *streak = 0;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nvidia_smi_free_bytes_parses_valid() {
        assert_eq!(parse_nvidia_smi_free_bytes("4731\n"), Some(4731 * 1024 * 1024));
        assert_eq!(parse_nvidia_smi_free_bytes(" 222 MiB \n"), Some(222 * 1024 * 1024));
        assert_eq!(parse_nvidia_smi_free_bytes("222, 5733\n"), Some(222 * 1024 * 1024));
    }

    #[test]
    fn parse_nvidia_smi_free_bytes_returns_none_invalid() {
        assert_eq!(parse_nvidia_smi_free_bytes(""), None);
        assert_eq!(parse_nvidia_smi_free_bytes("N/A\n"), None);
    }

    #[test]
    fn global_free_floor_demote_requires_committed_tier_and_streak() {
        let mut streak = 0;
        assert!(!observe_global_free_floor(
            Some(128),
            512,
            0,
            &mut streak,
            3
        ));
        assert_eq!(streak, 0);

        assert!(!observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
        assert_eq!(streak, 1);
        assert!(!observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
        assert!(observe_global_free_floor(
            Some(128),
            512,
            1024,
            &mut streak,
            3
        ));
    }

    #[test]
    fn global_free_floor_resets_on_healthy_or_missing_sample() {
        let mut streak = 2;
        assert!(!observe_global_free_floor(
            Some(2048),
            512,
            1024,
            &mut streak,
            3
        ));
        assert_eq!(streak, 0);
        streak = 2;
        assert!(!observe_global_free_floor(None, 512, 1024, &mut streak, 3));
        assert_eq!(streak, 0);
    }
}
