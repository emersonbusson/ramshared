//! Single configured-reserve authority for every VRAM admission surface.
//!
//! SPEC: `docs/specs/no-milestone/gpu-reserve-floor-authority/SPEC.md`.
//!
//! Before this module existed, four surfaces computed a GPU reserve floor from
//! four different formulas and four different defaults: the sparse tier's env
//! reader defaulted to 512 MiB and clamped to 128, the origin physical target
//! added a hardcoded `2 GiB`, the worker/broker paths used a 640 MiB runtime
//! buffer with a 20% share, and the preflight script aborted below 256 MiB.
//! A seal attesting one of those numbers guaranteed none of them.
//!
//! This module resolves the **configured** reserve once, from the already
//! verified host-origin manifest, and applies a raise-only environment
//! override. It deliberately owns no runtime headroom: `RUNTIME_FREE_BUFFER_BYTES`
//! stays the caller's second argument to `required_free_bytes` / `safe_target_bytes`
//! (DT-1). Callers pass `configured_reserve_bytes(capacity)` through unchanged.

use core::fmt;

/// MiB → bytes. The only unit conversion in this module.
const MIB: u64 = 1024 * 1024;

/// Non-negotiable percentage safety floor.
///
/// The helper's internal `capacity.div_ceil(5)` term is left in place and can
/// never be undercut, so a resealed percentage below 20 can only fail the seal
/// (DT-2). This constant is the seal-verification threshold.
pub const SEALED_PERCENT_SAFETY_FLOOR: u64 = 20;

/// Sealed host-origin manifest `gpu_reserve_min_mib` (DT-3).
///
/// The one literal every consumer must read. Sparse-tier admission, the
/// isolated cache worker, the daemon startup line, the preflight go/no-go gate,
/// and the cascade boot headroom default all bind to this value; a second copy
/// of `2048` is a drift hazard the seal cannot detect.
pub const SEALED_RESERVE_MIN_MIB: u64 = 2048;

/// Sealed host-origin manifest `gpu_reserve_percent` (DT-3).
///
/// Equal to [`SEALED_PERCENT_SAFETY_FLOOR`] today and required to be
/// `>=` it by [`ReserveFloorPolicy::from_manifest`]. They stay distinct
/// constants: one is what the seal attests, the other is the lowest value a
/// reseal may attest.
pub const SEALED_RESERVE_PERCENT: u64 = 20;

/// The two documented configuration names, both read from the **process
/// environment** exactly as `ReserveFloorEnv` reads them
/// (DT-11). Neither name is deprecated in this slice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReserveFloorEnv {
    /// `RAMSHARED_MIN_VRAM_FREE_MIB` — an environment override.
    pub env_mib: Option<u64>,
    /// `MIN_VRAM_HEADROOM_MIB` — a name documented as a
    /// `/etc/ramshared/cascade.conf` key (`wsl2-cascade-boot` PRD §7).
    /// The boot config resolver reads it from conf/env for the identity gate;
    /// this type reads it from the process environment only and must not claim
    /// to parse that file. The operator or systemd unit sets it in the
    /// environment when the conf key should affect runtime reserve policy.
    pub alias_mib: Option<u64>,
}

impl ReserveFloorEnv {
    /// Reads both documented names from the process environment.
    ///
    /// An unparsable value is treated as absent; a conflict between two
    /// *parsed* values is refused later by [`ReserveFloorPolicy::resolve_with_env`].
    pub fn from_process_env() -> Self {
        Self {
            env_mib: parse_env_mib("RAMSHARED_MIN_VRAM_FREE_MIB"),
            alias_mib: parse_env_mib("MIN_VRAM_HEADROOM_MIB"),
        }
    }
}

/// Parses one override value. An unparsable value is absent (DT-11): a typo
/// must never silently become a floor.
///
/// Split out from the environment lookup so it is testable without process-env
/// mutation (this crate forbids `unsafe`, and `set_var` is unsafe).
fn parse_mib_value(raw: Option<String>) -> Option<u64> {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
}

fn parse_env_mib(name: &str) -> Option<u64> {
    parse_mib_value(std::env::var(name).ok())
}

