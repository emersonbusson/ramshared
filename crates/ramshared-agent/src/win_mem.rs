//! Windows memory-pressure sampling boundary.
//!
//! The parser is platform independent and tested here. The runtime sampler
//! uses PowerShell/CIM as a dependency-free fallback; a future native service
//! can replace only `sample()` without changing the broker contract.
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemorySample {
    pub available_bytes: u64,
    pub commit_limit_bytes: u64,
}

pub fn parse_cim_sample(text: &str) -> Option<MemorySample> {
    let mut available_kib = None;
    let mut limit_kib = None;
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        let value = value.trim().parse::<u64>().ok()?;
        match key.trim() {
            "FreePhysicalMemory" => available_kib = Some(value),
            "TotalVirtualMemorySize" => limit_kib = Some(value),
            _ => {}
        }
    }
    Some(MemorySample {
        available_bytes: available_kib?.saturating_mul(1024),
        commit_limit_bytes: limit_kib?.saturating_mul(1024),
    })
}

#[cfg(windows)]
pub fn sample() -> Option<MemorySample> {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "Get-CimInstance Win32_OperatingSystem | ForEach-Object { \"FreePhysicalMemory=$($_.FreePhysicalMemory)\"; \"TotalVirtualMemorySize=$($_.TotalVirtualMemorySize)\" }"])
        .output().ok()?;
    parse_cim_sample(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
pub fn sample() -> Option<MemorySample> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locale_neutral_cim_output() {
        assert_eq!(
            parse_cim_sample("FreePhysicalMemory=1024\nTotalVirtualMemorySize=8192\n"),
            Some(MemorySample {
                available_bytes: 1024 * 1024,
                commit_limit_bytes: 8192 * 1024
            })
        );
    }

    #[test]
    fn malformed_output_is_rejected() {
        assert_eq!(parse_cim_sample("FreePhysicalMemory=abc"), None);
    }
}
