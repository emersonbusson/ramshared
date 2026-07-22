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

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.broker.slices == 0 {
            return Err("broker.slices must be > 0".into());
        }
        if self.broker.slice_mib == 0 {
            return Err("broker.slice_mib must be > 0".into());
        }
        if self.agent.watchdog_secs == 0 {
            return Err("agent.watchdog_secs must be > 0".into());
        }
        match self.broker.backend.as_str() {
            "cuda" | "vulkan" => Ok(()),
            other => Err(format!("unsupported backend: {other}")),
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
}
