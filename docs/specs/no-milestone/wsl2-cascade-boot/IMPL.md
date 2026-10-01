# IMPL — wsl2-cascade-boot

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md` rev 2
> PRD revision 2: native in-program bootstrap (RF-7..RF-10).

## Status

**PARTIAL (code complete, coverage and wiring gates passed; live reboot qualification pending)** · 2026-09-30

| Gate | Result |
| --- | --- |
| `cargo test -p ramshared-cli` | **528** passed (515 unit + 13 integration) |
| `cargo clippy -p ramshared-cli --all-targets -- -D warnings` | **PASS** |
| `cargo fmt --all -- --check` | **PASS** |
| `boot.rs` slice coverage | **84.4%** (465/551) — ≥80% gate **PASS** |
| `lifecycle.rs` slice coverage | **91.9%** (340/370) — ≥80% gate **PASS** |
| `test-control-plane-units.sh` | **PASS** (updated ExecStart/ExecStop assertions) |
| `test-nbd-product-preflight.sh` | **47/47 PASS** (wiring + installer token assertions) |
| Live reboot rounds (×3) + `BINARY_MATCH` | **env-bound** — PARTIAL, not DONE |

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-cli/src/cascade/boot.rs` | ITEM-1..3 / RF-7..RF-9, DT-1..DT-4, DT-7 | `BootConfig`, `BootError`, `ScopedApproval`, `parse_boot_invocation`, `verify_host_lease`, `BootHost` trait, `SystemBootHost`, `boot()` / `boot_with()`. 20 named matrix tests + 15 observer/edge tests. |
| `crates/ramshared-cli/src/main.rs` | ITEM-3 / RF-7, DT-1, DT-7 | `CliCommand::Boot` (17th variant), parse arm rejecting all options, `SystemCliActions::boot` (NFR-6), `RecordingCliActions::boot`, dispatch, usage line. Tests: `boot_command_parses_and_dispatches`, `boot_command_rejects_unknown_options`. |
| `crates/ramshared-cli/src/cascade/mod.rs` | ITEM-1 | `pub use boot::{BootConfig, BootError}` at cascade root. |
| `crates/ramshared-cli/src/cascade/lifecycle.rs` | ITEM-5 / RF-10, DT-6 | Display honesty tests: `status_text_reports_off_and_blocked_without_live_daemon`, `status_json_never_publishes_active_without_live_daemon`. |
| `scripts/safety/systemd/ramshared-cascade.service` | ITEM-4 / RF-1..RF-3, DT-2, DT-5 | `ExecStart=…/ramshared boot`, `ExecStop=…/ramshared down` with `NBD_STOP_WINDOW=unbounded` journal marker. Removed `ExecStartPre` and lease `ConditionPathExists`. |
| `scripts/safety/install-cascade-boot.sh` | ITEM-4 / RF-9, DT-4 | Writes `/var/lib/ramshared/approvals/activate-<v>.token` (0400, root); removes stale tokens for other versions. |
| `scripts/safety/uninstall-cascade-boot.sh` | ITEM-4 / RF-9 | Removes `/var/lib/ramshared/approvals/` entirely. |
| `scripts/safety/test-control-plane-units.sh` | ITEM-4 | Updated ExecStart/ExecStop assertions. |
| `scripts/safety/test-nbd-product-preflight.sh` | ITEM-4 | Updated `sealed_nbd_bundle_and_lifecycle_wiring` assertions; installer token check. |
| `docs/governance/rust-slice-coverage.json` | ITEM-3 | Added `wsl2-cascade-native-bootstrap` entry. |

## Design decisions

| ID | Decision |
| --- | --- |
| DT-1 | `boot` is the single top-level verb. `up --bootstrap` is rejected by the parser. |
| DT-2 | Identity gate evaluates **in-process** via `ramshared_tier::nbd_readiness::evaluate_product`. No shell-outs to `nbd-product-preflight.sh`. |
| DT-3 | Sizing authority: `/etc/ramshared/cascade.conf` → `RAMSHARED_VRAM_MIB`/`RAMSHARED_ZRAM_MIB`/`MIN_VRAM_HEADROOM_MIB` env → built-in 1024/1024/`ramshared_vram::SEALED_RESERVE_MIN_MIB` (2048). `MIN_VRAM_HEADROOM_MIB` is the operator-facing name for the configured GPU free floor; a resolved value below the seal is refused (raise-only, DT-8), never clamped. Sealed `cascade.conf.example` is **never** read for sizing. |
| DT-4 | Approval wire format `activate:<release>[:vram=<n>:zram=<n>]` with version **equality**. Token must be root-owned and not group/world-writable (`uid==0 && mode&0o022==0`). |
| DT-5 | `TimeoutStopSec=infinity`; `ExecStop` journals `NBD_STOP_WINDOW=unbounded` on entry. |
| DT-6 | Display honesty: text and JSON surfaces never claim "active" without a live daemon. |
| DT-7 | `boot` has no deploy API. `parse_boot_invocation` rejects `--install|--upgrade|--deploy|--replace|--release|--force|--uninstall|--release=`. |

