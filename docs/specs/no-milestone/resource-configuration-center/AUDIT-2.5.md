# AUDIT-2.5 — resource-configuration-center

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Medium | §DT-12, §Atomicity and rollback, §Required tests matrix | Native Linux file origins introduce a new daemon identity/open path. The current WSL origin reader is sealed to a block device, so treating an arbitrary file as equivalent would weaken provenance or risk replacing an existing origin. | The SPEC defines a separate native manifest, binds filesystem/device/mount/path/inode/size, validates the open fd before serving, leaves the WSL v3 block manifest unchanged, and requires identity-drift and lifecycle-selection tests before implementation can advance. Keep native live qualification separate from WSL qualification. |
| Medium | §DT-14..DT-16, §DT-21, §Atomicity and rollback | A storage benchmark can outlive its CLI deadline when a kernel/filesystem operation is uninterruptible. A timeout alone cannot prove that writes stopped or that cleanup is safe. | The SPEC caps each sample at exactly 32 MiB of payload, uses one outstanding operation and one worker, persists a lease with process start identity and exact target, and blocks later disk writes to that target until process exit and cleanup are proven. Timeout produces no ranking. Implement and test this recovery path before enabling the benchmark. |
| Low | §DT-7, §RF-13 | Filesystem-specific swapfile behavior differs, and no in-repository evidence qualifies every Linux filesystem. | V1 permits only tested ext4/XFS paths; all other filesystems remain visible and read-only with a reason. Add other filesystems only through a separate evidence-backed decision. |
| Low | §DT-19 | The shared 10 GiB free-space floor is intentionally conservative and is sourced from the existing WSL origin manager. Native Linux has no qualified lower reserve policy in this tree; this floor may make a low-free-space volume unavailable even when a smaller target would fit. | Preserve the non-overridable floor for initial implementation, expose it in the plan, and refuse rather than silently reducing it. Do not call it a detected capacity or user-selected size. Recalibration requires native Linux filesystem qualification and new evidence. |
| Low | §DT-10..DT-11 | WSL swap settings are per Windows user's global WSL2 configuration and become active only at a later WSL start. | The SPEC routes writes through the Windows provider, shows affected distributions and exact changes, reports `pending_wsl_restart`, and never shuts WSL down. Verify this in the Windows manufactured suite and a separate attended WSL2 E2E. |
| Low | §Audit frontier, §Security checklist | A successful file/config write without a durable intent/result event could be reported as untracked success; a log failure after a write cannot safely be rolled back over changed state. | The SPEC requires a synced intent before the first write and a synced result before success. If result logging fails, retain owned state and report `manual_recovery_required`. Add a refusal test proving no mutation occurs without durable intent. |

No high-severity or hard-no-go finding remains in the design. The review is against the current repository sources, including the existing `FileOrigin`, WSL sealed block-origin path, WSL origin manager reserve, GPU budget owner, `.wslconfig` owner, and Linux cascade lifecycle. The new native origin provider and config UI do not exist yet.

## Open questions

- No product-contract question blocks implementation. Native Linux and WSL2 are separate providers behind one CLI; WSL host memory remains read-only, and native Linux is not routed through Windows disk or swap policy.
- Native Linux ext4/XFS, Windows volume behavior, storage-ranking stability, GPU adapters, and WSL host/guest application remain environment-bound implementation and release gates. Passing manufactured tests alone cannot close them.
- A filesystem call may remain uninterruptible past the displayed benchmark deadline. The UI must say so; persisted worker state prevents a second operation from being admitted to the affected target until recovery proves exit and cleanup.

## Verdict

**go — Step 3 implementation only.** The native Linux and WSL2 contracts are distinct, the selected sizes remain variable and bounded by current measurements, and unsafe storage, stale telemetry, unsupported providers, and uncertain cleanup fail closed. This verdict does not mean the feature is implemented, tested, installed, or release-qualified. Do not mark the spec `DONE` until its named tests, coverage gate, and separate native Linux and WSL2 before/action/after E2E gates pass.
