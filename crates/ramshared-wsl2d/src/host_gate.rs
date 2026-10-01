//! Absorbed gate logic from `ramshared-host-gate.sh`.
//!
//! Origin manifest validation, guardian health check, safe-mode gate,
//! and lease minting — all in Rust, no scripts.
//!
//! SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md §RF-5

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Errors from gate evaluation.
#[derive(Debug, PartialEq, Eq)]
pub enum GateError {
    ManifestInvalid(String),
    ManifestHashMismatch,
    GuardianStale,
    GuardianUnhealthy,
    SafeModeForeignBootId,
    SafeModeBlocked,
    LeaseDenied(String),
    OriginAuthorityRevoked,
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestInvalid(e) => write!(f, "origin manifest invalid: {e}"),
            Self::ManifestHashMismatch => write!(f, "origin manifest SHA-256 mismatch"),
            Self::GuardianStale => write!(f, "guardian health proof is stale"),
            Self::GuardianUnhealthy => write!(f, "guardian reports unhealthy"),
            Self::SafeModeForeignBootId => write!(f, "safe-mode gate: foreign boot_id"),
            Self::SafeModeBlocked => write!(f, "safe-mode gate: blocked"),
            Self::LeaseDenied(e) => write!(f, "lease denied: {e}"),
            Self::OriginAuthorityRevoked => write!(f, "origin authority revoked"),
        }
    }
}

impl std::error::Error for GateError {}

/// Validated and sealed origin configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedOrigin {
    pub logical_capacity_mib: u64,
    pub partuuid: String,
    pub origin_vhdx: String,
}

/// Lease token minted after all gates pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseToken {
    pub lease_id: u32,
    pub deadline_ms: u64,
}

/// Safe-mode decision after gate evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeModeDecision {
    Allow,
    Deny,
}

/// Guardian health status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianHealthStatus {
    pub timestamp_ms: u64,
    pub healthy: bool,
}

/// Validate origin manifest bytes and expected SHA-256 hash.
///
/// Absorbs the Python manifest validation from `ramshared-host-gate.sh`.
/// Validates size bounds, SHA-256 integrity, and required fields.
pub fn validate_origin_manifest(
    data: &[u8],
    expected_sha256: &str,
) -> Result<SealedOrigin, GateError> {
    // Size bound: 1..=64KB (matches ORIGIN_MANIFEST_MAX_BYTES)
    if data.is_empty() || data.len() > 64 * 1024 {
        return Err(GateError::ManifestInvalid(format!(
            "size {} out of bounds",
            data.len()
        )));
    }

    // SHA-256 integrity check
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let computed: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !computed.eq_ignore_ascii_case(expected_sha256) {
        return Err(GateError::ManifestHashMismatch);
    }

    // Parse as JSON and extract required fields (strip UTF-8 BOM if present)
    let json_bytes = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
    let value: serde_json::Value = serde_json::from_slice(json_bytes)
        .map_err(|e| GateError::ManifestInvalid(e.to_string()))?;

    let logical_capacity_mib = value
        .get("logical_capacity_mib")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| GateError::ManifestInvalid("missing logical_capacity_mib".into()))?;

    let partuuid = value
        .get("partuuid")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GateError::ManifestInvalid("missing partuuid".into()))?
        .to_string();

    let origin_vhdx = value
        .get("origin_vhdx")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GateError::ManifestInvalid("missing origin_vhdx".into()))?
        .to_string();

    if logical_capacity_mib == 0 {
        return Err(GateError::ManifestInvalid(
            "logical_capacity_mib must be > 0".into(),
        ));
    }

    Ok(SealedOrigin {
        logical_capacity_mib,
        partuuid,
        origin_vhdx,
    })
}

