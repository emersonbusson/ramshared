//! Shared TOML configuration for broker and agents.
//!
//! Parsing is deliberately separate from process startup so CLI overrides and
//! validation can be tested without sockets, root, or a GPU.
#![forbid(unsafe_code)]

pub mod error;
pub use error::ConfigError;

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
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|err| {
            let message = err.message().to_string();
            let mut line = None;
            let mut column = None;
            if let Some(span) = err.span() {
                let mut l = 1;
                let mut c = 1;
                for (i, ch) in text.chars().enumerate() {
                    if i == span.start {
                        line = Some(l);
                        column = Some(c);
                        break;
                    }
                    if ch == '\n' {
                        l += 1;
                        c = 1;
                    } else {
                        c += 1;
                    }
                }
            }
            ConfigError::Parse {
                message,
                line,
                column,
            }
        })
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.broker.slices == 0 {
            return Err(ConfigError::Invalid {
                key_path: "broker.slices".into(),
                reason: "must be > 0".into(),
            });
        }
        if self.broker.slice_mib == 0 {
            return Err(ConfigError::Invalid {
                key_path: "broker.slice_mib".into(),
                reason: "must be > 0".into(),
            });
        }
        if self.agent.watchdog_secs == 0 {
            return Err(ConfigError::Invalid {
                key_path: "agent.watchdog_secs".into(),
                reason: "must be > 0".into(),
            });
        }
        match self.broker.backend.as_str() {
            "cuda" | "vulkan" => Ok(()),
            other => Err(ConfigError::Invalid {
                key_path: "broker.backend".into(),
                reason: format!("unsupported backend: {other}"),
            }),
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
        let err = cfg.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                ref key_path,
                ref reason,
            } if key_path == "broker.backend" && reason == "unsupported backend: unknown"
        ));
    }

    #[test]
    fn parses_error_captures_location() {
        let err = Config::parse("[broker]\nslice_mib = 'hello'").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Parse {
                line: Some(2),
                column: Some(13),
                ..
            }
        ));
    }
}
