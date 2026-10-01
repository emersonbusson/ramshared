# SPEC — wsl2-cascade-boot

> Passo 2 SSDV3 against `PRD.md` **revision 2** (2026-09-30). Rewritten in place
> (no `SPECvN.md`): revision 1 of this file described the retired
> `scripts/safety/*.sh` orchestrator and is superseded. Zero criatividade fora
> deste SPEC.

## Closed scope

**In now**

- Native in-program bootstrap command in the CLI that performs `PRD.md` §3
  steps 1–5 with one invocation (RF-7).
- Identity gate: `BINARY_MATCH` between the running CLI, the running daemon and
  one sealed release (RF-2, RF-8).
- Host-prerequisite gate: sealed origin attached, Guardian `HEALTHY` with a
  fresh `boot_id`, host-gate `NORMAL_BOOT` inside a freshness window (RF-2).
- Dirty-state refusal: ghost/deleted managed swap, live orphan NBD/zram, unknown
  half-state (RF-2).
- Version-scoped activation approval that names the **running** release (RF-9).
- Sizing from `/etc/ramshared/cascade.conf` (RF-4).
- Thin systemd unit as trigger/lifecycle boundary only (RF-1, RF-3, NFR-2,
  NFR-5).
- Opt-in enable of that unit (RF-1).
- Display-honesty regression coverage for every user-visible surface (RF-10).
- Human documentation for daily use, demotion cost, and legitimate `Off` (RF-6).

**Out now**

- Real Windows host driver; CoCo (SEV-SNP/TDX/Arm CCA) transitions; multi-vendor
  GPU qualification (`PRD.md` §12).
- Automatic enable without opt-in.
- ublk as the boot path (and the ublk `ramsharedd.service` lab unit).
- Any promise of zero latency under WDDM reclaim.
- Redesign of the sealed NBD lower-sink binding
  (`NBD_LOWER_SINK*` in `scripts/safety/cascade.conf.example`) — owned by
  `docs/specs/no-milestone/wsl2-nbd-product-readiness/`.
- Removal of the attended operators' tools
  (`scripts/safety/nbd-product-preflight.sh`, `cascade-up.sh`,
  `cascade-controller.sh`). They leave the **boot** path only.

**Assumed-ready dependencies (codebase anchors)**

| Dependency | Anchor |
| --- | --- |
| Idempotent `up` + half-state refusal | `crates/ramshared-cli/src/cascade/mod.rs` :: `cascade_already_healthy`, `refuse_half_cascade` |
| Ordered `down` (swapoff → nbd disconnect → daemon) | `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `down_with_runtime`, `down` |
| Sealed-origin binding + manifest digest | `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `sha256_file`, `canonical_sha256`, `expected_manifest_sha256` |
| Product readiness policy (incl. `BINARY_MATCH`, approval) | `crates/ramshared-tier/src/nbd_readiness.rs` :: `evaluate_product`, `RefusalCode`, `Approval`, `Gate` |
| Absorbed host gates | `crates/ramshared-wsl2d/src/host_gate.rs` :: `validate_origin_manifest`, `check_guardian_health`, `evaluate_safe_mode`, `mint_lease` |
| Lifecycle / protection derivation | `crates/ramshared-cli/src/cascade/lifecycle.rs` :: `derive_lifecycle`, `protection_state`, `overall_state` |
| Display honesty (post-`cb39389a`) | `crates/ramshared-cli/src/monitor.rs` :: `draw_dashboard` + `dashboard_reports_protection_off_when_cascade_is_disabled`, `dashboard_blocks_stale_active_state_without_live_daemon` |
| Sealed release identity files | `RELEASE_VERSION`, `SOURCE_COMMIT`, `SHA256SUMS` under the selected release root |

**Accepted boundaries (do not "fix" these later as bugs)**

- Attended `sudo ramshared up` is **not** covered by the boot gates. That is
  deliberate (`PRD.md` §8 keeps `up` as an attended command). Do not retrofit
  the approval token onto `up`; that would break the attended path.
- `up_with_config` keeps its internal `refuse_half_cascade` as a backstop.
  `boot`'s own dirty-state result is the one surfaced to `status` (NFR-6).
