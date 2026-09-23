# SPEC — Autonomous WSL2 Origin Attachment and Systemd Scope Envelopment

## Closed Scope

- **In now:**
  - Transparent auto-envelopment of `ramshared up` in `systemd-run --scope` when invoked in an active systemd environment without `INVOCATION_ID`.
  - Just-in-time detection of missing sealed origin device in `cascade_io.rs` and autonomous attachment of `ramshared-origin.vhdx` via bounded Windows interop.
  - Strict postcondition validation: PARTUUID, GPT disk GUID, and swap UUID verification before proceeding to NBD or swap setup.
  - Comprehensive unit test suites with mock runners covering both success and refusal branches.
- **Out now:**
  - Ublk transport changes (remains permanently refused on WSL2).
  - Modifying host Windows services or `Manage-RamSharedOrigin.ps1`.
- **Assumed-ready dependencies:**
  - WSL2 kernel with `CONFIG_BLK_DEV_NBD=m` and `CONFIG_ZRAM=m` (confirmed on running kernel `6.18.40.1-microsoft-standard-WSL2+ #2`).
  - Active systemd init (`/run/systemd/system` present).
  - Sealed origin VHDX present on Windows host at `C:\ProgramData\RamShared\ramshared-origin.vhdx`.

---

## Traceability

| PRD Requirement | Technical Decision | Implementation Items |
| :--- | :--- | :--- |
| **RF-1, RF-3** | DT-4 | ITEM-1, ITEM-2 |
| **RF-2** | DT-4 | ITEM-2 |
| **RF-4, RF-5** | DT-1, DT-2 | ITEM-3, ITEM-4 |
| **RF-6, RF-7** | DT-3 | ITEM-4, ITEM-5 |
| **RF-8** | DT-1, DT-3 | ITEM-4, ITEM-5 |
| **NFR-1, NFR-3** | DT-1, DT-4 | ITEM-2, ITEM-4 |
| **NFR-2, NFR-4** | DT-1, DT-4 | ITEM-2, ITEM-4 |

---

## Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| **DT-1** | Use `cmd.exe /c "wsl.exe --mount --vhd <path> --bare"` for host attachment. | WSL interop (/init) transparently dispatches cmd.exe without interactive UAC prompts or PowerShell startup overhead (~50ms vs ~1500ms). |
| **DT-2** | Derive origin VHDX path from `/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json` with fallback to `C:\ProgramData\RamShared\ramshared-origin.vhdx`. | Ensures strict alignment with the sealed host manifest while rejecting caller-controlled arbitrary paths. |
| **DT-3** | Bound device poll to 5 seconds with 250ms intervals. | Prevents indefinite hangs if the host fails to expose SCSI LUNs; provides fast detection upon device appearance. |
| **DT-4** | Re-exec via `systemd-run --scope` in `main.rs` when `INVOCATION_ID` is absent. | Completely transparent to the user; guarantees systemd cgroup v2 containment and canonical invocation tracking required by `superprompt.md`. |

---

## Atomicity and Rollback

- **Atomicity Frontier:**
  1. Scope envelopment executes before any filesystem, device, or swap operation.
  2. Origin attachment occurs before ZRAM creation, NBD socket binding, or `swapon`.
  3. If origin attachment fails or times out, zero devices are modified, no daemon is spawned, and zero swaps are activated.
- **Rollback Split:**
  - *Userspace / CLI:* Clean exit code 1 with diagnostic on stderr.
  - *Kernel / Block Devices:* No NBD or ZRAM devices created if origin fails. If scope re-exec fails, host state is untouched.
  - *Host / Persistent:* Zero persistent host modifications. Disk remains attached or unattached without partial formatting.

---

## Kahneman Map (Critical Only)

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| **ITEM-2** (Scope auto-wrap) | #17 (Replayability) | Does re-executing under `systemd-run --scope` prevent infinite recursion loops? | Unit test `up_dispatch_does_not_loop_when_invocation_id_present` | Abort if recursion depth > 1 or `INVOCATION_ID` is ignored |
| **ITEM-4** (Origin auto-attach) | #13 (Refusal + Legitimate) | Does auto-attach refuse invalid VHDX paths or mismatched PARTUUIDs while accepting legitimate sealed disks? | Unit tests `auto_attach_refuses_mismatched_partuuid` and `auto_attach_succeeds_with_matching_device` | Abort if unsealed device is accepted or timeout exceeds 10s |

---

## Security Checklist (Pre-Impl)

- [x] **Privilege:** `systemd-run` and `ramshared up` require root (`euid == 0`).
- [x] **User/Host copy:** VHDX path strictly validated to match sealed manifest regex (`^[A-Za-z]:\\[A-Za-z0-9._\\-]+$`). Arbitrary caller paths rejected.
- [x] **Flags/IOCTL codes:** N/A (uses existing safe block device and CLI interfaces).
- [x] **Info-leak:** No sensitive tokens or host credentials exposed in logs.
- [x] **IRQ/atomic or IRQL:** N/A (userspace CLI).
- [x] **Lifetime:** Device attachment is verified before use; swapoff precedes any disconnect.
- [x] **Hot-unplug / device-gone:** Handled fail-closed: missing device triggers immediate refusal.
- [x] **Host safety:** No unsupervised live pressure; bounded timeouts on all host interop calls.
- [x] **Shared-hardware cushion:** Preserves existing GPU headroom calculations.
- [x] **Bounded DMA / foreign driver calls:** Host `cmd.exe` call bounded by 10s deadline.
- [x] **Cooperative cascade spillover:** Preserves full 3-tier cascade (`zram0` > `nbd0` > `sdb`).
- [x] **Replayable ops:** Idempotent: attaching an already-attached VHDX is a no-op.

