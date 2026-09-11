//! Agent authentication token rotation and expiry enforcement.

use std::time::{Duration, Instant};

/// Authentication token for an agent.
#[derive(Debug, Clone)]
pub struct AuthToken {
    /// The actual token string.
    token: String,
    /// When the token expires.
    expires_at: Instant,
    /// When the token was issued.
    issued_at: Instant,
}

impl AuthToken {
    /// Creates a new AuthToken.
    pub fn new(token: String, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            token,
            expires_at: now + ttl,
            issued_at: now,
        }
    }

    /// Checks if the token is valid (not expired, or within a grace period).
    pub fn is_valid(&self, grace_period: Duration) -> bool {
        let now = Instant::now();
        now < self.expires_at + grace_period
    }

    /// Checks if the token needs rotation.
    pub fn needs_rotation(&self, rotation_interval: Duration) -> bool {
        let now = Instant::now();
        now >= self.issued_at + rotation_interval
    }

    /// Gets the token string.
    pub fn as_str(&self) -> &str {
        &self.token
    }
}

/// Token rotation manager for an agent.
pub struct TokenManager {
    current_token: Option<AuthToken>,
    ttl: Duration,
    rotation_interval: Duration,
    grace_period: Duration,
}

impl TokenManager {
    /// Creates a new TokenManager.
    pub fn new(ttl: Duration, rotation_interval: Duration, grace_period: Duration) -> Self {
        Self {
            current_token: None,
            ttl,
            rotation_interval,
            grace_period,
        }
    }

    /// Sets the current token.
    pub fn set_token(&mut self, token: String) {
        self.current_token = Some(AuthToken::new(token, self.ttl));
    }

    /// Gets the current token if valid.
    pub fn get_valid_token(&self) -> Option<&str> {
        self.current_token.as_ref().and_then(|t| {
            if t.is_valid(self.grace_period) {
                Some(t.as_str())
            } else {
                None
            }
        })
    }

    /// Checks if the token needs rotation.
    pub fn needs_rotation(&self) -> bool {
        self.current_token.as_ref().is_none_or(|t| {
            t.needs_rotation(self.rotation_interval)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_validity() {
        let token = AuthToken::new("test_token".to_string(), Duration::from_secs(1));
        assert!(token.is_valid(Duration::from_secs(0)));
        // Assuming test runs instantly, token shouldn't expire
    }

    #[test]
    fn test_manager() {
        let mut manager = TokenManager::new(
            Duration::from_secs(10),
            Duration::from_secs(5),
            Duration::from_secs(2),
        );
        assert!(manager.needs_rotation());
        assert_eq!(manager.get_valid_token(), None);

        manager.set_token("new_token".to_string());
        assert!(!manager.needs_rotation());
        assert_eq!(manager.get_valid_token(), Some("new_token"));
    }
}