/// Check guardian health proof freshness and status.
///
/// Absorbs the guardian health check from `ramshared-host-gate.sh`.
/// `max_age_sec` defaults to 60 (matching `guardian_proof_max_age_sec`).
pub fn check_guardian_health(
    health: &GuardianHealthStatus,
    max_age_sec: u64,
) -> Result<(), GateError> {
    if !health.healthy {
        return Err(GateError::GuardianUnhealthy);
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;

    let age_ms = now_ms.saturating_sub(health.timestamp_ms);
    if age_ms > max_age_sec * 1000 {
        return Err(GateError::GuardianStale);
    }

    Ok(())
}

/// Evaluate safe-mode gate.
///
/// Absorbs the safe-mode gate check from `ramshared-host-gate.sh`.
/// Refuses if boot_id is foreign (different from expected).
pub fn evaluate_safe_mode(
    safe_mode: bool,
    received_boot_id: &str,
    expected_boot_id: &str,
) -> Result<SafeModeDecision, GateError> {
    if received_boot_id != expected_boot_id {
        return Err(GateError::SafeModeForeignBootId);
    }
    if safe_mode {
        return Ok(SafeModeDecision::Deny);
    }
    Ok(SafeModeDecision::Allow)
}

/// Mint a lease token after all gates pass.
///
/// Absorbs the lease minting from `ramshared-host-gate.sh`.
/// Requires: valid origin manifest + guardian healthy + safe-mode allows.
pub fn mint_lease(
    origin: &SealedOrigin,
    guardian_ok: bool,
    safe_mode_decision: SafeModeDecision,
    lease_id: u32,
    ttl_ms: u64,
) -> Result<LeaseToken, GateError> {
    if !guardian_ok {
        return Err(GateError::LeaseDenied("guardian not healthy".into()));
    }
    if safe_mode_decision == SafeModeDecision::Deny {
        return Err(GateError::LeaseDenied("safe mode active".into()));
    }
    if origin.logical_capacity_mib == 0 {
        return Err(GateError::LeaseDenied("invalid origin capacity".into()));
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;

    Ok(LeaseToken {
        lease_id,
        deadline_ms: now_ms.saturating_add(ttl_ms),
    })
}

/// Check if a lease is expired at the current wall clock.
pub fn lease_expired(lease: &LeaseToken) -> bool {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    lease_expired_at(lease, now_ms)
}

/// Check if a lease is expired at an explicit instant (testable, no sleep).
pub fn lease_expired_at(lease: &LeaseToken, now_ms: u64) -> bool {
    now_ms >= lease.deadline_ms
}

/// Control-plane state, matching the SPEC telemetry enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneState {
    Disconnected,
    Handshaking,
    VsockLeased,
    OriginOnly,
    SafeMode,
}

impl ControlPlaneState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Handshaking => "handshaking",
            Self::VsockLeased => "vsock_leased",
            Self::OriginOnly => "origin_only",
            Self::SafeMode => "safe_mode",
        }
    }
}

/// Fail-closed authority for cache admission and origin I/O (ITEM-3, RF-3, RF-6).
///
/// Cache admission requires a live lease **and** a verified origin identity.
/// Origin I/O requires only the verified origin identity, so lease loss
/// revokes the cache without taking the authoritative path down. Losing the
/// origin identity blocks both paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneAuthority {
    state: ControlPlaneState,
    lease: Option<LeaseToken>,
    origin: Option<SealedOrigin>,
}

impl ControlPlaneAuthority {
    /// Start with no lease and no verified origin.
    pub fn disconnected() -> Self {
        Self {
            state: ControlPlaneState::Disconnected,
            lease: None,
            origin: None,
        }
    }

    pub fn state(&self) -> ControlPlaneState {
        self.state
    }

    pub fn lease(&self) -> Option<&LeaseToken> {
        self.lease.as_ref()
    }

    pub fn origin(&self) -> Option<&SealedOrigin> {
        self.origin.as_ref()
    }

    /// Begin a handshake attempt. Does not grant any authority.
    pub fn begin_handshake(&mut self) {
        self.state = ControlPlaneState::Handshaking;
    }

    /// Accept a freshly minted lease together with its verified origin.
    ///
    /// An already-expired token is refused: accepting it would advertise a
    /// lease that cannot admit anything.
    pub fn accept_lease(
        &mut self,
        lease: LeaseToken,
        origin: SealedOrigin,
        now_ms: u64,
    ) -> Result<(), GateError> {
        if lease_expired_at(&lease, now_ms) {
            return Err(GateError::LeaseDenied("lease already expired".into()));
        }
        self.lease = Some(lease);
        self.origin = Some(origin);
        self.state = ControlPlaneState::VsockLeased;
        Ok(())
    }

