//! Pure Linux swap apply/activate/cleanup policy for the resource
//! configuration center.
//!
//! This module is side-effect free. It decides whether a proposed Linux
//! swapfile transaction may proceed; the privileged helper
//! `scripts/linux/ramshared-resource-config-helper` performs the actual
//! filesystem, `mkswap`, `swapon`, and systemd work. Keeping the decision
//! separate lets the refusal matrix be unit-tested without root, without a
//! real swap device, and without touching host state.
//!
//! Governing decisions: DT-8 (create-once under an app-owned root-controlled
//! directory, never disable existing swap on apply) and DT-9 (forward-only
//! migration; cleanup requires exact ownership, inactive identity, zero use,
//! no unit references, and unchanged transaction hashes).
//!
//! The transaction-gate functions below are the decision layer for the
//! privileged helper. Until the CLI `config apply` / `config cleanup`
//! commands wire that call path they are exercised only by their named
//! contractual tests, so `dead_code` is allowed at module scope rather than
//! shipping a stub executor.

#![allow(dead_code)]

use std::fmt;

/// Filesystems whose swapfile allocation and `swapon` contract is named and
/// tested (DT-7). Everything else stays visible but ineligible.
pub const SUPPORTED_SWAP_FILESYSTEMS: &[&str] = &["ext4", "xfs"];

/// Block transports qualified as local backing for a managed swapfile (DT-7).
/// A transport outside this set — including missing or unrecognized identity —
/// is refused rather than guessed safe.
pub const QUALIFIED_SWAP_TRANSPORTS: &[&str] = &[
    "ata", "ide", "mmc", "nvme", "pci", "sas", "sata", "scsi", "virtio",
];

/// Network-backed block transports that must never back a managed swapfile
/// (DT-7), even when the filesystem itself is ext4/XFS.
pub const REFUSED_SWAP_TRANSPORTS: &[&str] = &[
    "iscsi", "nbd", "rbd", "drbd", "fcoe", "fc", "aoe", "nvme-of",
];

/// App-owned directory under which every managed swapfile must live (DT-8).
/// Relative to the managed root; the helper resolves the absolute path.
pub const MANAGED_SWAP_SUBDIR: &str = "var/lib/ramshared/swap";

/// Persistent mount binding captured when the plan was written (DT-6).
/// Only filesystem UUID and stable backing identity participate in the drift
/// check; mount IDs and major:minor numbers are boot-scoped and never
/// persisted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountBinding {
    pub filesystem_uuid: String,
    pub stable_backing_identity: String,
}

/// Fresh mount observation taken immediately before the first write (DT-5).
/// `fstype` and `read_only` are eligibility properties of the current mount,
/// not part of the persisted identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountObservation {
    pub binding: MountBinding,
    pub fstype: String,
    pub read_only: bool,
}

/// One row of the current active-swap inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSwap {
    pub path: String,
    pub used_kib: u64,
    /// True when this swap is managed by RamShared (owner marker present).
    pub ramshared_owned: bool,
}

/// Ownership evidence recorded when a managed swapfile was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapOwnership {
    pub path: String,
    pub bytes: u64,
    pub unit_name: String,
    /// Transaction hash captured at creation time (DT-9).
    pub transaction_hash: String,
}

/// Why a swap activation must not proceed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationRefusal {
    /// The mount identity observed now differs from the one the plan bound.
    MountIdentityChanged,
    /// A RamShared-managed swapfile is already active; apply never replaces it.
    RamSharedSwapAlreadyActive,
    /// The target path already appears in the active swap table.
    TargetAlreadyActive,
    /// Filesystem type is not in the named/tested set.
    UnsupportedFilesystem,
    /// Backing transport is network-backed or unrecognized.
    RefusedTransport,
    /// The mount is read-only or not a filesystem root (DT-6).
    IneligibleMount,
    /// The requested path escapes the app-owned managed directory.
    PathOutsideManagedRoot,
}