---

## Files to CREATE / MODIFY / DELETE

### MODIFY

#### **`crates/ramshared-cli/src/main.rs`**
- **Purpose:** In `CliActionRunner::up`, detect absence of `INVOCATION_ID` in systemd environments and auto-envelop execution via `systemd-run --scope`.
- **RF / DT:** RF-1, RF-2, RF-3; DT-4.
- **Symbol:** `CliActionRunner::up`, helper `should_auto_wrap_systemd_scope()`, `exec_systemd_scope()`.
- **Before → After:** Previously directly called `cascade::up_with_args(args)`. Now checks if auto-scoping is required; if so, spawns `systemd-run --scope -q -- <exe> up <args>` and propagates exit status.
- **Tests:** `crates/ramshared-cli/src/main.rs` :: `up_auto_envelops_in_systemd_scope_when_invocation_id_missing`, `up_executes_inline_when_invocation_id_present`.
- **Cover target:** >=80%.

#### **`crates/ramshared-cli/src/cascade/cascade_io.rs`**
- **Purpose:** Add `ensure_origin_attached()` invoked in `setup_new_cascade()` before `origin_partuuid(&args.origin_path)`.
- **RF / DT:** RF-4, RF-5, RF-6, RF-7, RF-8; DT-1, DT-2, DT-3.
- **Symbol:** `ensure_origin_attached()`, `probe_host_origin_vhdx_path()`, `attach_origin_vhdx_via_host()`.
- **Before → After:** Previously failed immediately with `CascadeError::Precondition` if `origin_path` was absent. Now detects absence, resolves sealed Windows VHDX path, issues bounded `cmd.exe /c wsl.exe --mount` host command, and polls until PARTUUID is visible or deadline expires.
- **Tests:** `crates/ramshared-cli/src/cascade/cascade_io.rs` :: `ensure_origin_attached_is_noop_when_device_present`, `ensure_origin_attached_issues_bounded_mount_when_absent`, `ensure_origin_attached_fails_closed_on_timeout_or_mismatch`.
- **Cover target:** >=80%.

---

## Living Docs

| Document | Action |
| :--- | :--- |
| `ARCHITECTURE.md` | Update CLI cascade startup sequence to document auto-scope and origin auto-attachment. |
| `docs/reliability/DEGRADATION-MATRIX.md` | Update with origin detachment auto-recovery row. |
| `MEMORY.md` | Append implementation progress and test evidence. |

---

## Implementation Order

- **ITEM-1:** Add test harness and unit tests in `crates/ramshared-cli/src/main.rs` for `should_auto_wrap_systemd_scope()` and scope dispatch.
- **ITEM-2:** Implement transparent scope envelopment in `crates/ramshared-cli/src/main.rs`.
- **ITEM-3:** Add test fixtures and unit tests in `crates/ramshared-cli/src/cascade/cascade_io.rs` for `ensure_origin_attached()`.
- **ITEM-4:** Implement origin VHDX auto-attachment and bounded device polling in `crates/ramshared-cli/src/cascade/cascade_io.rs`.
- **ITEM-5:** Run full test suite, verify slice coverage >=80%, run `./scripts/docs-check.sh`, and conduct live E2E verification.

---

## Required Tests Matrix

| Production Path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| `crates/ramshared-cli/src/main.rs` | `main.rs` :: `up_auto_envelops_in_systemd_scope_when_invocation_id_missing` | unit | #17 | >=80% |
| `crates/ramshared-cli/src/main.rs` | `main.rs` :: `up_executes_inline_when_invocation_id_present` | unit | #17 | >=80% |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `cascade_io.rs` :: `ensure_origin_attached_is_noop_when_device_present` | unit | #13 | >=80% |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `cascade_io.rs` :: `ensure_origin_attached_issues_bounded_mount_when_absent` | unit | #13 | >=80% |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | `cascade_io.rs` :: `ensure_origin_attached_fails_closed_on_timeout_or_mismatch` | unit | #16 | >=80% |

---

## Validation Checklist

- [x] `cargo fmt` / `cargo clippy -p ramshared-cli --all-targets -- -D warnings` / `cargo test -p ramshared-cli`
- [x] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/main.rs crates/ramshared-cli/src/cascade/cascade_io.rs --min 80`
- [x] Live path on WSL2: verify `ramshared up` from raw terminal succeeds autonomously (`PASS_ZERO_PANIC`).
- [x] Every matrix row has a real test name.
- [x] Kahneman critical rows have executable evidence.
