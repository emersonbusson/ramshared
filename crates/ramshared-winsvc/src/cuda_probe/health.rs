use std::process::Command;

/// Represents the physical health and usage metrics of a CUDA device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuHealth {
    /// The current GPU temperature in degrees Celsius.
    pub temperature_c: u32,
    /// The current compute/rendering utilization as a percentage (0-100).
    pub utilization_percent: u32,
    /// The number of volatile (since last boot) corrected ECC errors, if supported.
    pub ecc_errors: u64,
}

/// Errors that can occur when probing GPU health metrics.
#[derive(Debug)]
pub enum HealthProbeError {
    /// The health monitoring command (e.g., `nvidia-smi`) failed to execute or returned an error status.
    CommandFailed(String),
    /// The output of the health monitoring command could not be parsed.
    ParseError(String),
}

impl std::fmt::Display for HealthProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthProbeError::CommandFailed(s) => write!(f, "command failed: {s}"),
            HealthProbeError::ParseError(s) => write!(f, "parse error: {s}"),
        }
    }
}

impl std::error::Error for HealthProbeError {}

/// Probes the hardware health of the specified GPU ordinal using `nvidia-smi`.
///
/// This provides defense-in-depth monitoring (temperature, utilization, ECC)
/// independently of the primary CUDA offset allocation probe.
pub fn probe_gpu_health(ordinal: i32) -> Result<GpuHealth, HealthProbeError> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=temperature.gpu,utilization.gpu,ecc.errors.corrected.volatile")
        .arg("--format=csv,noheader,nounits")
        .arg("-i")
        .arg(ordinal.to_string())
        .output()
        .map_err(|e| HealthProbeError::CommandFailed(e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(HealthProbeError::CommandFailed(err.into_owned()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_health_output(&stdout)
}

fn parse_health_output(stdout: &str) -> Result<GpuHealth, HealthProbeError> {
    let parts: Vec<&str> = stdout.trim().split(',').map(|s| s.trim()).collect();

    if parts.len() != 3 {
        return Err(HealthProbeError::ParseError(format!("expected 3 parts, got {}", parts.len())));
    }

    let temp = parts[0].parse().map_err(|_| HealthProbeError::ParseError("invalid temperature".into()))?;
    let util = parts[1].parse().map_err(|_| HealthProbeError::ParseError("invalid utilization".into()))?;

    // Some GPUs don't support ECC, they return "[Not Supported]"
    let ecc = if parts[2].contains("Not Supported") {
        0
    } else {
        parts[2].parse().map_err(|_| HealthProbeError::ParseError("invalid ecc errors".into()))?
    };

    Ok(GpuHealth {
        temperature_c: temp,
        utilization_percent: util,
        ecc_errors: ecc,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_parse_health_output_valid() {
        let output = "45, 10, 0";
        let health = parse_health_output(output).unwrap();
        assert_eq!(health.temperature_c, 45);
        assert_eq!(health.utilization_percent, 10);
        assert_eq!(health.ecc_errors, 0);
    }

    #[test]
    fn test_parse_health_output_not_supported_ecc() {
        let output = "60, 99, [Not Supported]";
        let health = parse_health_output(output).unwrap();
        assert_eq!(health.temperature_c, 60);
        assert_eq!(health.utilization_percent, 99);
        assert_eq!(health.ecc_errors, 0);
    }

    #[test]
    fn test_parse_health_output_invalid_temp() {
        let output = "invalid, 10, 0";
        assert!(parse_health_output(output).is_err());
    }

    #[test]
    fn test_parse_health_output_wrong_parts_count() {
        let output = "45, 10";
        assert!(parse_health_output(output).is_err());
    }
}