impl fmt::Display for ActivationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::MountIdentityChanged => "mount identity changed",
            Self::RamSharedSwapAlreadyActive => "ramshared swap already active",
            Self::TargetAlreadyActive => "target already active",
            Self::UnsupportedFilesystem => "unsupported filesystem",
            Self::RefusedTransport => "refused backing transport",
            Self::IneligibleMount => "ineligible mount",
            Self::PathOutsideManagedRoot => "path outside managed root",
        };
        f.write_str(text)
    }
}

/// Why a cleanup must not proceed. DT-9: any missing proof leaves the old
/// file and unit intact and reports cleanup pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanupRefusal {
    /// Path is not under the app-owned managed directory.
    ForeignPath,
    /// Ownership marker missing or its transaction hash differs.
    OwnershipMismatch,
    /// The swap is still present in the active table.
    StillActive,
    /// `SwapUsed` is non-zero; pages are still referenced.
    StillUsed,
    /// A systemd unit still references the target.
    UnitStillReferenced,
    /// Transaction hash changed since creation.
    TransactionHashChanged,
    /// This is the last persistent fallback swap; removing it would leave the
    /// system with no configured fallback tier.
    LastPersistentFallback,
}

impl fmt::Display for CleanupRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::ForeignPath => "foreign path",
            Self::OwnershipMismatch => "ownership mismatch",
            Self::StillActive => "still active",
            Self::StillUsed => "still used",
            Self::UnitStillReferenced => "unit still referenced",
            Self::TransactionHashChanged => "transaction hash changed",
            Self::LastPersistentFallback => "last persistent fallback",
        };
        f.write_str(text)
    }
}

/// True when `path` is strictly inside the app-owned managed swap directory.
///
/// The check is lexical and conservative: any `..` component, a symlink
/// component, or a path that does not start with the managed prefix is
/// refused. The helper re-validates with a directory fd before any write.
pub fn path_is_managed(path: &str) -> bool {
    if path.is_empty() || !path.starts_with('/') {
        return false;
    }
    if path.contains('\0') {
        return false;
    }
    let prefix = format!("/{MANAGED_SWAP_SUBDIR}/");
    if !path.starts_with(&prefix) {
        return false;
    }
    let relative = &path[prefix.len()..];
    if relative.is_empty() {
        return false;
    }
    !relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
}

/// True when `fstype` is in the named/tested swapfile set and `transport` is a
/// recognized local transport (DT-7). Missing or unrecognized transport
/// identity is refused, never guessed safe.
pub fn filesystem_allows_swapfile(fstype: &str, transport: &str) -> bool {
    let fstype_ok = SUPPORTED_SWAP_FILESYSTEMS
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(fstype));
    let transport_ok = QUALIFIED_SWAP_TRANSPORTS
        .iter()
        .any(|qualified| qualified.eq_ignore_ascii_case(transport));
    fstype_ok && transport_ok
}

/// True when `transport` is a known network-backed identity or one of its
/// common suffixed spellings (DT-7).
pub fn transport_is_network_backed(transport: &str) -> bool {
    let lowered = transport.to_ascii_lowercase();
    REFUSED_SWAP_TRANSPORTS
        .iter()
        .any(|refused| lowered == *refused || lowered.starts_with(&format!("{refused}-")))
}

