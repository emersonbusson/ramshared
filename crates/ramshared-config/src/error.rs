use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
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
            } => {
                write!(f, "parse error at line {}, col {}: {}", l, c, message)
            }
            Self::Parse { message, .. } => {
                write!(f, "parse error: {}", message)
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