    /// Transport disconnect fails closed: enter safe mode and revoke cache
    /// authority. The verified origin identity is preserved so origin I/O
    /// can continue (RF-3).
    pub fn on_vsock_disconnect(&mut self) {
        self.state = ControlPlaneState::SafeMode;
        self.lease = None;
    }

    /// Lease loss revokes cache authority and leaves only the verified origin
    /// path (ITEM-3).
    pub fn on_lease_expired(&mut self) {
        self.lease = None;
        if self.state == ControlPlaneState::VsockLeased {
            self.state = ControlPlaneState::OriginOnly;
        }
    }

    /// Origin identity loss blocks every I/O path (ITEM-3 companion).
    pub fn lose_origin_identity(&mut self) {
        self.origin = None;
    }

    /// Cache admission: live lease + verified origin. Everything else is
    /// refused — there is no file fallback and no cached-only path.
    pub fn admit_cache(&self, now_ms: u64) -> Result<(), GateError> {
        if self.state == ControlPlaneState::SafeMode {
            return Err(GateError::LeaseDenied("safe mode after disconnect".into()));
        }
        if self.state != ControlPlaneState::VsockLeased {
            return Err(GateError::LeaseDenied(
                "cache admission requires a live vsock lease".into(),
            ));
        }
        let lease = self
            .lease
            .as_ref()
            .ok_or_else(|| GateError::LeaseDenied("no live lease".into()))?;
        if lease_expired_at(lease, now_ms) {
            return Err(GateError::LeaseDenied("lease expired".into()));
        }
        if self.origin.is_none() {
            return Err(GateError::OriginAuthorityRevoked);
        }
        Ok(())
    }

    /// Origin I/O admission: only the verified origin identity is required.
    /// Lease state (including expiry and safe mode) does not take the
    /// authoritative path down.
    pub fn admit_origin_io(&self) -> Result<(), GateError> {
        if self.origin.is_none() {
            return Err(GateError::OriginAuthorityRevoked);
        }
        Ok(())
    }
}

