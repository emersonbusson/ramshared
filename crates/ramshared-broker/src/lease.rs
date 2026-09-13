//! Transport-independent logical lease ownership.

use crate::model::TenantId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingLease {
    pub holder: TenantId,
    pub requested_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogicalLease {
    pub id: u32,
    pub holder: TenantId,
    pub bytes: u64,
    #[serde(skip)]
    pub expires_at: Option<std::time::Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseDecision {
    Pending(PendingLease),
    Denied(LeaseDeny),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseDeny {
    ZeroBytes,
    OverCapacity,
    AlreadyHeld,
    WrongHolder,
    WrongLease,
    InsufficientGrant,
    LeaseIdExhausted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaseDisconnect {
    pub cancelled_pending: bool,
    pub released: Option<LogicalLease>,
}

impl LeaseDisconnect {
    pub fn is_none(&self) -> bool {
        !self.cancelled_pending && self.released.is_none()
    }
}

#[derive(Clone, Debug)]
pub struct LeaseBook {
    capacity_bytes: u64,
    next_id: u32,
    pending: Option<PendingLease>,
    active: Option<LogicalLease>,
    lease_duration: Option<std::time::Duration>,
    grace_period: Option<std::time::Duration>,
}

impl LeaseBook {
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            next_id: 1,
            pending: None,
            active: None,
            lease_duration: None,
            grace_period: None,
        }
    }

    pub fn with_expiry(mut self, duration: std::time::Duration, grace: std::time::Duration) -> Self {
        self.lease_duration = Some(duration);
        self.grace_period = Some(grace);
        self
    }

    #[cfg(test)]
    fn with_next_id_for_test(capacity_bytes: u64, next_id: u32) -> Self {
        Self {
            capacity_bytes,
            next_id,
            pending: None,
            active: None,
            lease_duration: None,
            grace_period: None,
        }
    }

    pub fn pending(&self) -> Option<&PendingLease> {
        self.pending.as_ref()
    }

    pub fn active(&self) -> Option<&LogicalLease> {
        self.active.as_ref()
    }

    pub fn begin_request(&mut self, holder: TenantId, bytes: u64) -> LeaseDecision {
        let denied = if bytes == 0 {
            Some(LeaseDeny::ZeroBytes)
        } else if bytes > self.capacity_bytes {
            Some(LeaseDeny::OverCapacity)
        } else if self.pending.is_some() || self.active.is_some() {
            Some(LeaseDeny::AlreadyHeld)
        } else {
            None
        };
        if let Some(reason) = denied {
            return LeaseDecision::Denied(reason);
        }

        let pending = PendingLease {
            holder,
            requested_bytes: bytes,
        };
        self.pending = Some(pending.clone());
        LeaseDecision::Pending(pending)
    }

    pub fn grant_pending(&mut self, granted_bytes: u64) -> Result<LogicalLease, LeaseDeny> {
        self.grant_pending_at(granted_bytes, std::time::Instant::now())
    }

    pub fn grant_pending_at(&mut self, granted_bytes: u64, now: std::time::Instant) -> Result<LogicalLease, LeaseDeny> {
        let pending = self.pending.as_ref().ok_or(LeaseDeny::WrongLease)?;
        if granted_bytes < pending.requested_bytes || granted_bytes > self.capacity_bytes {
            return Err(LeaseDeny::InsufficientGrant);
        }
        let following_id = self
            .next_id
            .checked_add(1)
            .ok_or(LeaseDeny::LeaseIdExhausted)?;
        let lease = LogicalLease {
            id: self.next_id,
            holder: pending.holder,
            bytes: granted_bytes,
            expires_at: self.lease_duration.map(|d| now + d),
        };
        self.next_id = following_id;
        self.pending = None;
        self.active = Some(lease.clone());
        Ok(lease)
    }

    pub fn renew(&mut self, holder: TenantId, lease: u32, now: std::time::Instant) -> Result<bool, LeaseDeny> {
        let Some(active) = self.active.as_mut() else {
            return Ok(false);
        };
        if active.holder != holder {
            return Err(LeaseDeny::WrongHolder);
        }
        if active.id != lease {
            return Err(LeaseDeny::WrongLease);
        }
        if let Some(d) = self.lease_duration {
            active.expires_at = Some(now + d);
        }
        Ok(true)
    }

    pub fn check_expiry(&mut self, now: std::time::Instant) -> Option<LogicalLease> {
        let expired = {
            let active = self.active.as_ref()?;
            let expires_at = active.expires_at?;
            let hard_expiry = expires_at + self.grace_period.unwrap_or(std::time::Duration::ZERO);
            now >= hard_expiry
        };
        if expired {
            self.active.take()
        } else {
            None
        }
    }

    pub fn cancel_pending(&mut self, holder: TenantId) -> bool {
        if self.pending.as_ref().is_some_and(|p| p.holder == holder) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub fn release(&mut self, holder: TenantId, lease: u32) -> Result<bool, LeaseDeny> {
        let Some(active) = self.active.as_ref() else {
            return Ok(false);
        };
        if active.holder != holder {
            return Err(LeaseDeny::WrongHolder);
        }
        if active.id != lease {
            return Err(LeaseDeny::WrongLease);
        }
        self.active = None;
        Ok(true)
    }

    pub fn disconnect(&mut self, holder: TenantId) -> LeaseDisconnect {
        let cancelled_pending = self.cancel_pending(holder);
        let released = if self.active.as_ref().is_some_and(|l| l.holder == holder) {
            self.active.take()
        } else {
            None
        };
        LeaseDisconnect {
            cancelled_pending,
            released,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{LeaseBook, LeaseDecision, LeaseDeny};

    #[test]
    fn zero_and_over_capacity_are_denied() {
        let mut book = LeaseBook::new(1024);
        assert_eq!(
            book.begin_request(7, 0),
            LeaseDecision::Denied(LeaseDeny::ZeroBytes)
        );
        assert_eq!(
            book.begin_request(7, 1025),
            LeaseDecision::Denied(LeaseDeny::OverCapacity)
        );
    }

    #[test]
    fn request_stays_pending_until_explicit_grant() {
        let mut book = LeaseBook::new(1024);
        assert!(matches!(
            book.begin_request(7, 513),
            LeaseDecision::Pending(_)
        ));
        assert!(book.active().is_none());
        assert_eq!(book.pending().map(|p| p.requested_bytes), Some(513));
    }

    #[test]
    fn grant_may_round_to_slice_capacity() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 513);
        let lease = book.grant_pending(768).unwrap();
        assert_eq!(lease.bytes, 768);
        assert_eq!(lease.holder, 7);
    }

    #[test]
    fn second_holder_is_denied() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        assert_eq!(
            book.begin_request(8, 512),
            LeaseDecision::Denied(LeaseDeny::AlreadyHeld)
        );
    }

    #[test]
    fn wrong_holder_cannot_release() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        let lease = book.grant_pending(512).unwrap();
        assert_eq!(book.release(8, lease.id), Err(LeaseDeny::WrongHolder));
        assert_eq!(book.active(), Some(&lease));
    }

    #[test]
    fn lease_book_release_twice_is_one_transition() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        let lease = book.grant_pending(512).unwrap();
        assert_eq!(book.release(7, lease.id), Ok(true));
        assert_eq!(book.release(7, lease.id), Ok(false));
    }

    #[test]
    fn disconnect_cancels_or_releases_only_holder() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        assert!(book.disconnect(8).is_none());
        assert!(book.pending().is_some());
        assert!(book.disconnect(7).cancelled_pending);

        let _ = book.begin_request(7, 512);
        let lease = book.grant_pending(512).unwrap();
        assert!(book.disconnect(8).is_none());
        assert_eq!(book.active(), Some(&lease));
        assert_eq!(book.disconnect(7).released, Some(lease));
    }

    #[test]
    fn expiry_with_grace_period() {
        let mut book = LeaseBook::new(1024).with_expiry(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(5),
        );
        let _ = book.begin_request(7, 512);
        let now = std::time::Instant::now();
        let lease = book.grant_pending_at(512, now).unwrap();

        // Not expired yet
        assert!(book.check_expiry(now).is_none());
        assert!(book.check_expiry(now + std::time::Duration::from_secs(14)).is_none());

        // Expired! (10s duration + 5s grace = 15s)
        let expired = book.check_expiry(now + std::time::Duration::from_secs(15));
        assert!(expired.is_some());
        assert_eq!(expired.unwrap().id, lease.id);

        // Active lease should be gone
        assert!(book.active().is_none());
    }

    #[test]
    fn renewal_resets_expiry() {
        let mut book = LeaseBook::new(1024).with_expiry(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(5),
        );
        let _ = book.begin_request(7, 512);
        let now = std::time::Instant::now();
        let lease = book.grant_pending_at(512, now).unwrap();

        // Renew at 9s
        let renew_time = now + std::time::Duration::from_secs(9);
        assert_eq!(book.renew(7, lease.id, renew_time), Ok(true));

        // Should not expire at 15s anymore (since it was renewed at 9s)
        assert!(book.check_expiry(now + std::time::Duration::from_secs(15)).is_none());

        // Will expire at 9s + 10s + 5s = 24s
        assert!(book.check_expiry(now + std::time::Duration::from_secs(24)).is_some());
    }

    #[test]
    fn lease_id_wrap_is_refused() {
        let mut book = LeaseBook::with_next_id_for_test(1024, u32::MAX);
        let _ = book.begin_request(7, 512);
        assert_eq!(book.grant_pending(512), Err(LeaseDeny::LeaseIdExhausted));
        assert!(book.pending().is_some());
    }
}