## Gate order (fail-closed, no retry loop, NFR-5)

```
gate_no_deploy_api → load_boot_config_from → identity_gate → resolve_approval → verify_host_lease → dirty_gate → activate
```

- **Identity**: `SystemBootHost::observe_product_input` gathers fresh facts (CLI BINARY_MATCH, daemon BINARY_MATCH, release gate, relay gate, lower-tier capacity, lifecycle state, legacy ublk). `evaluate_product` applies policy. Anything unmeasurable is `Gate::Unknown` → refusal (Kahneman #1).
- **Approval**: version-scoped token at `/var/lib/ramshared/approvals/activate-<release>.token`; trust check (root-owned, not group/world-writable); wire format parse; version equality.
- **Host lease**: `/run/ramshared/host-resume-lease.json` must exist, be well-formed, and not be expired.
- **Dirty state**: `refuse_half_cascade` refuses ghost or half-cascade state; `cascade_already_healthy` short-circuits as idempotent no-op (Kahneman #17).
- **Activate**: `up_with_args(--vram <n> --zram <n>)` with sizes from the resolved config.

## Validation results

### Rust slice coverage (2026-09-30)

- `crates/ramshared-cli/src/cascade/boot.rs`: **84.4%** (465/551) — **PASS**
- `crates/ramshared-cli/src/cascade/lifecycle.rs`: **91.9%** (340/370) — **PASS**

### Test suites

- `cargo test -p ramshared-cli -- --test-threads=1`: **528 passed, 0 failed**
- `scripts/safety/test-control-plane-units.sh`: **PASS**
- `scripts/safety/test-nbd-product-preflight.sh`: **47/47 PASS**

### Named matrix tests (14/14 for boot.rs)

| Test | Type | Status |
| --- | --- | --- |
| `boot_refuses_deploy_shaped_invocation` | #13 | PASS |
| `boot_refuses_untrusted_approval_token` | #13 | PASS |
| `scoped_approval_accepts_only_the_running_release_version` | #9 | PASS |
| `boot_requires_fresh_host_prerequisites` | #13 | PASS |
| `boot_refuses_ghost_or_half_cascade_state` | #13 | PASS |
| `boot_is_idempotent_when_cascade_already_healthy` | #17 | PASS |
| `boot_records_refusal_reason_and_leaves_state_off` | #13 | PASS |
| `boot_config_source_as_str` | #9 | PASS |
| `boot_error_display_names_every_variant` | #9 | PASS |
| `verify_host_lease_accepts_a_fresh_lease_and_rejects_the_boundary` | #9/#13 | PASS |
| `approval_token_trust_refuses_non_root_and_writable_tokens` | #13 | PASS |
| `boot_command_parses_and_dispatches` | #9 | PASS |
| `boot_command_rejects_unknown_options` | #13 | PASS |
| `boot_never_mutates_product_binaries` | #13 | PASS |

## Open evidence (env-bound)

- **Live reboot rounds (×3)**: requires a real WSL2 reboot cycle with the sealed release installed. Env-bound for Day-0.
- **`BINARY_MATCH` on `ramsharedd`**: requires deployed daemon with vsock/NBD path active. Pending live E2E.
- **Approval token on real root-owned production path**: unit-tested via `RealApprovalHost` for error paths; happy path requires root-owned token (non-root test process cannot mint one).

## Rollback trigger

Ghost swap or WSL hard freeze after enabling the unit → `uninstall-cascade-boot.sh` + disable unit; record in `validation.md`. Revert `c9ee700a` and `fb959aaf` to return to the script orchestrator path (still in-tree).

## Traceability

PRD RF-7..RF-10 + DT-1..DT-7 → SPEC ITEM-1..6 → this IMPL.