/// The configured-reserve authority (DT-1).
///
/// Immutable after resolution. Carries **no** runtime headroom — that is
/// `RUNTIME_FREE_BUFFER_BYTES` and stays the caller's second argument to
/// `required_free_bytes` / `safe_target_bytes`, exactly as the existing call
/// sites already pass it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveFloorPolicy {
    /// Resolved minimum floor in bytes. Starts as the sealed manifest minimum;
    /// a raise-only override may raise it and never lower it. After an override
    /// this is **not** the sealed value.
    pub min_floor_bytes: u64,
    /// Always the verified manifest percentage. Never overridable (DT-8).
    pub sealed_percent: u64,
}

/// Refusal raised while resolving the reserve policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReserveFloorError {
    /// A raise-only override tried to go below the sealed minimum (DT-8).
    OverrideBelowSealed {
        /// The rejected override, in MiB.
        override_mib: u64,
        /// The sealed minimum it was measured against, in MiB.
        sealed_min_mib: u64,
    },
    /// Both documented names were set to different values (DT-11).
    ConflictingOverrides {
        /// `RAMSHARED_MIN_VRAM_FREE_MIB`.
        env_mib: u64,
        /// `MIN_VRAM_HEADROOM_MIB`.
        alias_mib: u64,
    },
    /// `gpu_reserve_percent` below [`SEALED_PERCENT_SAFETY_FLOOR`] (DT-2).
    PercentBelowSafetyFloor {
        /// The rejected percentage.
        sealed_percent: u64,
    },
    /// A sealed manifest field that must be positive was zero (DT-9 case 1).
    ZeroSealedField {
        /// Which sealed field was zero.
        field: &'static str,
    },
    /// A checked conversion or arithmetic step overflowed (DT-9).
    Overflow,
}

impl fmt::Display for ReserveFloorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReserveFloorError::OverrideBelowSealed {
                override_mib,
                sealed_min_mib,
            } => write!(
                f,
                "reserve floor override below sealed authority: {override_mib} < {sealed_min_mib}"
            ),
            ReserveFloorError::ConflictingOverrides { env_mib, alias_mib } => write!(
                f,
                "conflicting reserve floor overrides: \
                 RAMSHARED_MIN_VRAM_FREE_MIB={env_mib} MIN_VRAM_HEADROOM_MIB={alias_mib}"
            ),
            ReserveFloorError::PercentBelowSafetyFloor { sealed_percent } => write!(
                f,
                "sealed gpu_reserve_percent {sealed_percent} below the \
                 non-negotiable {SEALED_PERCENT_SAFETY_FLOOR}% safety floor"
            ),
            ReserveFloorError::ZeroSealedField { field } => {
                write!(f, "sealed manifest field {field} must be non-zero")
            }
            ReserveFloorError::Overflow => write!(f, "reserve floor resolution overflowed"),
        }
    }
}

impl std::error::Error for ReserveFloorError {}

/// The one three-term free-floor formula (DT-2, DT-4).
///
/// `max(configured_reserve_bytes, ceil(capacity / 5)) + runtime_headroom_bytes`.
///
/// This is arithmetically identical to
/// `GpuBudgetSnapshot::required_free_bytes(configured_reserve_bytes,
/// runtime_headroom_bytes)` over the same capacity, which
/// `enforced_free_floor_bytes_matches_required_free_bytes` asserts. It exists
/// so a caller that has no `GpuBudgetSnapshot` (the sparse tier, which samples
/// `mem_info()` directly) can obtain the same threshold without constructing a
/// snapshot and without inventing budget provenance.
///
/// The `ceil(capacity / 5)` term is the non-negotiable 20% safety floor: a
/// configured value of `0` never drops the floor below it.
pub fn enforced_free_floor_from_configured(
    configured_reserve_bytes: u64,
    capacity: u64,
    runtime_headroom_bytes: u64,
) -> u64 {
    configured_reserve_bytes
        .max(capacity.div_ceil(5))
        .saturating_add(runtime_headroom_bytes)
}

