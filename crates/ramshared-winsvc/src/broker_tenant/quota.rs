/// Quota tracking for a single broker tenant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TenantQuota {
    /// Maximum allowed memory limit in bytes.
    pub limit: u64,
}

impl TenantQuota {
    /// Create a new quota with the specified limit.
    pub fn new(limit: u64) -> Self {
        Self { limit }
    }

    /// Enforce the quota against a requested byte count.
    pub fn enforce(&self, requested: u64) -> Result<(), u64> {
        if requested > self.limit {
            Err(self.limit)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_quota() {
        let q = TenantQuota::new(1024);
        assert_eq!(q.enforce(512), Ok(()));
        assert_eq!(q.enforce(1024), Ok(()));
        assert_eq!(q.enforce(2048), Err(1024));
    }
}