/// Gate lease minting on a successful control-plane connection.
///
/// There is no file fallback (RF-6 / SPEC §MODIFY): a connection failure
/// must never grant a lease. The mint closure runs only after `connect`
/// succeeds, so a failed connection cannot produce a token by construction.
pub fn lease_after_connect<T, E, F>(connect: Result<T, E>, mint: F) -> Result<LeaseToken, GateError>
where
    F: FnOnce(T) -> Result<LeaseToken, GateError>,
{
    let established =
        connect.map_err(|_| GateError::LeaseDenied("control-plane connection failed".into()))?;
    mint(established)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn make_manifest_json(capacity_mib: u64) -> Vec<u8> {
        serde_json::json!({
            "logical_capacity_mib": capacity_mib,
            "partuuid": "11111111-2222-3333-4444-555555555555",
            "origin_vhdx": "C:\\ramshared\\origin.vhdx"
        })
        .to_string()
        .into_bytes()
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn validate_origin_manifest_matches_script() {
        let data = make_manifest_json(4096);
        let hash = sha256_hex(&data);
        let result = validate_origin_manifest(&data, &hash).expect("valid manifest");
        assert_eq!(result.logical_capacity_mib, 4096);
        assert_eq!(result.partuuid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(result.origin_vhdx, "C:\\ramshared\\origin.vhdx");
    }

    #[test]
    fn validate_origin_manifest_rejects_bad_hash() {
        let data = make_manifest_json(4096);
        assert!(matches!(
            validate_origin_manifest(&data, "bad-hash"),
            Err(GateError::ManifestHashMismatch)
        ));
    }

    #[test]
    fn validate_origin_manifest_rejects_empty() {
        assert!(matches!(
            validate_origin_manifest(b"", "any"),
            Err(GateError::ManifestInvalid(_))
        ));
    }

    #[test]
    fn validate_origin_manifest_rejects_oversized() {
        let big = vec![0u8; 64 * 1024 + 1];
        assert!(matches!(
            validate_origin_manifest(&big, "any"),
            Err(GateError::ManifestInvalid(_))
        ));
    }

    #[test]
    fn validate_origin_manifest_rejects_missing_fields() {
        let data = b"{\"logical_capacity_mib\": 100}";
        let hash = sha256_hex(data);
        assert!(matches!(
            validate_origin_manifest(data, &hash),
            Err(GateError::ManifestInvalid(_))
        ));
    }

    #[test]
    fn check_guardian_health_rejects_stale() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let stale = GuardianHealthStatus {
            timestamp_ms: now_ms - 120_000, // 120s old
            healthy: true,
        };
        assert!(matches!(
            check_guardian_health(&stale, 60),
            Err(GateError::GuardianStale)
        ));
    }

    #[test]
    fn check_guardian_health_rejects_unhealthy() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let unhealthy = GuardianHealthStatus {
            timestamp_ms: now_ms,
            healthy: false,
        };
        assert!(matches!(
            check_guardian_health(&unhealthy, 60),
            Err(GateError::GuardianUnhealthy)
        ));
    }

    #[test]
    fn check_guardian_health_accepts_fresh() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let fresh = GuardianHealthStatus {
            timestamp_ms: now_ms,
            healthy: true,
        };
        assert!(check_guardian_health(&fresh, 60).is_ok());
    }

    #[test]
    fn evaluate_safe_mode_refuses_foreign_boot_id() {
        assert!(matches!(
            evaluate_safe_mode(false, "boot-abc", "boot-xyz"),
            Err(GateError::SafeModeForeignBootId)
        ));
    }

    #[test]
    fn evaluate_safe_mode_allows_matching_boot_id() {
        assert_eq!(
            evaluate_safe_mode(false, "boot-abc", "boot-abc"),
            Ok(SafeModeDecision::Allow)
        );
    }

    #[test]
    fn evaluate_safe_mode_denies_when_safe_mode_active() {
        assert_eq!(
            evaluate_safe_mode(true, "boot-abc", "boot-abc"),
            Ok(SafeModeDecision::Deny)
        );
    }

    #[test]
    fn mint_lease_requires_all_gates() {
        let origin = SealedOrigin {
            logical_capacity_mib: 4096,
            partuuid: "p".into(),
            origin_vhdx: "v".into(),
        };

        // Happy path
        let lease = mint_lease(&origin, true, SafeModeDecision::Allow, 1, 15_000)
            .expect("lease should mint");
        assert_eq!(lease.lease_id, 1);

        // Guardian not healthy
        assert!(matches!(
            mint_lease(&origin, false, SafeModeDecision::Allow, 1, 15_000),
            Err(GateError::LeaseDenied(_))
        ));

        // Safe mode active
        assert!(matches!(
            mint_lease(&origin, true, SafeModeDecision::Deny, 1, 15_000),
            Err(GateError::LeaseDenied(_))
        ));

        // Invalid origin
        let bad_origin = SealedOrigin {
            logical_capacity_mib: 0,
            partuuid: "p".into(),
            origin_vhdx: "v".into(),
        };
        assert!(matches!(
            mint_lease(&bad_origin, true, SafeModeDecision::Allow, 1, 15_000),
            Err(GateError::LeaseDenied(_))
        ));
    }

    #[test]
    fn lease_expiry_is_detected() {
        let origin = SealedOrigin {
            logical_capacity_mib: 4096,
            partuuid: "p".into(),
            origin_vhdx: "v".into(),
        };
        let lease = mint_lease(&origin, true, SafeModeDecision::Allow, 1, 0).expect("mint");
        // TTL 0 means deadline == now → already expired
        assert!(lease_expired(&lease));
    }

    #[test]
    fn host_gate_shadow_comparison() {
        // Shadow comparison: validate_origin_manifest must produce identical
        // decisions to the Python logic in ramshared-host-gate.sh on known fixtures.
        let fixtures: Vec<(Vec<u8>, &str, bool)> = vec![
            (make_manifest_json(4096), "", true), // valid (hash computed below)
            (make_manifest_json(0), "", false),   // zero capacity
            (b"{}".to_vec(), "", false),          // missing fields
            (vec![0u8; 100_000], "", false),      // oversized
        ];

        for (data, _placeholder, expect_valid) in fixtures {
            let hash = sha256_hex(&data);
            let result = validate_origin_manifest(&data, &hash);
            if expect_valid {
                assert!(result.is_ok(), "fixture should be valid: {data:?}");
            } else {
                assert!(result.is_err(), "fixture should be invalid");
            }
        }
    }
}