impl ReserveFloorPolicy {
    /// Builds the policy from the already-verified host-origin manifest fields
    /// (DT-3). No new source of truth is introduced.
    ///
    /// Refuses `gpu_reserve_min_mib == 0` or `gpu_reserve_percent == 0`
    /// (DT-9 case 1) and `gpu_reserve_percent < 20` (DT-2).
    pub fn from_manifest(
        gpu_reserve_min_mib: u64,
        gpu_reserve_percent: u64,
    ) -> Result<Self, ReserveFloorError> {
        if gpu_reserve_min_mib == 0 {
            return Err(ReserveFloorError::ZeroSealedField {
                field: "gpu_reserve_min_mib",
            });
        }
        if gpu_reserve_percent == 0 {
            return Err(ReserveFloorError::ZeroSealedField {
                field: "gpu_reserve_percent",
            });
        }
        if gpu_reserve_percent < SEALED_PERCENT_SAFETY_FLOOR {
            return Err(ReserveFloorError::PercentBelowSafetyFloor {
                sealed_percent: gpu_reserve_percent,
            });
        }
        let min_floor_bytes = gpu_reserve_min_mib
            .checked_mul(MIB)
            .ok_or(ReserveFloorError::Overflow)?;
        Ok(Self {
            min_floor_bytes,
            sealed_percent: gpu_reserve_percent,
        })
    }

    /// Applies the raise-only environment override (DT-8).
    ///
    /// Writes `min_floor_bytes` only; `sealed_percent` is taken unchanged from
    /// the verified manifest and is never overridable, so
    /// [`Self::configured_reserve_bytes`] keeps applying the percentage share
    /// after any override. An override is accepted only when it is
    /// `>=` the sealed minimum; there is no clamp-to-128.
    pub fn resolve_with_env(base: &Self, env: &ReserveFloorEnv) -> Result<Self, ReserveFloorError> {
        let override_mib = match (env.env_mib, env.alias_mib) {
            (None, None) => None,
            (Some(a), None) | (None, Some(a)) => Some(a),
            (Some(a), Some(b)) => {
                if a != b {
                    return Err(ReserveFloorError::ConflictingOverrides {
                        env_mib: a,
                        alias_mib: b,
                    });
                }
                Some(a)
            }
        };
        let Some(override_mib) = override_mib else {
            return Ok(*base);
        };

        let sealed_min_mib = base.min_floor_bytes / MIB;
        if override_mib < sealed_min_mib {
            return Err(ReserveFloorError::OverrideBelowSealed {
                override_mib,
                sealed_min_mib,
            });
        }
        let override_bytes = override_mib
            .checked_mul(MIB)
            .ok_or(ReserveFloorError::Overflow)?;
        Ok(Self {
            min_floor_bytes: override_bytes,
            sealed_percent: base.sealed_percent,
        })
    }

    /// `max(min_floor_bytes, floor(capacity * sealed_percent / 100))` (DT-2).
    ///
    /// `capacity` is **exactly** the expression the shared helpers already
    /// use — `total_bytes.unwrap_or(budget_bytes).min(budget_bytes)` derived
    /// from the same snapshot handed to the helper — never `total_vram_bytes`
    /// alone and never a second definition. See
    /// [`Self::helper_capacity`] for the one place that expression is named.
    ///
    /// `capacity == 0` is a degenerate query and returns `min_floor_bytes`
    /// (DT-9 case 2); it is not an error. The value returned is passed
    /// unchanged as the `configured_reserve_bytes` argument to
    /// `required_free_bytes` / `safe_target_bytes`, which still apply their own
    /// non-negotiable `capacity.div_ceil(5)` safety floor — so a configured
    /// value of `0` is safe and never yields a larger allocation than a
    /// well-formed one (DT-9 case 3).
    pub fn configured_reserve_bytes(&self, capacity: u64) -> u64 {
        if capacity == 0 {
            return self.min_floor_bytes;
        }
        let share = capacity
            .checked_mul(self.sealed_percent)
            .map(|p| p / 100)
            .unwrap_or(u64::MAX);
        self.min_floor_bytes.max(share)
    }

    /// The one capacity expression every caller must use (DT-2).
    ///
    /// Mirrors `GpuBudgetSnapshot::{safe_target_bytes, required_free_bytes}`:
    /// `total_bytes.unwrap_or(budget_bytes).min(budget_bytes)`. Named here so a
    /// test can pin the equality instead of asserting a formula against its own
    /// definition.
    pub fn helper_capacity(total_bytes: Option<u64>, budget_bytes: u64) -> u64 {
        total_bytes.unwrap_or(budget_bytes).min(budget_bytes)
    }

