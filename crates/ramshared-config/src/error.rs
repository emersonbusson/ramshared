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
                    write!(f, "parse error at line {}, col {}: {}", l, c, message)
                } else {
                    write!(
                        f,
                        "parse error at line {}, col {} for key '{}': {}",
                        l, c, key_path, message
                    )
                }
            }
            Self::Parse {
                message, key_path, ..
            } => {
                if key_path.is_empty() {
                    write!(f, "parse error: {}", message)
                } else {
                    write!(f, "parse error for key '{}': {}", key_path, message)
                }
            }
            Self::Invalid { key_path, reason } => {
                write!(f, "invalid configuration at '{}': {}", key_path, reason)
            }
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Self::OutOfRange(msg) => write!(f, "out of range: {msg}"),
            Self::UnsupportedBackend(msg) => write!(f, "unsupported backend: {msg}"),
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
}
