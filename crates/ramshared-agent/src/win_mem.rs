//! Windows memory-pressure sampling boundary.
//!
//! The parser is platform independent and tested here. The runtime sampler
//! uses PowerShell/CIM with Base64 encoded command parameters as a
//! dependency-free fallback; a future native service can replace only
//! `sample()` without changing the broker contract.
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

/// UTF-16LE Base64 encoded command for:
/// Get-CimInstance Win32_OperatingSystem | ForEach-Object { "FreePhysicalMemory=$($_.FreePhysicalMemory)"; "TotalVirtualMemorySize=$($_.TotalVirtualMemorySize)" }
#[allow(dead_code)]
const CIM_COMMAND_B64: &str = "RwBlAHQALQBDAGkAbQBJAG4AcwB0AGEAbgBjAGUAIABXAGkAbgAzADIAXwBPAHAAZQByAGEAdABpAG4AZwBTAHkAcwB0AGUAbQAgAHwAIABGAG8AcgBFAGEAYwBoAC0ATwBiAGoAZQBjAHQAIAB7ACAAIgBGAHIAZQBlAFAAaAB5AHMAaQBjAGEAbABNAGUAbQBvAHIAeQA9ACQAKAAkAF8ALgBGAHIAZQBlAFAAaAB5AHMAaQBjAGEAbABNAGUAbQBvAHIAeQApACIAOwAgACIAVABvAHQAYQBsAFYAaQByAHQAdQBhAGwATQBlAG0AbwByAHkAUwBpAHoAZQA9ACQAKAAkAF8ALgBUAG8AdABhAGwAVgBpAHIAdAB1AGEAbABNAGUAbQBvAHIAeQBTAGkAegBlACkAIgAgAH0A";

#[cfg(windows)]
pub fn sample() -> Option<MemorySample> {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            CIM_COMMAND_B64,
        ])
        .output()
        .ok()?;
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

    #[test]
    fn cim_command_b64_decodes_to_expected_powershell_script() {
        let expected = "Get-CimInstance Win32_OperatingSystem | ForEach-Object { \"FreePhysicalMemory=$($_.FreePhysicalMemory)\"; \"TotalVirtualMemorySize=$($_.TotalVirtualMemorySize)\" }";
        let decoded_bytes = base64_decode(CIM_COMMAND_B64);
        let u16_vec: Vec<u16> = decoded_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let decoded_str = String::from_utf16(&u16_vec).expect("valid utf16");
        assert_eq!(decoded_str, expected);
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        let mut table = [255u8; 256];
        for (i, &b) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .enumerate()
        {
            table[b as usize] = i as u8;
        }

        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0;
        for &b in s.as_bytes() {
            if b == b'=' {
                break;
            }
            let val = table[b as usize];
            if val == 255 {
                continue;
            }
            buf = (buf << 6) | (val as u32);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        out
    }

    #[cfg(not(windows))]
    #[test]
    fn sample_fallback_returns_none() {
        assert_eq!(sample(), None);
    }
}