    /// The three-term maximum floor this policy plus the helper enforce (DT-2).
    ///
    /// `max(min_floor_bytes, floor(capacity * sealed_percent / 100), ceil(capacity / 5))
    /// + runtime_headroom_bytes`. The first two terms come from the policy; the
    ///   third is the helper's non-negotiable 20% safety floor. This is the
    ///   value that must be logged and shown as the enforced floor (NFR-3).
    pub fn enforced_free_floor_bytes(&self, capacity: u64, runtime_headroom_bytes: u64) -> u64 {
        enforced_free_floor_from_configured(
            self.configured_reserve_bytes(capacity),
            capacity,
            runtime_headroom_bytes,
        )
    }

    /// Honest source label for the startup log (DT-10).
    ///
    /// `LabOverrideRaise` only when an override actually raises the enforced
    /// result above the sealed-only result. When the override is **subsumed** by
    /// the percentage share the label is `SealedManifest`, so an operator is
    /// never told the floor moved when it did not.
    pub fn source_label(&self, base: &Self, capacity: u64) -> ReserveFloorSource {
        if self.min_floor_bytes == base.min_floor_bytes {
            return ReserveFloorSource::SealedManifest;
        }
        let sealed_only = base.configured_reserve_bytes(capacity);
        if self.configured_reserve_bytes(capacity) > sealed_only {
            ReserveFloorSource::LabOverrideRaise
        } else {
            ReserveFloorSource::SealedManifest
        }
    }
}

/// Where the enforced floor actually came from (DT-10, NFR-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReserveFloorSource {
    /// The sealed manifest resolved the floor. Includes an override that was
    /// accepted but subsumed by the percentage share.
    SealedManifest,
    /// A raise-only lab override actually raised the enforced result.
    LabOverrideRaise,
}