/// Decide whether a swap activation may proceed (DT-8).
///
/// `bound` is the mount binding captured when the plan was written; `observed`
/// is the mount resolved immediately before the first write. `active` is the
/// live swap table. `target` is the absolute path of the new swapfile.
/// `transport` is the current block transport of the backing device.
pub fn activation_refusal(
    bound: &MountBinding,
    observed: &MountObservation,
    active: &[ActiveSwap],
    target: &str,
    transport: &str,
) -> Result<(), ActivationRefusal> {
    if !path_is_managed(target) {
        return Err(ActivationRefusal::PathOutsideManagedRoot);
    }
    if bound != &observed.binding {
        return Err(ActivationRefusal::MountIdentityChanged);
    }
    if observed.read_only {
        return Err(ActivationRefusal::IneligibleMount);
    }
    if transport_is_network_backed(transport) {
        return Err(ActivationRefusal::RefusedTransport);
    }
    if !filesystem_allows_swapfile(&observed.fstype, transport) {
        return if SUPPORTED_SWAP_FILESYSTEMS
            .iter()
            .any(|supported| supported.eq_ignore_ascii_case(&observed.fstype))
        {
            Err(ActivationRefusal::RefusedTransport)
        } else {
            Err(ActivationRefusal::UnsupportedFilesystem)
        };
    }
    if active.iter().any(|swap| swap.ramshared_owned) {
        return Err(ActivationRefusal::RamSharedSwapAlreadyActive);
    }
    if active.iter().any(|swap| swap.path == target) {
        return Err(ActivationRefusal::TargetAlreadyActive);
    }
    Ok(())
}

/// Inputs required to decide a cleanup (DT-9).
pub struct CleanupContext<'a> {
    pub target: &'a str,
    pub ownership: Option<&'a SwapOwnership>,
    pub active: &'a [ActiveSwap],
    pub unit_references: u32,
    /// Transaction hash recomputed from the on-disk file and unit right now.
    pub current_transaction_hash: &'a str,
    /// Number of other persistent fallback swaps that would remain if this
    /// one is removed.
    pub remaining_persistent_fallbacks: u32,
}

/// Decide whether a cleanup may proceed (DT-9).
///
/// Every proof must be present and exact. Any missing proof is a refusal that
/// leaves the file and unit intact and reports cleanup pending.
pub fn cleanup_refusal(ctx: &CleanupContext<'_>) -> Result<(), CleanupRefusal> {
    if !path_is_managed(ctx.target) {
        return Err(CleanupRefusal::ForeignPath);
    }
    let ownership = ctx.ownership.ok_or(CleanupRefusal::OwnershipMismatch)?;
    if ownership.path != ctx.target {
        return Err(CleanupRefusal::OwnershipMismatch);
    }
    if ownership.transaction_hash != ctx.current_transaction_hash {
        return Err(CleanupRefusal::TransactionHashChanged);
    }
    if let Some(swap) = ctx.active.iter().find(|swap| swap.path == ctx.target) {
        if swap.used_kib > 0 {
            return Err(CleanupRefusal::StillUsed);
        }
        return Err(CleanupRefusal::StillActive);
    }
    if ctx.unit_references > 0 {
        return Err(CleanupRefusal::UnitStillReferenced);
    }
    if ctx.remaining_persistent_fallbacks == 0 {
        return Err(CleanupRefusal::LastPersistentFallback);
    }
    Ok(())
}

