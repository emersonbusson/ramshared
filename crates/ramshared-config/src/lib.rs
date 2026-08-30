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
        if self.broker.slice_mib < 16 {
            return Err(ConfigError::OutOfRange(format!(
                "broker.slice_mib must be >= 16 (got {})",
                self.broker.slice_mib
            )));
        }
        if self.agent.watchdog_secs == 0 {
            return Err(ConfigError::Invalid {
                key_path: "agent.watchdog_secs".into(),
                reason: "must be > 0".into(),
            });
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
        let err = cfg.validate().expect_err("expected error");
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
        let err = Config::parse("[broker]\nslice_mib = 'hello'").expect_err("expected error");
        assert!(matches!(
            err,
            ConfigError::Parse {
                line: Some(2),
                column: Some(13),
                ..
            }
        ));
    }

    #[test]
    fn display_errors() {
        let p1 = ConfigError::Parse {
            message: "msg".into(),
            line: Some(1),
            column: Some(2),
        };
        assert_eq!(p1.to_string(), "parse error at line 1, col 2: msg");

        let p2 = ConfigError::Parse {
            message: "msg".into(),
            line: None,
            column: None,
        };
        assert_eq!(p2.to_string(), "parse error: msg");

        let i = ConfigError::Invalid {
            key_path: "k".into(),
            reason: "r".into(),
        };
        assert_eq!(i.to_string(), "invalid configuration at 'k': r");
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
        cfg.broker.slices = 1000;
        cfg.broker.slice_mib = 1024 * 1024 * 1024; // 1 PB slice
        if std::fs::read_to_string("/proc/meminfo").is_ok() {
            let err = cfg.validate().expect_err("should reject excessive ram");
            assert!(matches!(err, ConfigError::OutOfRange(_)));
        }
    }
}
