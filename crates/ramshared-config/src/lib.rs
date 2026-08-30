//! Shared TOML configuration for broker and agents.
//!
//! Parsing is deliberately separate from process startup so CLI overrides and
//! validation can be tested without sockets, root, or a GPU.
#![forbid(unsafe_code)]

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub broker: BrokerConfig,
    #[serde(default)]
    pub agent: AgentConfig,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BrokerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_slices")]
    pub slices: u16,
    #[serde(default = "default_slice_mib")]
    pub slice_mib: u64,
    #[serde(default = "default_backend")]
    pub backend: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    #[serde(default = "default_broker")]
    pub broker: String,
    #[serde(default)]
    pub tenant: String,
    #[serde(default = "default_watchdog_secs")]
    pub watchdog_secs: u64,
}

fn default_listen() -> String {
    "127.0.0.1:7777".into()
}
fn default_broker() -> String {
    "127.0.0.1:7777".into()
}
fn default_slices() -> u16 {
    1
}
fn default_slice_mib() -> u64 {
    256
}
fn default_backend() -> String {
    "cuda".into()
}
fn default_watchdog_secs() -> u64 {
    90
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            slices: default_slices(),
            slice_mib: default_slice_mib(),
            backend: default_backend(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            broker: default_broker(),
            tenant: String::new(),
            watchdog_secs: default_watchdog_secs(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    InvalidInput(String),
    OutOfRange(String),
    UnsupportedBackend(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            ConfigError::OutOfRange(msg) => write!(f, "Out of range: {}", msg),
            ConfigError::UnsupportedBackend(msg) => write!(f, "Unsupported backend: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

fn parse_meminfo(text: &str) -> Option<u64> {
    for line in text.lines() {
        if !line.starts_with("MemTotal:") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let _ = parts.next(); // MemTotal:
        let kb_str = parts.next()?;
        let kb = kb_str.parse::<u64>().ok()?;
        return Some(kb * 1024); // return bytes
    }
    None
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.broker.slices == 0 {
            return Err(ConfigError::InvalidInput(
                "broker.slices must be > 0".into(),
            ));
        }
        if self.broker.slice_mib < 16 {
            return Err(ConfigError::OutOfRange(format!(
                "broker.slice_mib must be >= 16 (got {})",
                self.broker.slice_mib
            )));
        }
        if self.agent.watchdog_secs == 0 {
            return Err(ConfigError::InvalidInput(
                "agent.watchdog_secs must be > 0".into(),
            ));
        }

        let total_mib = (self.broker.slices as u64).saturating_mul(self.broker.slice_mib);
        let total_bytes = total_mib.saturating_mul(1024 * 1024);

        if let Some(host_ram) = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|m| parse_meminfo(&m))
        {
            let limit_exceeded = total_bytes > host_ram;
            if limit_exceeded {
                return Err(ConfigError::OutOfRange(format!(
                    "configured memory ({total_bytes} bytes) exceeds host RAM ({host_ram} bytes)"
                )));
            }
        }

        match self.broker.backend.as_str() {
            "cuda" | "vulkan" => Ok(()),
            other => Err(ConfigError::UnsupportedBackend(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn defaults_are_safe_and_local() {
        let cfg = Config::parse("").expect("parse");
        assert_eq!(cfg.broker.listen, "127.0.0.1:7777");
        assert_eq!(cfg.broker.backend, "cuda");
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn parses_explicit_config() {
        let cfg = Config::parse("[broker]\nlisten='127.0.0.1:8888'\nslices=4\nslice_mib=128\nbackend='vulkan'\n[agent]\ntenant='dcc'\n").expect("parse");
        assert_eq!(cfg.broker.slices, 4);
        assert_eq!(cfg.agent.tenant, "dcc");
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn rejects_unsafe_or_invalid_configuration() {
        let mut cfg = Config::parse("").expect("parse");
        cfg.broker.listen = "0.0.0.0:7777".into();
        cfg.broker.backend = "unknown".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_small_slice() {
        let mut cfg = Config::parse("").expect("parse");
        cfg.broker.slice_mib = 15;
        let err = cfg.validate().expect_err("should reject small slice");
        assert!(matches!(err, ConfigError::OutOfRange(_)));
    }

    #[test]
    fn rejects_excessive_ram() {
        let mut cfg = Config::parse("").expect("parse");
        // this will exceed any reasonable test machine's RAM
        cfg.broker.slices = 1000;
        cfg.broker.slice_mib = 1024 * 1024 * 1024; // 1 PB slice
        if std::fs::read_to_string("/proc/meminfo").is_ok() {
            let err = cfg.validate().expect_err("should reject excessive ram");
            assert!(matches!(err, ConfigError::OutOfRange(_)));
        }
    }
}