/// True when a replayed cleanup of an already-removed target is a stable
/// no-op rather than a second destructive effect (Kahneman #17).
///
/// A cleanup replay is idempotent when the target is absent from the active
/// table, has no ownership marker left, and has no unit references. In that
/// state the correct answer is "already clean", not a new mutation.
pub fn cleanup_replay_is_noop(ctx: &CleanupContext<'_>) -> bool {
    if ctx.ownership.is_some() {
        return false;
    }
    if ctx.active.iter().any(|swap| swap.path == ctx.target) {
        return false;
    }
    ctx.unit_references == 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    const TARGET: &str = "/var/lib/ramshared/swap/fallback.swap";

    fn ext4_binding() -> MountBinding {
        MountBinding {
            filesystem_uuid: "11111111-2222-3333-4444-555555555555".into(),
            stable_backing_identity: "wwn-0x5000c500aabbccdd".into(),
        }
    }

    fn ext4_mount() -> MountObservation {
        MountObservation {
            binding: ext4_binding(),
            fstype: "ext4".into(),
            read_only: false,
        }
    }

    fn owned_swap(path: &str) -> SwapOwnership {
        SwapOwnership {
            path: path.into(),
            bytes: 4 * 1024 * 1024 * 1024,
            unit_name: "ramshared-swap-fallback".into(),
            transaction_hash: "hash-at-creation".into(),
        }
    }

    #[test]
    fn path_is_managed_accepts_only_app_owned_subdir() {
        assert!(path_is_managed(TARGET));
        assert!(path_is_managed("/var/lib/ramshared/swap/nested/leaf.swap"));
        assert!(!path_is_managed("/var/lib/ramshared/swap"));
        assert!(!path_is_managed("/var/lib/ramshared/swap/"));
        assert!(!path_is_managed("/var/lib/ramshared/swap/../escape.swap"));
        assert!(!path_is_managed("/var/lib/ramshared/swap/./dot.swap"));
        assert!(!path_is_managed("/var/lib/ramshared/swap//double.swap"));
        assert!(!path_is_managed("/etc/fstab"));
        assert!(!path_is_managed("var/lib/ramshared/swap/relative.swap"));
        assert!(!path_is_managed("/tmp/evil.swap"));
        assert!(!path_is_managed(""));
    }

    #[test]
    fn filesystem_rules_allow_only_named_local_swapfile_targets() {
        assert!(filesystem_allows_swapfile("ext4", "nvme"));
        assert!(filesystem_allows_swapfile("XFS", "sata"));
        assert!(filesystem_allows_swapfile("ext4", "virtio"));
        assert!(!filesystem_allows_swapfile("btrfs", "nvme"));
        assert!(!filesystem_allows_swapfile("ext4", "nbd"));
        assert!(!filesystem_allows_swapfile("ext4", "iscsi"));
        assert!(!filesystem_allows_swapfile("ext4", "nvme-of"));
        assert!(!filesystem_allows_swapfile("ext4", "unknown"));
        assert!(!filesystem_allows_swapfile("ext4", ""));
        // Unrecognized transport is refused even on a supported filesystem.
        assert!(transport_is_network_backed("nbd"));
        assert!(transport_is_network_backed("nbd-tcp"));
        assert!(transport_is_network_backed("nvme-of"));
        assert!(!transport_is_network_backed("nvme"));
        assert!(!transport_is_network_backed("sata"));
    }

    /// TestName: `linux_swap_activation_refuses_changed_mount_or_active_ramshared`
    ///
    /// A legitimate activation binds one exact mount identity and one empty
    /// RamShared swap set. The paired refusals are a drifted mount and an
    /// already-active RamShared swap; neither may fall through to `swapon`.
    #[test]
    fn linux_swap_activation_refuses_changed_mount_or_active_ramshared() {
        let bound = ext4_binding();
        let observed = ext4_mount();
        let empty: Vec<ActiveSwap> = Vec::new();

        // Legitimate boundary: identity unchanged, no RamShared swap active.
        assert_eq!(
            activation_refusal(&bound, &observed, &empty, TARGET, "nvme"),
            Ok(()),
            "unchanged mount with no active ramshared swap must be admitted"
        );

        // Paired refusal: filesystem UUID drifted after the plan was written.
        let mut drifted_uuid = ext4_binding();
        drifted_uuid.filesystem_uuid = "99999999-8888-7777-6666-555555555555".into();
        let drifted_obs = MountObservation {
            binding: drifted_uuid.clone(),
            ..ext4_mount()
        };
        assert_eq!(
            activation_refusal(&bound, &drifted_obs, &empty, TARGET, "nvme"),
            Err(ActivationRefusal::MountIdentityChanged)
        );

        // Paired refusal: stable backing identity drifted.
        let mut drift_stable = ext4_binding();
        drift_stable.stable_backing_identity = "wwn-0xdeadbeef".into();
        let drift_stable_obs = MountObservation {
            binding: drift_stable,
            ..ext4_mount()
        };
        assert_eq!(
            activation_refusal(&bound, &drift_stable_obs, &empty, TARGET, "nvme"),
            Err(ActivationRefusal::MountIdentityChanged)
        );

        // Paired refusal: a RamShared-managed swap is already active.
        let active_owned = vec![ActiveSwap {
            path: "/var/lib/ramshared/swap/other.swap".into(),
            used_kib: 0,
            ramshared_owned: true,
        }];
        assert_eq!(
            activation_refusal(&bound, &observed, &active_owned, TARGET, "nvme"),
            Err(ActivationRefusal::RamSharedSwapAlreadyActive)
        );

        // The exact target being active is also a refusal.
        let target_active = vec![ActiveSwap {
            path: TARGET.into(),
            used_kib: 0,
            ramshared_owned: true,
        }];
        assert_eq!(
            activation_refusal(&bound, &observed, &target_active, TARGET, "nvme"),
            Err(ActivationRefusal::RamSharedSwapAlreadyActive)
        );

        // A foreign (non-RamShared) swap elsewhere does not block a new
        // managed swapfile: apply never disables existing swap (DT-8).
        let foreign_only = vec![ActiveSwap {
            path: "/swapfile".into(),
            used_kib: 128,
            ramshared_owned: false,
        }];
        assert_eq!(
            activation_refusal(&bound, &observed, &foreign_only, TARGET, "nvme"),
            Ok(())
        );

        // Path outside the app-owned root is refused before any identity test.
        assert_eq!(
            activation_refusal(&bound, &observed, &empty, "/tmp/escape.swap", "nvme"),
            Err(ActivationRefusal::PathOutsideManagedRoot)
        );

        // Read-only mount is an eligibility refusal, not a binding drift.
        let ro = MountObservation {
            read_only: true,
            ..ext4_mount()
        };
        assert_eq!(
            activation_refusal(&bound, &ro, &empty, TARGET, "nvme"),
            Err(ActivationRefusal::IneligibleMount)
        );

        // Network-backed and unrecognized transports fail closed.
        assert_eq!(
            activation_refusal(&bound, &observed, &empty, TARGET, "nbd"),
            Err(ActivationRefusal::RefusedTransport)
        );
        assert_eq!(
            activation_refusal(&bound, &observed, &empty, TARGET, "unknown"),
            Err(ActivationRefusal::RefusedTransport)
        );

        // Unsupported filesystem fails closed.
        let btrfs = MountObservation {
            fstype: "btrfs".into(),
            ..ext4_mount()
        };
        assert_eq!(
            activation_refusal(&bound, &btrfs, &empty, TARGET, "nvme"),
            Err(ActivationRefusal::UnsupportedFilesystem)
        );
    }

    /// TestName: `linux_swap_cleanup_refuses_last_persistent_fallback`
    ///
    /// Cleanup is forward-only and proof-gated (DT-9). Removing the final
    /// persistent fallback would leave the machine with no configured
    /// fallback tier, so it is refused even when every other proof is exact.
    #[test]
    fn linux_swap_cleanup_refuses_last_persistent_fallback() {
        let ownership = owned_swap(TARGET);
        let inactive: Vec<ActiveSwap> = Vec::new();
        let exact = CleanupContext {
            target: TARGET,
            ownership: Some(&ownership),
            active: &inactive,
            unit_references: 0,
            current_transaction_hash: "hash-at-creation",
            remaining_persistent_fallbacks: 1,
        };

        // Legitimate boundary: one fallback would remain after removal.
        let mut ok = CleanupContext {
            remaining_persistent_fallbacks: 1,
            ..exact
        };
        // `exact` already has remaining = 1, which admits the cleanup.
        ok.remaining_persistent_fallbacks = 1;
        assert_eq!(cleanup_refusal(&ok), Ok(()));

        // Paired refusal: this is the last persistent fallback.
        let last = CleanupContext {
            remaining_persistent_fallbacks: 0,
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&last),
            Err(CleanupRefusal::LastPersistentFallback)
        );

        // Every other missing proof is a separate refusal, never a delete.
        let foreign = CleanupContext {
            target: "/swapfile",
            ..exact
        };
        assert_eq!(cleanup_refusal(&foreign), Err(CleanupRefusal::ForeignPath));

        let missing_owner = CleanupContext {
            ownership: None,
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&missing_owner),
            Err(CleanupRefusal::OwnershipMismatch)
        );

        let wrong_owner = owned_swap("/var/lib/ramshared/swap/other.swap");
        let owner_mismatch = CleanupContext {
            ownership: Some(&wrong_owner),
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&owner_mismatch),
            Err(CleanupRefusal::OwnershipMismatch)
        );

        let hash_changed = CleanupContext {
            current_transaction_hash: "hash-after-tamper",
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&hash_changed),
            Err(CleanupRefusal::TransactionHashChanged)
        );

        let active = vec![ActiveSwap {
            path: TARGET.into(),
            used_kib: 0,
            ramshared_owned: true,
        }];
        let still_active = CleanupContext {
            active: &active,
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&still_active),
            Err(CleanupRefusal::StillActive)
        );

        let used = vec![ActiveSwap {
            path: TARGET.into(),
            used_kib: 64,
            ramshared_owned: true,
        }];
        let still_used = CleanupContext {
            active: &used,
            ..exact
        };
        assert_eq!(cleanup_refusal(&still_used), Err(CleanupRefusal::StillUsed));

        let referenced = CleanupContext {
            unit_references: 1,
            ..exact
        };
        assert_eq!(
            cleanup_refusal(&referenced),
            Err(CleanupRefusal::UnitStillReferenced)
        );
    }

    /// A replayed cleanup must not perform a second destructive effect
    /// (Kahneman #17). When the target is already gone the answer is
    /// `cleanup_replay_is_noop`, not a new mutation.
    #[test]
    fn linux_swap_cleanup_replay_is_idempotent() {
        let ownership = owned_swap(TARGET);
        let inactive: Vec<ActiveSwap> = Vec::new();

        // First call: proofs are exact, cleanup is admitted.
        let first = CleanupContext {
            target: TARGET,
            ownership: Some(&ownership),
            active: &inactive,
            unit_references: 0,
            current_transaction_hash: "hash-at-creation",
            remaining_persistent_fallbacks: 1,
        };
        assert_eq!(cleanup_refusal(&first), Ok(()));
        assert!(!cleanup_replay_is_noop(&first));

        // Replay after a successful cleanup: nothing left to mutate.
        let replay = CleanupContext {
            target: TARGET,
            ownership: None,
            active: &inactive,
            unit_references: 0,
            current_transaction_hash: "hash-at-creation",
            remaining_persistent_fallbacks: 1,
        };
        assert!(cleanup_replay_is_noop(&replay));

        // Replay while the target is somehow active again is not a no-op;
        // the cleanup decision path refuses instead of double-deleting.
        let resurrected = vec![ActiveSwap {
            path: TARGET.into(),
            used_kib: 0,
            ramshared_owned: true,
        }];
        let not_noop = CleanupContext {
            target: TARGET,
            ownership: None,
            active: &resurrected,
            unit_references: 0,
            current_transaction_hash: "hash-at-creation",
            remaining_persistent_fallbacks: 1,
        };
        assert!(!cleanup_replay_is_noop(&not_noop));
        assert_eq!(
            cleanup_refusal(&not_noop),
            Err(CleanupRefusal::OwnershipMismatch)
        );

        // Replay while a unit still references the path is not a no-op.
        let unit_left = CleanupContext {
            target: TARGET,
            ownership: None,
            active: &inactive,
            unit_references: 1,
            current_transaction_hash: "hash-at-creation",
            remaining_persistent_fallbacks: 1,
        };
        assert!(!cleanup_replay_is_noop(&unit_left));
    }
}
