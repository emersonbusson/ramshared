use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
        key_path: String,
    },
    Invalid {
        key_path: String,
        reason: String,
    },
    InvalidInput(String),
    OutOfRange(String),
    UnsupportedBackend(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                message,
                line: Some(l),
                column: Some(c),
                key_path,
            } => {
                if key_path.is_empty() {
                    write!(f, "parse error at line {}, col {}: {}", l, c, crate::redact::redact(message))
                } else {
                    write!(
                        f,
                        "parse error at line {}, col {} for key '{}': {}",
                        l, c, crate::redact::redact(key_path), crate::redact::redact(message)
                    )
                }
            }
            Self::Parse {
                message, key_path, ..
            } => {
                if key_path.is_empty() {
                    write!(f, "parse error: {}", crate::redact::redact(message))
                } else {
                    write!(f, "parse error for key '{}': {}", crate::redact::redact(key_path), crate::redact::redact(message))
                }
            }
            Self::Invalid { key_path, reason } => {
                write!(f, "invalid configuration at '{}': {}", crate::redact::redact(key_path), crate::redact::redact(reason))
            }
            Self::InvalidInput(msg) => write!(f, "invalid input: {}", crate::redact::redact(msg)),
            Self::OutOfRange(msg) => write!(f, "out of range: {}", crate::redact::redact(msg)),
            Self::UnsupportedBackend(msg) => write!(f, "unsupported backend: {}", crate::redact::redact(msg)),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_all_variants() {
        let e1 = ConfigError::Parse {
            message: "bad token".into(),
            line: Some(10),
            column: Some(5),
            key_path: "broker.listen".into(),
        };
        assert_eq!(
            e1.to_string(),
            "parse error at line 10, col 5 for key 'broker.listen': bad token"
        );

        let e2 = ConfigError::Parse {
            message: "unexpected eof".into(),
            line: None,
            column: None,
            key_path: "".into(),
        };
        assert_eq!(e2.to_string(), "parse error: unexpected eof");

        let e3 = ConfigError::Invalid {
            key_path: "daemon.port".into(),
            reason: "must be positive".into(),
        };
        assert_eq!(
            e3.to_string(),
            "invalid configuration at 'daemon.port': must be positive"
        );

        let e4 = ConfigError::InvalidInput("empty file".into());
        assert_eq!(e4.to_string(), "invalid input: empty file");

        let e5 = ConfigError::OutOfRange("size too large".into());
        assert_eq!(e5.to_string(), "out of range: size too large");

        let e6 = ConfigError::UnsupportedBackend("directx".into());
        assert_eq!(e6.to_string(), "unsupported backend: directx");
    }

    #[test]
    fn display_redacts_sensitive_data() {
        let e1 = ConfigError::InvalidInput("file /etc/shadow not found".into());
        assert_eq!(e1.to_string(), "invalid input: file <REDACTED> not found");

        let e2 = ConfigError::OutOfRange("address 0xdeadbeef too high".into());
        assert_eq!(e2.to_string(), "out of range: address <REDACTED> too high");

        let e3 = ConfigError::Invalid {
            key_path: "broker.device".into(),
            reason: "nvme0n1 is full".into(),
        };
        assert_eq!(e3.to_string(), "invalid configuration at 'broker.device': <REDACTED> is full");
    }
}
