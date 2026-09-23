# IMPL — Autonomous WSL2 Origin Attachment and Systemd Scope Envelopment

## 1. Summary

Implemented autonomous WSL2 origin VHDX auto-attachment and transparent systemd scope auto-envelopment in `ramshared-cli`:
- **Transparent Scope Envelopment (RF-1, RF-2, RF-3; DT-4):** In `crates/ramshared-cli/src/main.rs`, when `ramshared up` is invoked from an unwrapped interactive shell in a running systemd environment (where `INVOCATION_ID` is absent), the CLI automatically re-executes itself under `systemd-run --scope -q -- /proc/self/exe up "$@"` with recursion guard `_RAMSHARED_SCOPED=1`.
- **Just-In-Time Origin Auto-Attachment (RF-4, RF-5, RF-6, RF-7, RF-8; DT-1, DT-2, DT-3):** In `crates/ramshared-cli/src/cascade/cascade_io.rs`, `ensure_origin_attached()` detects when the sealed origin partition is absent (such as post `wsl --shutdown`), derives the Windows VHDX path from `/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json` (stripping UTF-8 BOM if present), verifies the host manifest SHA-256 and PARTUUID against the sealed origin configuration, applies an ASCII path allowlist, and executes `wsl.exe --mount --vhd <path> --bare` directly with a 10-second bound. It polls for the device appearance before proceeding.

## 2. Modified Files

- `crates/ramshared-cli/src/main.rs`:
  - Added `should_auto_wrap_systemd_scope()` and `dispatch_systemd_scope()`.
  - Added unit tests `up_auto_envelops_in_systemd_scope_when_invocation_id_missing` and `up_executes_inline_when_invocation_id_present`.
- `crates/ramshared-cli/src/cascade/cascade_io.rs`:
  - Added `ensure_origin_attached()` and `validate_windows_origin_path()`.
  - Added unit tests `ensure_origin_attached_is_noop_when_device_present`, `ensure_origin_attached_issues_bounded_mount_when_absent`, and `ensure_origin_attached_fails_closed_on_timeout_or_mismatch`.

## 3. Test Evidence and Slice Coverage

- **Unit tests:** 311 unit tests passed (0 failed).
- **Integration tests:** 10 integration tests passed (0 failed).
- **Clippy & fmt:** `cargo fmt --check` and `cargo clippy -p ramshared-cli --all-targets -- -D warnings` passed 100% clean.
- **Slice line coverage (gate >= 80%):**
  - `crates/ramshared-cli/src/cascade/cascade_io.rs`: **80.3%** (4949 / 6165 lines)
  - `crates/ramshared-cli/src/main.rs`: **91.1%** (1796 / 1971 lines)
  - Verdict: **Coverage gate PASSED**.

## 4. Live E2E Evidence

- **Baseline Status:**
  - VHDX bare attached as SCSI disk exposing sealed origin partition matching manifest.
  - Cascade initialized under systemd scope with unique `INVOCATION_ID`.
  - Swaps:
    - Tier 1: `/dev/zram0` (1048572 KiB, prio 200, 0 used)
    - Tier 2: `/dev/nbd0` (4194300 KiB, prio 100, 0 used, authoritative write-through SSD origin)
    - Tier 3: WSL fallback disk swap (4194304 KiB, prio -2, 0 used)
- **Kernel Health:** `PASS_ZERO_PANIC`, zero D-state stalls or ring buffer warnings.

## 5. Current qualification

Earlier test counts and live activation in this file predate the sealed-hash and direct-interop correction. Current targeted unit tests and static checks pass, but a new binary has not completed a clean before→action→after host attachment, cascade, and teardown run. The current host reports pending recovery with active managed swaps and unavailable cache telemetry.

- Verdict: **🟡 PARTIAL** until a clean controlled host E2E, binary match, and fresh coverage evidence.
