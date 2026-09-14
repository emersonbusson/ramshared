//! Transport-independent logical lease ownership.

use std::time::{Duration, Instant};

use crate::model::TenantId;

/// Default lifetime for an active logical lease when a broker shell does not
/// expose a deployment-specific policy surface.
pub const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(30);

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
    InvalidTtl,
    DeadlineOverflow,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaseDisconnect {
    pub cancelled_pending: bool,
    pub retained_active: Option<LogicalLease>,
}

impl LeaseDisconnect {
    pub fn is_none(&self) -> bool {
        !self.cancelled_pending && self.retained_active.is_none()
    }
}

#[derive(Clone, Debug)]
struct ActiveLease {
    lease: LogicalLease,
    deadline: Instant,
}

#[derive(Clone, Debug)]
pub struct LeaseBook {
    capacity_bytes: u64,
    next_id: u32,
    pending: Option<PendingLease>,
    active: Option<ActiveLease>,
}

impl LeaseBook {
    pub fn new(capacity_bytes: u64) -> Self {
        Self {
            capacity_bytes,
            next_id: 1,
            pending: None,
            active: None,
        }
    }

    #[cfg(test)]
    fn with_next_id_for_test(capacity_bytes: u64, next_id: u32) -> Self {
        Self {
            capacity_bytes,
            next_id,
            pending: None,
            active: None,
        }
    }

    pub fn pending(&self) -> Option<&PendingLease> {
        self.pending.as_ref()
    }

    pub fn active(&self) -> Option<&LogicalLease> {
        self.active.as_ref().map(|active| &active.lease)
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

    /// Converts a pending request into a time-bounded active lease.
    ///
    /// The deadline is computed before mutating state, so an invalid lifetime
    /// leaves the request pending for a later valid grant attempt.
    pub fn grant_pending(
        &mut self,
        granted_bytes: u64,
        now: Instant,
        ttl: Duration,
    ) -> Result<LogicalLease, LeaseDeny> {
        if ttl.is_zero() {
            return Err(LeaseDeny::InvalidTtl);
        }
        let deadline = now.checked_add(ttl).ok_or(LeaseDeny::DeadlineOverflow)?;
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
        };
        self.next_id = following_id;
        self.pending = None;
        self.active = Some(ActiveLease {
            lease: lease.clone(),
            deadline,
        });
        Ok(lease)
    }

    /// Extends the active lease only when the heartbeat belongs to its holder.
    ///
    /// Invalid durations never modify an existing deadline.
    pub fn renew_active(&mut self, holder: TenantId, now: Instant, ttl: Duration) -> bool {
        if ttl.is_zero() {
            return false;
        }
        let Some(deadline) = now.checked_add(ttl) else {
            return false;
        };
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        if active.lease.holder != holder {
            return false;
        }
        active.deadline = deadline;
        true
    }

    /// Returns the expired lease exactly once. Time is supplied by the caller
    /// so the state machine remains deterministic under test.
    pub fn expire(&mut self, now: Instant) -> Option<LogicalLease> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| now >= active.deadline)
        {
            self.active.take().map(|active| active.lease)
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
        if active.lease.holder != holder {
            return Err(LeaseDeny::WrongHolder);
        }
        if active.lease.id != lease {
            return Err(LeaseDeny::WrongLease);
        }
        self.active = None;
        Ok(true)
    }

    pub fn disconnect(&mut self, holder: TenantId) -> LeaseDisconnect {
        let cancelled_pending = self.cancel_pending(holder);
        let retained_active = if self
            .active
            .as_ref()
            .is_some_and(|active| active.lease.holder == holder)
        {
            self.active.as_ref().map(|active| active.lease.clone())
        } else {
            None
        };
        LeaseDisconnect {
            cancelled_pending,
            retained_active,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use std::time::{Duration, Instant};

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
        let lease = book
            .grant_pending(768, Instant::now(), Duration::from_secs(1))
            .unwrap();
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
        let lease = book
            .grant_pending(512, Instant::now(), Duration::from_secs(1))
            .unwrap();
        assert_eq!(book.release(8, lease.id), Err(LeaseDeny::WrongHolder));
        assert_eq!(book.active(), Some(&lease));
    }

    #[test]
    fn lease_book_release_twice_is_one_transition() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        let lease = book
            .grant_pending(512, Instant::now(), Duration::from_secs(1))
            .unwrap();
        assert_eq!(book.release(7, lease.id), Ok(true));
        assert_eq!(book.release(7, lease.id), Ok(false));
    }

    #[test]
    fn disconnect_cancels_pending_and_retains_only_holder_active_lease() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        assert!(book.disconnect(8).is_none());
        assert!(book.pending().is_some());
        assert!(book.disconnect(7).cancelled_pending);

        let _ = book.begin_request(7, 512);
        let lease = book
            .grant_pending(512, Instant::now(), Duration::from_secs(1))
            .unwrap();
        assert!(book.disconnect(8).is_none());
        assert_eq!(book.active(), Some(&lease));
        assert_eq!(book.disconnect(7).retained_active, Some(lease));
    }

    #[test]
    fn lease_id_wrap_is_refused() {
        let mut book = LeaseBook::with_next_id_for_test(1024, u32::MAX);
        let _ = book.begin_request(7, 512);
        assert_eq!(
            book.grant_pending(512, Instant::now(), Duration::from_secs(1)),
            Err(LeaseDeny::LeaseIdExhausted)
        );
        assert!(book.pending().is_some());
    }

    #[test]
    fn active_lease_renews_only_for_holder() {
        let mut book = LeaseBook::new(1024);
        let started = Instant::now();
        let ttl = Duration::from_secs(10);
        let _ = book.begin_request(7, 512);
        let lease = book.grant_pending(512, started, ttl).unwrap();

        assert!(!book.renew_active(8, started + Duration::from_secs(5), ttl));
        assert!(book.renew_active(7, started + Duration::from_secs(5), ttl));
        assert_eq!(book.expire(started + ttl), None);
        assert_eq!(book.expire(started + Duration::from_secs(15)), Some(lease));
    }

    #[test]
    fn active_lease_expires_at_its_monotonic_deadline() {
        let mut book = LeaseBook::new(1024);
        let started = Instant::now();
        let ttl = Duration::from_secs(10);
        let _ = book.begin_request(7, 512);
        let lease = book.grant_pending(512, started, ttl).unwrap();

        assert_eq!(
            book.expire(started + Duration::from_secs(9)),
            None,
            "a lease must remain active before its deadline"
        );
        assert_eq!(book.expire(started + ttl), Some(lease));
        assert_eq!(book.expire(started + Duration::from_secs(20)), None);
    }

    #[test]
    fn disconnect_cancels_pending_but_retains_active_lease() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);
        assert!(book.disconnect(7).cancelled_pending);

        let started = Instant::now();
        let _ = book.begin_request(7, 512);
        let lease = book
            .grant_pending(512, started, Duration::from_secs(10))
            .unwrap();
        let disconnected = book.disconnect(7);

        assert_eq!(disconnected.retained_active, Some(lease.clone()));
        assert_eq!(book.active(), Some(&lease));
    }

    #[test]
    fn zero_ttl_refuses_before_activating_pending_lease() {
        let mut book = LeaseBook::new(1024);
        let _ = book.begin_request(7, 512);

        assert_eq!(
            book.grant_pending(512, Instant::now(), Duration::ZERO),
            Err(LeaseDeny::InvalidTtl)
        );
        assert!(book.pending().is_some());
        assert!(book.active().is_none());
    }
}
