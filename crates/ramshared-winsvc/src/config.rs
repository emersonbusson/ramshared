//! WinDrive service configuration (SPEC ITEM-3 / DT-15).
//!
//! Section `[win_drive]` is self-contained until `ramshared-config` lands.

use std::net::SocketAddr;
use std::str::FromStr;

use serde::Deserialize;

/// Configuration for the Windows VRAM pagefile service.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WinDriveConfig {
    pub size_bytes: u64,
    pub block_size: u32,
    pub pagefile_min: u64,
    pub pagefile_max: u64,
    pub priority: i32,
    /// Broker listen address (e.g. `127.0.0.1:7700`).
    pub broker: String,
    pub tenant: String,
    /// Heartbeat interval seconds (default 5 — SPEC H3).
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
}

fn default_heartbeat_secs() -> u64 {
    5
}

/// Configuration parse / validation error.
#[derive(Debug, PartialEq)]
pub enum ConfigError {
    Parse(String),
    Invalid { field: &'static str, detail: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Parse(s) => write!(f, "config parse: {s}"),
            ConfigError::Invalid { field, detail } => {
                write!(f, "config invalid {field}: {detail}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// TOML wrapper with a `[win_drive]` table.
#[derive(Deserialize)]
struct Root {
    win_drive: WinDriveConfig,
}

impl WinDriveConfig {
    /// Parse TOML text containing a `[win_drive]` section.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let root: Root = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        root.win_drive.validate()?;
        Ok(root.win_drive)
    }

    /// Validate invariants before provision.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.block_size != 512 && self.block_size != 4096 {
            return Err(ConfigError::Invalid {
                field: "block_size",
                detail: format!("must be 512 or 4096, got {}", self.block_size),
            });
        }
        if self.size_bytes == 0 || !self.size_bytes.is_multiple_of(self.block_size as u64) {
            return Err(ConfigError::Invalid {
                field: "size_bytes",
                detail: "must be non-zero multiple of block_size".into(),
            });
        }
        if self.pagefile_min == 0 || self.pagefile_min > self.pagefile_max {
            return Err(ConfigError::Invalid {
                field: "pagefile_min",
                detail: "must be > 0 and <= pagefile_max".into(),
            });
        }
        if self.tenant.is_empty() {
            return Err(ConfigError::Invalid {
                field: "tenant",
                detail: "must be non-empty".into(),
            });
        }
        SocketAddr::from_str(&self.broker).map_err(|e| ConfigError::Invalid {
            field: "broker",
            detail: e.to_string(),
        })?;
        Ok(())
    }

    /// Parsed broker socket address.
    pub fn broker_addr(&self) -> Result<SocketAddr, ConfigError> {
        SocketAddr::from_str(&self.broker).map_err(|e| ConfigError::Invalid {
            field: "broker",
            detail: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const GOOD: &str = r#"
[win_drive]
size_bytes = 1073741824
block_size = 4096
pagefile_min = 268435456
pagefile_max = 1073741824
priority = 1
broker = "127.0.0.1:7700"
tenant = "windrive-host"
"#;

    #[test]
    fn parse_good_config() {
        let c = WinDriveConfig::from_toml(GOOD).unwrap();
        assert_eq!(c.size_bytes, 1 << 30);
        assert_eq!(c.block_size, 4096);
        assert_eq!(c.heartbeat_secs, 5);
        assert_eq!(c.tenant, "windrive-host");
        c.broker_addr().unwrap();
    }

    #[test]
    fn reject_bad_block_size() {
        let bad = GOOD.replace("4096", "1024");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "block_size",
                ..
            }
        ));
    }

    #[test]
    fn reject_empty_tenant() {
        let bad = GOOD.replace("windrive-host", "");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "tenant",
                ..
            }
        ));
    }
}