- Gate parity with `scripts/safety/nbd-product-preflight.sh` is maintained by
  reusing the same policy types (`nbd_readiness::RefusalCode`,
  `host_gate::*`), and is proved by keeping
  `scripts/safety/test-nbd-product-preflight.sh` green after the unit change —
  not by a second boot-time gate implementation (DT-2).

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-4 (opt-in install), ITEM-4 (unit `WantedBy`, never auto-enabled) |
| RF-2 | ITEM-2 (gates), ITEM-3 (`boot` orchestrator) |
| RF-3 | ITEM-4 (`ExecStop` = `ramshared down`; `KillMode=process`, `SendSIGKILL=no`) |
| RF-4 | ITEM-1 (`/etc/ramshared/cascade.conf` loader) |
| RF-5 | ITEM-3 (`boot` delegates to the existing idempotent `up`) |
| RF-6 | ITEM-6 (human docs) |
| RF-7 | ITEM-3 (`ramshared boot`) |
| RF-8 | ITEM-2 (identity gate, no deploy code), ITEM-5 (`boot_never_mutates_product_binaries`) |
| RF-9 | ITEM-2 (approval parse + version equality), ITEM-5 (stale-approval refusal test) |
| RF-10 | ITEM-5 (display honesty matrix) |
| NFR-1 | ITEM-2 (every gate fail-closed to `Off`) |
| NFR-2 | ITEM-4 (`TimeoutStopSec=infinity`, see DT-5) |
| NFR-3 | ITEM-4 (opt-in only; forensic install never enables) |
| NFR-4 | N/A — repository host-safety rule, not this slice |
| NFR-5 | ITEM-4 (`TimeoutStartSec=120`), ITEM-3 (`boot` has no retry loop) |
| NFR-6 | ITEM-3 (refusal reason → `status` + TUI + journal), ITEM-5 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | The native entrypoint is **`ramshared boot`** (new top-level verb). `up --bootstrap` is rejected. | `PRD.md` §8 leaves the name open. A dedicated verb keeps `up`'s contract unchanged (§8 "Unchanged main CLI surface"), gives RF-8 one isolated code path to test for "never mutates binaries", and makes `ExecStart=ramshared boot` read as the single orchestrator of §3 rather than a mode of the attended `up`. |
| DT-2 | `boot` evaluates its gates **in-process** using `ramshared-tier::nbd_readiness::evaluate_product`, `ramshared-wsl2d::host_gate::*` and the existing `cascade_already_healthy` / `refuse_half_cascade` predicates. It does **not** shell out to `scripts/safety/nbd-product-preflight.sh`. | `PRD.md` §3: "One primary path: the CLI command … no dual orchestrator." The shell preflight remains the **attended** read-only tool (`PRD.md` §8). Every gate it owns already exists in Rust (`RefusalCode`, `Gate`, `evaluate_product`); the host gates were deliberately absorbed into `host_gate.rs`. A second boot-time gate implementation is the dual path Day-0 forbids. |
| DT-3 | Sizing authority is **`/etc/ramshared/cascade.conf`** → `RAMSHARED_VRAM_MIB` / `RAMSHARED_ZRAM_MIB` / `MIN_VRAM_HEADROOM_MIB` env → built-in **1024 / 1024 / 256**. The sealed `scripts/safety/cascade.conf.example` is **never** read for sizing by `boot`. | `PRD.md` §7 + RF-4 name `/etc/ramshared/cascade.conf` and "conservative default 1024/1024". The sealed example's `VRAM_MIB`/`ZRAM_MIB` lines are a second source of the same numbers — the exact dual-default `wsl2-cascade-boot` rev 1 warned against. The sealed file keeps only the machine binding keys (`NBD_LOWER_SINK*`), which `nbd-product-preflight.sh` still owns. |
| DT-4 | Activation approval is a version-scoped token with wire format `activate:<release>[:vram=<n>:zram=<n>]`, delivered as `RAMSHARED_NBD_LIFECYCLE_APPROVAL` or as `/var/lib/ramshared/approvals/activate-<release>.token`. `boot` accepts it **only** when the `<release>` field equals the running sealed release version; anything else is `APPROVAL_STALE_VERSION` (never `Present`). | RF-9 "generated for the running release or refused; a pinned old version is never accepted" and `PRD.md` §3 "no version-skewed drop-in approvals". Version **equality to the running release** is the property, so a drop-in cannot skew either. The wire format is already what `scripts/safety/cascade-up.sh` mints (`EXPECTED_APPROVAL="activate:$RELEASE_VERSION"`), so attended tooling stays compatible. |
| DT-5 | The unit keeps `KillMode=process`, `SendSIGKILL=no`, `TimeoutStopSec=infinity`, `TimeoutStartSec=120`. | Stronger than `PRD.md` NFR-2's "for example, 600 s", and required by RF-3 / §3.5 ("never `kill -9` while nbd appears in `/proc/swaps`") and by the 2026-09-23 outage lesson (§2.1). A stop that exceeds any finite window would let systemd escalate into a ghost swap. The **start** side is bounded (NFR-5); the stop side is intentionally not. |
| DT-6 | Forbidden green strings (`ACTIVE`, `ARMED`, `OPERATIONAL`, `Protection: ACTIVE`) are named constants shared by the renderer and the tests, not free text scattered in `format!`. | RF-10 must fail CI on any hardcoded green (`PRD.md` §15 #1 abort). Constants make the abort executable: a test asserts the renderer emits a constant only when live state proves it, and asserts the constant is absent otherwise. |
| DT-7 | `boot` has no deploy, copy, build, or write-to-product-binary API. `boot --check` / any future deploy-shaped flag is refused before any filesystem write. | RF-8 and Kahneman #16 ("Any `cp`/write to product binaries at boot → rollback trigger"). The 2026-09-21 `455565db` outage was exactly a boot-time `cp target/release/*` into `/usr/local/bin`. Absence of the API is not enough on its own — a named fixture test proves the property survives later edits. |

## Atomicity and rollback

- **Atomicity frontier.** `boot` is a gate-then-act command: all gates
  (identity, approval, host prerequisites, dirty state) run to completion
  **before** the first mutation. The mutation phase is exactly the existing
  `up_with_config` sequence, which already owns its own atomicity. A gate
  failure performs **zero** writes outside `/run/ramshared/` bookkeeping and a
  journal line.
- **Rollback — userspace/daemon.** `boot` writes no daemon state of its own.
  If `up_with_config` fails partway, the existing `up` error path is the
  rollback; `boot` does not add a second one. Refusal leaves the system
  exactly `Off` (RF-2).
- **Rollback — kernel/module.** N/A — this slice loads no LKM.
- **Rollback — host/persistent.** The unit file, `/etc/ramshared/cascade.conf`
  and `/var/lib/ramshared/approvals/*` are installed by the **attended** opt-in
  installer (`scripts/safety/install-cascade-boot.sh`), not by `boot`.
  Rollback of enable is `systemctl disable --now ramshared-cascade` +
  `ramshared down` + removal of the approval token — operator steps, already
  documented in `scripts/safety/uninstall-cascade-boot.sh`.
- **Forward-only?** No. Every artifact this slice creates is removable by the
  attended uninstaller.

**Rollback trigger (structural).** If after enabling `ramshared-cascade.service`
any session shows ghost swap, an orphan NBD/zram device, or a WSL hard-freeze
attributable to the boot path: `systemctl disable --now ramshared-cascade`,
`sudo ramshared down`, append to `validation.md`, and revert the unit to
disabled-by-default. Same trigger if any `boot` code path is found writing to a
product binary path.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-3 identity gate | #16 From exhaustion | What did the 2026-09-21/23 outages teach that this path must not repeat? | `cargo test -p ramshared-cli boot_never_mutates_product_binaries` and `boot_refuses_deploy_arguments` | Any write to a product binary path from `boot` → rollback trigger |
| ITEM-3 refusal | #1 Record the state (WYSIATI) | Is the refusal visible, or silent? | `cargo test -p ramshared-cli boot_records_refusal_reason_and_leaves_state_off`; `ramshared status` after a forced refusal | Any refusal that does not appear in `status` + journal |
| ITEM-3 idempotent start | #17 Idempotence | Does a second `boot` on a healthy cascade do nothing? | `cargo test -p ramshared-cli boot_is_idempotent_when_cascade_already_healthy` | Any second activation that creates a second stack |
| ITEM-2 approval | #13 Refusal + legitimate path | Can the safe path also succeed? | `cargo test -p ramshared-cli scoped_approval_accepts_only_the_running_release_version` (matching version → `Present`) | Permanent refusal of a matching approval → fix the gate, do not bypass it |
| ITEM-2 host prerequisites | #5 Worst case / real load | What happens when origin/Guardian are missing at boot? | `cargo test -p ramshared-cli boot_requires_fresh_host_prerequisites`; one live reboot with origin detached → reason recorded, state exactly `Off` | Any dirty swap left behind → `wsl --shutdown` recovery, no retry |
| ITEM-4 stop order | #5 Worst case | Can stop hang the box? | `cargo test -p ramshared-cli down_with_runtime_preserves_swapoff_first_and_cleans_temp_state`; live `systemctl stop` with real swap load | `kill -9` while nbd appears in `/proc/swaps` |
| ITEM-5 display | #1 Record the state | Is the dashboard describing reality? | `cargo test -p ramshared-cli dashboard_reports_protection_off_when_cascade_is_disabled` and `dashboard_blocks_stale_active_state_without_live_daemon` | Any hardcoded green string → CI fails |

## Security checklist (pre-impl)

- [x] **Privilege:** `boot` runs as root via the unit (existing `User=root`); it opens no new device nodes and issues no new `ioctl`. Capability surface is unchanged from `up`. N/A to `capable()` — userspace, not LKM.
- [x] **User/host copy:** `boot` reads three config files and one env var, all with length caps and strict integer parsing (`DT-3`); no `__user` pointers. Operates on owned `String` copies.
- [x] **Flags/args:** unknown `boot` options are refused (`CliParseError::InvalidOption`) before any gate runs. Approval wire format is strictly parsed; unknown verbs or fields → refuse.
- [x] **Token trust (approval file):** a token at `/var/lib/ramshared/approvals/` that is not root-owned, or is group/world-writable, is `ApprovalUntrusted` — never `Present`. Boundary-refusal pair: `boot_refuses_untrusted_approval_token` (#13) with the legitimate path `scoped_approval_accepts_only_the_running_release_version`.
- [x] **Info-leak:** refusal reasons are stable tokens (`APPROVAL_STALE_VERSION`, `BINARY_MATCH_FAILED`, …), never paths containing hostnames, never kernel addresses.
- [x] **IRQ/atomic / IRQL:** N/A — userspace.
- [x] **Lifetime:** N/A — no map/unmap, no get/put. `down_with_runtime` already owns daemon teardown ordering.
- [x] **Hot-unplug / device-gone:** the identity gate treats an absent daemon as `Gate::NotApplicable` / `BINARY_MATCH_REQUIRED` (already modelled in `nbd_readiness`), never as success.
- [x] **Host safety:** `boot` runs no pressure workload and no swap/ublk stress. NFR-4 stays a repository rule.
- [x] **Shared-hardware cushion:** `MIN_VRAM_HEADROOM_MIB` (default 256) is enforced by the existing `check_safety_net`; `boot` does not lower it. Raise-only semantics stay as in `scripts/safety/test-preflight-reserve-floor.sh`.
- [x] **Bounded foreign calls:** `boot` issues no driver `ioctl` and spawns no unbounded child. `down_with_runtime` keeps its existing timeout (`down_timeout_preserves_runtime_evidence_and_refuses_success`).
- [x] **Cooperative cascade spillover:** unchanged — disk/VHDX tier stays at priority −2. N/A to this slice's new code.
- [x] **Replayable ops:** `boot` is idempotent (RF-5) and every refusal is deterministic (Kahneman #15/#17): no retry loop, no state mutated on refusal.

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-cli/src/cascade/boot.rs`**
- Purpose: the native bootstrap (RF-7). Owns config loading (RF-4), the gate
  sequence of `PRD.md` §3 steps 1–4, and the refusal surface (RF-2, NFR-6).
- RF / DT: RF-2, RF-4, RF-5, RF-7, RF-8, RF-9, NFR-1, NFR-5, NFR-6 · DT-1..DT-4, DT-7
- Types / fns:
  - `pub struct BootConfig { pub vram_mib: u64, pub zram_mib: u64, pub min_vram_headroom_mib: u64 }`
  - `pub fn load_boot_config_from(path: &Path, env: &dyn Env) -> Result<BootConfig, BootError>`
    (pure over an injected env seam; falls back to 1024/1024/256)
  - `pub struct ScopedApproval { pub verb: String, pub release: String, pub vram_mib: Option<u64>, pub zram_mib: Option<u64> }`
  - `pub fn parse_scoped_approval(raw: &str) -> Result<ScopedApproval, BootError>`
    (wire format `activate:<release>[:vram=<n>:zram=<n>]`)
  - `impl ScopedApproval { pub fn matches_release(&self, release: &str) -> bool }`
  - `fn approval_token_is_root_owned(path: &Path, meta: &Metadata) -> bool` — a
    token that is group/world-writable, or not owned by root, is refused as
    `ApprovalUntrusted` (never treated as `Present`)
  - `fn verify_host_lease(path: &Path, now: SystemTime) -> Result<(), BootError>` —
    reads `/run/ramshared/host-resume-lease.json` and rejects it when
    `host_gate::lease_expired` says so. `boot` **verifies** the lease; it never
    calls `host_gate::mint_lease` (minting is `ramshared-host-gate.service`).
  - `pub enum BootError { ConfigInvalid(String), ApprovalMissing, ApprovalStaleVersion { found: String, running: String }, ApprovalUntrusted(&'static str), Identity(RefusalCode), HostPrerequisite(&'static str), DirtyState(&'static str), DeployRefused }`
  - `pub fn boot() -> Result<(), BootError>` — gate order: identity → approval
    (incl. token trust) → host prerequisites (incl. lease freshness) → dirty
    state → `up_with_config`. No retry loop. `up_with_config` keeps its own
    `refuse_half_cascade` backstop; `boot`'s dirty-state result is the one
    surfaced to `status` (NFR-6).
  - `fn assert_no_product_binary_mutation(path: &Path) -> bool` — test seam used
    by `boot_never_mutates_product_binaries`
- Reference pattern in this repo: `crates/ramshared-cli/src/cascade/mod.rs`
  (`default_mb_from_env`, `refuse_half_cascade`) for pure helpers + test seams;
  `crates/ramshared-tier/src/nbd_readiness.rs` for the `RefusalCode` vocabulary.
- Required tests: see matrix rows 1–13.
- Cover target: **≥80%**
- Kahneman: #1, #5, #13, #16, #17

**`docs/specs/no-milestone/wsl2-cascade-boot/AUDIT-2.5.md`** (re-issue)
- Purpose: step 2.5 audit against **this** SPEC (rev-1 audit targeted the
  retired script design). `PRD.md` §10.1 requires both.
- RF / DT: process gate, no RF
- Types / fns: N/A
- Reference pattern: `docs/SSDV3-PROMPTS.md` STEP 2.5 skeleton
- Required tests: N/A
- Cover target: `N/A — documentation`
- Kahneman: N/A

### MODIFY

**`crates/ramshared-cli/src/cascade/mod.rs`**
- What: `pub mod boot;` declaration; move the sizing note on
  `default_mb_from_env` off the stale rev-1 ITEM-4 reference; re-export
  `BootConfig` / `BootError` at the cascade root.
- RF / DT: RF-4 · DT-3
- Symbol: `default_mb_from_env` (doc comment only) · before: "SPEC: … ITEM-4"
  (rev 1) → after: "Sizing authority is `/etc/ramshared/cascade.conf`
  (`boot::load_boot_config_from`); this env helper is the `up` argv path only."
- Callers / docs: none change behaviour.
- Required tests: `crates/ramshared-cli/src/cascade/mod.rs` ::
  `default_mb_from_env_uses_injected_value_or_fallback` (already exists, must
  stay green).
- Cover target: ≥80% (already mapped, `cascade-lifecycle-observability`)

**`crates/ramshared-cli/src/main.rs`**
- What: add `CliCommand::Boot` to the enum at `main.rs:202`, its argv parser,
  and its dispatch to `cascade::boot::boot`. Usage text gains `boot`.
- RF / DT: RF-7, NFR-6 · DT-1
- Symbol: `enum CliCommand` · before: 16 variants · after: 17 variants (`Boot`)
- Callers / docs: `print_usage`/help text; `crates/ramshared-cli/tests/cli_dispatch.rs`.
- Required tests: `crates/ramshared-cli/src/main.rs` ::
  `boot_command_parses_and_dispatches`, `boot_command_rejects_unknown_options`
- Cover target: ≥80% (already mapped, `cascade-lifecycle-observability`)

**`crates/ramshared-cli/src/cascade/lifecycle.rs`**
- What: RF-10 covers **both** user-visible status surfaces. Add the two named
  honesty tests for the plain-text and `--json` `status` output (the TUI ones
  already exist in `monitor.rs`).
- RF / DT: RF-10, NFR-6 · DT-6
- Symbol: test module only · before: `status_off_does_not_attribute_preexisting_disk_pages_to_ramshared`
  and siblings · after: those plus `status_text_reports_off_and_blocked_without_live_daemon`
  and `status_json_never_publishes_active_without_live_daemon`
- Callers / docs: none (no production behaviour change; the renderer already
  derives from live state after `cb39389a`).
- Required tests: the two named above (matrix rows 16–17).
- Cover target: ≥80% (already mapped, `cascade-lifecycle-observability`)

**`scripts/safety/systemd/ramshared-cascade.service`**
- What: replace the script orchestrator with the CLI bootstrap. `ExecStart` and
  `ExecStartPre` change; stop-safety flags are kept (DT-5).
- RF / DT: RF-1, RF-2, RF-3, NFR-2, NFR-5 · DT-2, DT-5
- Symbol (file body):
  - before: `ExecStartPre=…/nbd-product-preflight.sh --check`,
    `ExecStart=…/cascade-controller.sh --execute`, `Documentation=file://…nbd-product-preflight.sh`
  - after: `ExecStart=<product-root>/current/bin/ramshared boot`,
    `ExecStop=<product-root>/current/bin/ramshared down`,
    `Documentation=file://<product-root>/current/README.md`,
    kept: `Type=simple`, `User=root`, `KillMode=process`, `SendSIGKILL=no`,
    `TimeoutStartSec=120`, `TimeoutStopSec=infinity`, `Restart=on-failure`,
    `RestartSec=5`, `ConditionPathExists=!/var/lib/ramshared/safe-mode.json`,
    `After=ramshared-host-gate.service`, `Requires=ramshared-host-gate.service`.
    `ConditionPathExists=/run/ramshared/host-resume-lease.json` is **removed**:
    the lease is a host prerequisite the bootstrap verifies and reports
    (NFR-6), not a silent unit-level skip. `TimeoutStopSec=infinity` is
    deliberate (DT-5); `ExecStop` must journal `NBD_STOP_WINDOW=unbounded` on
    entry so a long swapoff is expected rather than mysterious (NFR-6).
- Callers / docs: `scripts/safety/test-control-plane-units.sh:77`,
  `scripts/safety/test-nbd-product-preflight.sh` (`sealed_nbd_bundle_and_lifecycle_wiring`).
- Required tests: those two named shell assertions, updated to the new
  `ExecStart`/`ExecStop` pair (see matrix row 19).
- Cover target: `N/A — E2E-only` (systemd unit; proved by the wiring test +
  live reboot rounds)

**`scripts/safety/test-control-plane-units.sh`**
- What: the ExecStart assertion at line 77 follows the new unit body.
- RF / DT: RF-1 · DT-1
- Required tests: the script itself (`test_control_plane_units`).
- Cover target: `N/A — E2E-only`

**`scripts/safety/test-nbd-product-preflight.sh`**
- What: `sealed_nbd_bundle_and_lifecycle_wiring` (≈ lines 1305–1315) asserts the
  old `ExecStart`/`ExecStartPre` pair and `TimeoutStopSec=infinity`. Update the
  expected strings to the new `ExecStart=<product-root>/current/bin/ramshared
  boot` + `ExecStop=…/ramshared down` pair. The existing negative assertions
  stay: the unit must still **not** contain `/etc/ramshared/cascade.conf` (the
  CLI reads it, the unit never names it) and must still **not** contain
  `ramsharedd.service`.
- RF / DT: RF-1, RF-3 · DT-2, DT-3, DT-5
- Required tests: `sealed_nbd_bundle_and_lifecycle_wiring`.
- Cover target: `N/A — E2E-only`

**`scripts/safety/install-cascade-boot.sh`**
- What: after a successful attended install of release `<v>`, write
  `/var/lib/ramshared/approvals/activate-<v>.token` with body
  `activate:<v>` (0400, root). Generation is version-scoped to the release just
  installed (RF-9). Any previously present token for a **different** version is
  removed, not left behind. No `systemctl enable` is added (RF-1 stays opt-in;
  the script already refuses `systemctl enable`).
- RF / DT: RF-1, RF-9 · DT-4
- Required tests: `scripts/safety/test-nbd-product-preflight.sh` ::
  `sealed_nbd_bundle_and_lifecycle_wiring` (installer must still refuse
  `systemctl enable` and `cargo build`) plus a new assertion that the installer
  writes exactly one `activate-<v>.token`.
- Cover target: `N/A — E2E-only` (attended installer)

**`scripts/safety/uninstall-cascade-boot.sh`**
- What: also remove `/var/lib/ramshared/approvals/` (all tokens). Leaves
  `/etc/ramshared/cascade.conf` in place (already documented).
- RF / DT: RF-9 · DT-4
- Required tests: the script itself.
- Cover target: `N/A — E2E-only`

**`docs/governance/rust-slice-coverage.json`**
- What: add one `rust-line-coverage` entry so `crates/ramshared-cli/src/cascade/boot.rs`
  is not a `changed-rust-file-unmapped` hit.
- RF / DT: process gate · DT-1
- Symbol: new entry
  `{ "id": "wsl2-cascade-native-bootstrap", "kind": "rust-line-coverage",
     "spec": "docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md",
     "command": ["node","tools/ci/check-rust-slice-coverage.mjs","-p","ramshared-cli",
       "--files","crates/ramshared-cli/src/cascade/boot.rs","--min","80",
       "--report-json","tmp/wsl2-cascade-native-bootstrap-cov.json"],
     "packages": ["ramshared-cli"],
     "files": ["crates/ramshared-cli/src/cascade/boot.rs"], "min": 80 }`
- Required tests: `node tools/ci/check-rust-slice-coverage.test.mjs` stays
  green; the new entry is planned by `plan-rust-slice-coverage.mjs --all`.
- Cover target: the entry's own `min: 80`

**`README.md`, `docs/FAQ.md`, `docs/OPERATOR-GUIDE.md`, `ROADMAP.md`, `ARCHITECTURE.md`**
- What: RF-6 human voice — what to do daily, what not to do, what demotion
  costs, and why boot may legitimately stay `Off`. "You / your machine", short
  sentences, honest limits (stall ≠ freeze; WDDM reclaim costs ~1.18 s for a 4K
  read; host-side attach/Guardian cannot be eliminated from userspace).
- RF / DT: RF-6 · DT-3
- Required tests: `./scripts/docs-check.sh` (link + governance) ·
  `node tools/ci/check-public-hygiene.mjs --candidate`
- Cover target: `N/A — documentation`

**`docs/reliability/GAP-REGISTER.md`**
- What: the "Legacy WSL2 service handoff" row's close evidence is this SPEC
  rewrite + the qualification listed in `PRD.md` §13. Update the row when the
  step-3 evidence lands (not before).
- RF / DT: process · NFR-6
- Required tests: `node tools/ci/check-gap-register.mjs`
- Cover target: `N/A — documentation`

**`docs/INDEX.md`**
- What: regenerate after the folder's SPEC/IMPL change.
- Command: `node tools/generate-docs-index.mjs` then `--check`.
- Cover target: `N/A — documentation`

**`docs/specs/no-milestone/wsl2-cascade-boot/IMPL.md`**
- What: step 3. Replace the SUPERSEDED rev-1 body with the real
  implementation record against **this** SPEC.
- Cover target: `N/A — documentation`

**`validation.md`**
- What: append the close entry with `BINARY_MATCH` proof of the exact installed
  release, three reboot rounds, and the refusal-reason captures (PRD §13/§14).
- Cover target: `N/A — documentation`

### DELETE

None. `scripts/safety/cascade-controller.sh`, `cascade-up.sh` and
`nbd-product-preflight.sh` leave the **boot** path (the unit no longer calls
them) and stay as attended operators' tools per `PRD.md` §8. Deleting them is
out of this slice's closed scope; the capability map continues to list them.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| Boot verdict (`OK` / `REFUSED`) | journal via the unit (`StandardOutput=journal`) + `ramshared status` | info / stable token |
| Refusal token (`APPROVAL_STALE_VERSION`, `BINARY_MATCH_FAILED`, `BINARY_MATCH_REQUIRED`, `BINARY_MATCH_UNKNOWN`, `HOST_PREREQUISITE_*`, `DIRTY_STATE_*`, `DEPLOY_REFUSED`) | `ramshared status` text + `status --json` (`refusal_reason`) | warn / stable enum string |
| Config actually used (`vram_mib`, `zram_mib`, `min_vram_headroom_mib`, source: `etc` \| `env` \| `default`) | `ramshared status --json` (`boot_config`) | info |
| Approval identity (matched release string; never the token body) | `ramshared status --json` (`approval_release`) | info |
| Tier / protection display | existing `monitor.rs` TUI + `status` | info — derived only from live state (RF-10) |

No new kernel `printk`, no new `/proc` or `/sys` file. Journal lines carry no
host paths beyond `/etc/ramshared/cascade.conf` and no kernel addresses.

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter — one paragraph: `ramshared boot` is the single boot orchestrator; the unit is a trigger. |
| `docs/decisions/ADR-…` | N/A — DTs stay in this SPEC (no cross-slice architectural change). |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter — add "boot refused at start" as a degradation with its visible-refusal contract (NFR-6). |
| `validation.md` | Append on close (three reboot rounds + `BINARY_MATCH`). |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | N/A — no P0 performance claim in this slice. |
| `.claude/rules/*` · `CLAUDE.md` · `AGENTS.md` | N/A — no convention change. |

## Implementation order

1. **ITEM-1** — `boot.rs` types: `BootConfig`, `load_boot_config_from`,
   `ScopedApproval`, `parse_scoped_approval`, `BootError` (DT-3, DT-4). Tests
   1–6 with the code they prove.
2. **ITEM-2** — gate predicates in `boot.rs`: identity via
   `nbd_readiness::evaluate_product`, host prerequisites via `host_gate::*`,
   dirty state via `refuse_half_cascade` / ghost scan, approval version
   equality (DT-2, DT-4, DT-7). Tests 7, 8, 10, 13.
3. **ITEM-3** — `boot()` orchestrator + `main.rs` `CliCommand::Boot` (DT-1).
   Delegates activation to `up_with_config`. Tests 9, 11, 12, 17, 18.
4. **ITEM-4** — thin unit + install/uninstall approval generation + the two
   wiring tests (DT-5). Test 19.
5. **ITEM-5** — display-honesty matrix (DT-6). Tests 14–16.
6. **ITEM-6** — human docs, living docs, `IMPL.md`, `validation.md` entry.

No gaps. Types and the refusal vocabulary before the orchestrator; tests with
the code they prove.

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_config_defaults_to_conservative_1024_1024_when_file_absent` | unit | #3 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_config_reads_vram_zram_and_headroom_from_etc_cascade_conf` | unit | #3 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_config_rejects_non_integer_and_out_of_range_sizes` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `scoped_approval_parses_activate_with_and_without_size_binding` | unit | #9 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `scoped_approval_accepts_only_the_running_release_version` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_refuses_untrusted_approval_token` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_refuses_stale_version_scoped_approval` | unit | #13/#16 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_refuses_missing_approval_before_any_mutation` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_never_mutates_product_binaries` | unit | #16 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_refuses_deploy_arguments` | unit | #16 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_refuses_ghost_or_half_cascade_state` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_is_idempotent_when_cascade_already_healthy` | unit | #17 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_records_refusal_reason_and_leaves_state_off` | unit | #1 | ≥80% |
| `crates/ramshared-cli/src/cascade/boot.rs` | `…/boot.rs` :: `boot_requires_fresh_host_prerequisites` | unit | #5 | ≥80% |
| `crates/ramshared-cli/src/monitor.rs` | `…/monitor.rs` :: `dashboard_reports_protection_off_when_cascade_is_disabled` | unit | #1 | ≥80% (exists) |
| `crates/ramshared-cli/src/monitor.rs` | `…/monitor.rs` :: `dashboard_blocks_stale_active_state_without_live_daemon` | unit | #1 | ≥80% (exists) |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | `…/lifecycle.rs` :: `status_text_reports_off_and_blocked_without_live_daemon` | unit | #1 | ≥80% (new) |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | `…/lifecycle.rs` :: `status_json_never_publishes_active_without_live_daemon` | unit | #1 | ≥80% (new) |
| `crates/ramshared-cli/src/main.rs` | `…/main.rs` :: `boot_command_parses_and_dispatches` | unit | #9 | ≥80% |
| `crates/ramshared-cli/src/main.rs` | `…/main.rs` :: `boot_command_rejects_unknown_options` | unit | #13 | ≥80% |
| `scripts/safety/test-control-plane-units.sh` · `scripts/safety/test-nbd-product-preflight.sh` | `sealed_nbd_bundle_and_lifecycle_wiring` | drill/E2E | #16 | `N/A — E2E-only` |
| `crates/ramshared-cli/src/cascade/mod.rs` | `…/mod.rs` :: `default_mb_from_env_uses_injected_value_or_fallback`, `zram_zero_is_parsed`, `cascade_healthy_requires_vram_swap_record_and_live_daemon_signal`, `ghost_blocks_healthy`, `refuse_half_cascade_when_vram_live_without_health` | unit | #9/#13 | ≥80% (exists) |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `…/cascade_io.rs` :: `down_with_runtime_preserves_swapoff_first_and_cleans_temp_state`, `down_timeout_preserves_runtime_evidence_and_refuses_success` | unit | #5/#17 | ≥80% (exists) |
| live WSL2 host | three reboot rounds: `wsl --shutdown` → start → `cat /proc/swaps` shows three tiers, `ls /dev/nbd*` zero orphans, zero ghost swap; plus one round with origin detached → `Off` + recorded reason | drill/E2E | #3/#5 | `N/A — E2E-only` |
| live WSL2 host | `BINARY_MATCH` of the exact installed release: `sha256sum` of `bin/ramshared` + `bin/ramsharedd` equals `SHA256SUMS` of the selected release | drill/E2E | #16 | `N/A — E2E-only` |

Kinds: unit · integration · kselftest · WDK/SDV/Verifier · drill/E2E.

## Validation checklist

- [ ] `cargo fmt --all -- --check`
- [ ] `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `CARGO_BUILD_JOBS=1 cargo test -p ramshared-cli -- --test-threads=1`
- [ ] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/cascade/boot.rs --min 80`
- [ ] Linux LKM checks: **N/A** — this slice changes no C/Rust kernel code
- [ ] Windows InfVerif / SDV / Driver Verifier / lab VM: **N/A** — no Windows driver surface in this slice
- [ ] Live path for this product surface: `bash -n` on the touched shell scripts;
      `bash scripts/safety/test-control-plane-units.sh`;
      `bash scripts/safety/test-nbd-product-preflight.sh`;
      then the three reboot rounds + the detached-origin round from the matrix
- [ ] Every matrix row has a real test name
- [ ] Kahneman critical rows have executable evidence
- [ ] `./scripts/docs-check.sh`
- [ ] `node tools/ci/check-public-hygiene.mjs --candidate`
- [ ] `node tools/generate-docs-index.mjs --check`
- [ ] `node tools/ci/check-gap-register.mjs`