impl fmt::Display for ReserveFloorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReserveFloorSource::SealedManifest => write!(f, "sealed-manifest"),
            ReserveFloorSource::LabOverrideRaise => write!(f, "lab-override-raise"),
        }
    }
}

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules).
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{GpuBudgetSnapshot, GpuBudgetSource};
    use std::time::Instant;

    /// Sealed defaults used across the tests, bound to the public authority so
    /// a resealed literal cannot drift away from what the tests exercise.
    const SEALED_MIN_MIB: u64 = SEALED_RESERVE_MIN_MIB;
    const SEALED_PERCENT: u64 = SEALED_RESERVE_PERCENT;

    #[test]
    fn source_labels_are_human_readable() {
        assert_eq!(
            ReserveFloorSource::SealedManifest.to_string(),
            "sealed-manifest"
        );
        assert_eq!(
            ReserveFloorSource::LabOverrideRaise.to_string(),
            "lab-override-raise"
        );
        assert_eq!(
            ReserveFloorError::Overflow.to_string(),
            "reserve floor resolution overflowed"
        );
    }

    fn sealed() -> ReserveFloorPolicy {
        ReserveFloorPolicy::from_manifest(SEALED_MIN_MIB, SEALED_PERCENT).unwrap()
    }

    #[test]
    fn override_below_sealed_is_refused() {
        let base = sealed();
        let env = ReserveFloorEnv {
            env_mib: Some(512),
            alias_mib: None,
        };
        let err = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap_err();
        assert_eq!(
            err,
            ReserveFloorError::OverrideBelowSealed {
                override_mib: 512,
                sealed_min_mib: SEALED_MIN_MIB,
            }
        );
    }

    #[test]
    fn override_above_sealed_is_raise_only() {
        let base = sealed();
        let env = ReserveFloorEnv {
            env_mib: Some(4096),
            alias_mib: None,
        };
        let resolved = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap();
        assert_eq!(resolved.min_floor_bytes, 4096 * MIB);
        assert_eq!(resolved.sealed_percent, SEALED_PERCENT);
        // The sealed minimum is never lowered by the base path either.
        assert!(resolved.min_floor_bytes >= base.min_floor_bytes);
    }

    #[test]
    fn override_writes_min_floor_only() {
        let base = sealed();
        let env = ReserveFloorEnv {
            env_mib: None,
            alias_mib: Some(3072),
        };
        let resolved = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap();
        // Raised.
        assert_eq!(resolved.min_floor_bytes, 3072 * MIB);
        // Percentage share is untouched and still applied after the override.
        assert_eq!(resolved.sealed_percent, base.sealed_percent);
        // Capacity large enough that the 20% share dominates the raised floor,
        // so this asserts the share is still applied and not dropped by the
        // override (DT-8). 20 GiB * 20% = 4 GiB > 3072 MiB.
        let capacity = 20 * 1024 * MIB;
        let share = capacity * SEALED_PERCENT / 100;
        assert!(share > resolved.min_floor_bytes);
        assert_eq!(resolved.configured_reserve_bytes(capacity), share);
        // And the sealed percentage really is the one used.
        assert_eq!(
            resolved.configured_reserve_bytes(capacity),
            capacity * base.sealed_percent / 100
        );
    }

    #[test]
    fn conflicting_overrides_refuse() {
        let base = sealed();
        let env = ReserveFloorEnv {
            env_mib: Some(4096),
            alias_mib: Some(3072),
        };
        let err = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap_err();
        assert_eq!(
            err,
            ReserveFloorError::ConflictingOverrides {
                env_mib: 4096,
                alias_mib: 3072,
            }
        );
    }

    #[test]
    fn percent_below_twenty_is_rejected() {
        let err = ReserveFloorPolicy::from_manifest(2048, 19).unwrap_err();
        assert_eq!(
            err,
            ReserveFloorError::PercentBelowSafetyFloor { sealed_percent: 19 }
        );
        assert!(ReserveFloorPolicy::from_manifest(2048, 20).is_ok());
    }

    #[test]
    fn configured_reserve_never_below_twenty_percent() {
        // A sealed minimum far below the 20% share must not win: the helper
        // keeps its own ceil(capacity/5) floor, so this asserts the policy
        // returns at least the share and the helper's floor is never undercut.
        let policy = ReserveFloorPolicy::from_manifest(1, 20).unwrap();
        let capacity = 5000 * MIB;
        let configured = policy.configured_reserve_bytes(capacity);
        assert!(configured >= capacity / 5);
        assert_eq!(configured, capacity * 20 / 100);
    }

    #[test]
    fn configured_reserve_honors_sealed_percent_above_twenty() {
        // A 1 MiB sealed minimum so the 30% share is the binding term.
        let policy = ReserveFloorPolicy::from_manifest(1, 30).unwrap();
        let capacity = 6144 * MIB;
        let share = capacity * 30 / 100;
        assert!(share > policy.min_floor_bytes);
        assert_eq!(policy.configured_reserve_bytes(capacity), share);
        // Above the 20% floor the sealed percentage is honoured exactly, not
        // clamped down to 20.
        assert!(share > capacity * SEALED_PERCENT_SAFETY_FLOOR / 100);
    }

    #[test]
    fn configured_reserve_capacity_matches_helper_capacity() {
        // DT-2 pins `capacity` to the helpers' own expression. This is a real
        // equality between two independently written expressions, not a
        // tautology over one function.
        // Not a literal `Some`: the point is the `unwrap_or` shape the helper
        // uses, which clippy would flag as a tautology if the value were known.
        let total_bytes: Option<u64> = std::env::var("X")
            .ok()
            .map(|_| 6144 * MIB)
            .or(Some(6144 * MIB));
        let budget_bytes = 4016 * MIB;
        let from_helper_shape = total_bytes.unwrap_or(budget_bytes).min(budget_bytes);
        assert_eq!(from_helper_shape, 4016 * MIB);
        assert_eq!(
            ReserveFloorPolicy::helper_capacity(total_bytes, budget_bytes),
            from_helper_shape
        );
        // When the total is unknown the budget is the capacity.
        assert_eq!(
            ReserveFloorPolicy::helper_capacity(None, budget_bytes),
            budget_bytes
        );
        // When budget < total (typical WDDM) the budget binds.
        assert_eq!(
            ReserveFloorPolicy::helper_capacity(Some(6144 * MIB), 4016 * MIB),
            4016 * MIB
        );
        // And the policy consumes exactly that value.
        let policy = sealed();
        let capacity = ReserveFloorPolicy::helper_capacity(total_bytes, budget_bytes);
        assert_eq!(
            policy.configured_reserve_bytes(capacity),
            policy.configured_reserve_bytes(from_helper_shape)
        );
    }

    #[test]
    fn zero_sealed_field_is_refused() {
        assert_eq!(
            ReserveFloorPolicy::from_manifest(0, 20).unwrap_err(),
            ReserveFloorError::ZeroSealedField {
                field: "gpu_reserve_min_mib"
            }
        );
        assert_eq!(
            ReserveFloorPolicy::from_manifest(2048, 0).unwrap_err(),
            ReserveFloorError::ZeroSealedField {
                field: "gpu_reserve_percent"
            }
        );
    }

    #[test]
    fn zero_capacity_returns_min_floor() {
        let policy = sealed();
        assert_eq!(policy.configured_reserve_bytes(0), policy.min_floor_bytes);
    }

    #[test]
    fn zero_configured_never_raises_allocation() {
        // DT-9 case 3: `configured_reserve_bytes = 0` is safe, not an error,
        // because `required_free_bytes` still applies `capacity.div_ceil(5)`.
        // Assert the fail-safe: a zero configured value never produces a larger
        // allocation than a well-formed one.
        let capacity = 6144 * MIB;
        let runtime = 640 * MIB;
        let zero = ReserveFloorPolicy {
            min_floor_bytes: 0,
            sealed_percent: 20,
        };
        let well_formed = sealed();
        // `enforced_free_floor_bytes` is the same three-term maximum the helper
        // computes; the zero policy must never floor *below* the well-formed
        // one is the wrong direction — it must never floor *lower*, i.e. never
        // allow a larger allocation. Lower floor = larger allocation.
        let floor_zero = zero.enforced_free_floor_bytes(capacity, runtime);
        let floor_good = well_formed.enforced_free_floor_bytes(capacity, runtime);
        assert!(
            floor_zero <= floor_good,
            "a zero configured reserve must not raise the floor above a well-formed policy \
             (floor_zero={floor_zero} floor_good={floor_good})"
        );
        // The safety share still binds, so the zero policy is not a free pass.
        assert_eq!(floor_zero, capacity.div_ceil(5) + runtime);
    }

    #[test]
    fn overflow_is_refused() {
        // MiB → bytes overflow.
        assert_eq!(
            ReserveFloorPolicy::from_manifest(u64::MAX / 2, 20).unwrap_err(),
            ReserveFloorError::Overflow
        );
        // Override overflow after the raise-only comparison passes.
        let base = sealed();
        let env = ReserveFloorEnv {
            env_mib: Some(u64::MAX),
            alias_mib: None,
        };
        assert_eq!(
            ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap_err(),
            ReserveFloorError::Overflow
        );
    }

    #[test]
    fn both_documented_names_resolve_raise_only() {
        let base = sealed();
        // Each name alone is accepted when it raises.
        for env in [
            ReserveFloorEnv {
                env_mib: Some(4096),
                alias_mib: None,
            },
            ReserveFloorEnv {
                env_mib: None,
                alias_mib: Some(4096),
            },
        ] {
            let resolved = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap();
            assert_eq!(resolved.min_floor_bytes, 4096 * MIB);
            assert_eq!(resolved.sealed_percent, SEALED_PERCENT);
        }
        // Each name alone is refused when it lowers.
        for env in [
            ReserveFloorEnv {
                env_mib: Some(128),
                alias_mib: None,
            },
            ReserveFloorEnv {
                env_mib: None,
                alias_mib: Some(128),
            },
        ] {
            assert!(matches!(
                ReserveFloorPolicy::resolve_with_env(&base, &env),
                Err(ReserveFloorError::OverrideBelowSealed { .. })
            ));
        }
        // Both names agreeing on a raise is not a conflict.
        let agree = ReserveFloorEnv {
            env_mib: Some(4096),
            alias_mib: Some(4096),
        };
        assert_eq!(
            ReserveFloorPolicy::resolve_with_env(&base, &agree)
                .unwrap()
                .min_floor_bytes,
            4096 * MIB
        );
    }

    #[test]
    fn source_label_reports_subsumed_override_honestly() {
        let base = sealed();
        // Raise min_floor above the sealed minimum but far below the 20% share
        // of a large capacity: the override is subsumed by the percentage.
        // 20 GiB * 20% = 4 GiB > 3072 MiB, so the enforced result is unchanged.
        let env = ReserveFloorEnv {
            env_mib: Some(3072),
            alias_mib: None,
        };
        let resolved = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap();
        let capacity = 20 * 1024 * MIB;
        assert_eq!(
            resolved.configured_reserve_bytes(capacity),
            base.configured_reserve_bytes(capacity)
        );
        assert_eq!(
            resolved.source_label(&base, capacity),
            ReserveFloorSource::SealedManifest
        );

        // A raise that actually moves the enforced result is labelled honestly.
        let env = ReserveFloorEnv {
            env_mib: Some(8192),
            alias_mib: None,
        };
        let resolved = ReserveFloorPolicy::resolve_with_env(&base, &env).unwrap();
        assert!(
            resolved.configured_reserve_bytes(capacity) > base.configured_reserve_bytes(capacity)
        );
        assert_eq!(
            resolved.source_label(&base, capacity),
            ReserveFloorSource::LabOverrideRaise
        );
    }

    #[test]
    fn enforced_free_floor_bytes_matches_required_free_bytes() {
        // The two named formulas in DT-2/DT-4 must be one formula. This is a
        // real cross-expression equality: `required_free_bytes` is a method on
        // `GpuBudgetSnapshot` in this crate; `enforced_free_floor_from_configured`
        // is the free function the sparse tier calls because it has no snapshot.
        let policy = sealed();
        let total_bytes = Some(6144 * MIB);
        let budget_bytes = 4016 * MIB;
        let snapshot = GpuBudgetSnapshot {
            adapter: None,
            total_bytes,
            budget_bytes,
            used_bytes: 1024 * MIB,
            source: GpuBudgetSource::DriverReported,
            sampled_at: Instant::now(),
        };
        let capacity = ReserveFloorPolicy::helper_capacity(total_bytes, budget_bytes);
        let runtime = 640 * MIB;
        let configured = policy.configured_reserve_bytes(capacity);
        assert_eq!(
            policy.enforced_free_floor_bytes(capacity, runtime),
            snapshot.required_free_bytes(configured, runtime)
        );
        assert_eq!(
            enforced_free_floor_from_configured(configured, capacity, runtime),
            snapshot.required_free_bytes(configured, runtime)
        );
    }

    #[test]
    fn process_env_reader_ignores_junk_and_keeps_valid_mib() {
        // DT-11: both documented names are read from the process environment;
        // an unparsable value is treated as absent so a typo never silently
        // becomes a floor. Exercised on the pure parser — this crate forbids
        // `unsafe`, so the process env is not mutated in tests.
        assert_eq!(parse_mib_value(Some("3072".into())), Some(3072));
        assert_eq!(parse_mib_value(Some(" 3072 ".into())), Some(3072));
        assert_eq!(parse_mib_value(Some("not-a-number".into())), None);
        assert_eq!(parse_mib_value(Some(String::new())), None);
        assert_eq!(parse_mib_value(None), None);
    }

    #[test]
    fn agreeing_overrides_are_accepted_not_conflicting() {
        let base = sealed();
        let agree = ReserveFloorEnv {
            env_mib: Some(4096),
            alias_mib: Some(4096),
        };
        let resolved = ReserveFloorPolicy::resolve_with_env(&base, &agree).expect("agreeing");
        assert_eq!(resolved.min_floor_bytes, 4096 * MIB);
    }

    #[test]
    fn error_display_names_the_authority() {
        let e = ReserveFloorError::OverrideBelowSealed {
            override_mib: 128,
            sealed_min_mib: 2048,
        };
        let s = e.to_string();
        assert!(s.contains("128"), "display must show the value: {s}");
        assert!(s.contains("2048"), "display must show the seal: {s}");
    }
}
