use core::fmt;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

pub const AUTH_TIMEOUT_SECS: u64 = 5; // Replay attack timeout window (5s).

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub magic: u64,
    pub last_timestamp: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[derive(Debug)]
pub enum HandshakeAuthError {
    Io(io::Error),
    InvalidMagic,
    TimestampExpired,
}

impl From<io::Error> for HandshakeAuthError {
    fn from(e: io::Error) -> Self {
        HandshakeAuthError::Io(e)
    }
}

impl fmt::Display for HandshakeAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeAuthError::Io(e) => write!(f, "auth I/O error: {}", e),
            HandshakeAuthError::InvalidMagic => f.write_str("invalid authentication magic nonce"),
            HandshakeAuthError::TimestampExpired => f.write_str("authentication timestamp expired (replay protection)"),
        }
    }
}

impl std::error::Error for HandshakeAuthError {}

pub struct HandshakeAuth;

impl HandshakeAuth {
    pub fn verify<R: Read>(r: &mut R, ctx: &AuthContext) -> Result<(), HandshakeAuthError> {
        let mut magic_bytes = [0u8; 8];
        r.read_exact(&mut magic_bytes)?;
        let client_magic = u64::from_be_bytes(magic_bytes);

        if client_magic != ctx.magic {
            return Err(HandshakeAuthError::InvalidMagic);
        }

        let mut ts_bytes = [0u8; 8];
        r.read_exact(&mut ts_bytes)?;
        let client_ts = u64::from_be_bytes(ts_bytes);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if client_ts.abs_diff(now) > AUTH_TIMEOUT_SECS {
            return Err(HandshakeAuthError::TimestampExpired);
        }

        // Monotonic validation to prevent replay within the 5s window
        let mut current_last = ctx.last_timestamp.load(std::sync::atomic::Ordering::Acquire);
        loop {
            if client_ts <= current_last {
                return Err(HandshakeAuthError::TimestampExpired);
            }
            match ctx.last_timestamp.compare_exchange_weak(
                current_last,
                client_ts,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(x) => current_last = x,
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_valid_auth() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x1234567890ABCDEF_u64.to_be_bytes());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        data.extend_from_slice(&now.to_be_bytes());

        let mut r = Cursor::new(&data);
        let ctx = AuthContext { magic: 0x1234567890ABCDEF, last_timestamp: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)) };
        assert!(HandshakeAuth::verify(&mut r, &ctx).is_ok());

        // Replay should fail
        let mut r = Cursor::new(&data);
        assert!(matches!(HandshakeAuth::verify(&mut r, &ctx), Err(HandshakeAuthError::TimestampExpired)));
    }

    #[test]
    fn test_valid_auth_old() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x1234567890ABCDEF_u64.to_be_bytes());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        data.extend_from_slice(&now.to_be_bytes());

        let mut r = Cursor::new(data);
        let ctx = AuthContext { magic: 0x1234567890ABCDEF, last_timestamp: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)) };
        assert!(HandshakeAuth::verify(&mut r, &ctx).is_ok());
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = Vec::new();
        data.extend_from_slice(&0xBADBADBADBADBAD_u64.to_be_bytes());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        data.extend_from_slice(&now.to_be_bytes());

        let mut r = Cursor::new(data);
        let ctx = AuthContext { magic: 0x1234567890ABCDEF, last_timestamp: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)) };
        assert!(matches!(HandshakeAuth::verify(&mut r, &ctx), Err(HandshakeAuthError::InvalidMagic)));
    }

    #[test]
    fn test_timestamp_expired() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x1234567890ABCDEF_u64.to_be_bytes());
        let past = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() - AUTH_TIMEOUT_SECS - 1;
        data.extend_from_slice(&past.to_be_bytes());

        let mut r = Cursor::new(data);
        let ctx = AuthContext { magic: 0x1234567890ABCDEF, last_timestamp: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)) };
        assert!(matches!(HandshakeAuth::verify(&mut r, &ctx), Err(HandshakeAuthError::TimestampExpired)));
    }
}
