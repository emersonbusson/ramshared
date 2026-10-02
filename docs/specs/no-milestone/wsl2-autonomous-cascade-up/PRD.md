---
slug: wsl2-autonomous-cascade-up
title: Autonomous WSL2 origin attachment and systemd scope envelopment
milestone: —
issues: []
---

# PRD — Autonomous WSL2 Origin Attachment and Systemd Scope Envelopment

## 1. Summary

Enable `ramshared up` to execute fully autonomously on WSL2 without requiring manual Windows host intervention or external command-line wrapping. When invoked, `ramshared up` automatically ensures execution inside a canonical systemd transient scope (providing the required `INVOCATION_ID`) and automatically re-attaches the sealed authoritative SSD origin VHDX (`ramshared-origin.vhdx`) via bounded host interop if it was detached during a WSL shutdown or reboot.

## 2. Technical Context

- **Confirmed in codebase:** `crates/ramshared-cli/src/cascade/cascade_io.rs` enforces that `daemon_invocation_id()` reads `/proc/{pid}/environ` for `INVOCATION_ID=`, refusing direct activation with `CascadeError::Precondition("daemon has no unique systemd InvocationID; direct unmanaged activation is refused")` when invoked from an unwrapped shell.
- **Confirmed in codebase:** `systemd-run --scope` generates a transient systemd scope unit and sets `INVOCATION_ID` in the process environment, which child processes (including `ramsharedd`) inherit by default.
- **Confirmed in codebase:** `/etc/ramshared/origin.conf` records `origin_path=/dev/disk/by-partuuid/<uuid>`, `partuuid`, and expected swap parameters. In product mode, `ramsharedd` mandates `--origin-manifest /etc/ramshared/origin.conf` for the authoritative SSD tier.
- **Confirmed locally:** Executing `wsl.exe --shutdown` cleanly terminates the WSL2 VM but causes Hyper-V to detach all `--bare` VHDX disks. Post-reboot, `/dev/disk/by-partuuid/<uuid>` is absent until re-attached.
- **Confirmed locally:** Executing `timeout 10 cmd.exe /c "wsl.exe --mount --vhd <path> --bare"` from inside WSL2 succeeds and exposes the origin SCSI disk and partition without triggering an interactive Windows UAC GUI prompt.
- **Inference:** Automating origin attachment and scope envelopment inside `ramshared up` eliminates human operational error and transient activation blocks without weakening fail-closed safety invariants.

## 3. Recommended Option

Implement a two-stage autonomous bootstrap directly in `crates/ramshared-cli`:

1. **Transparent Scope Envelopment (CLI Dispatcher):**
   When `ramshared up` is invoked from a shell where `INVOCATION_ID` is not present in `std::env::var("INVOCATION_ID")`, and systemd is detected as active (`/run/systemd/system` exists), the CLI does not attempt unmanaged activation. Instead, it transparently executes `systemd-run --scope -q -- /proc/self/exe up <args>` using `execvp` (or `Command::status`). If already inside a systemd unit or scope, it proceeds directly.

2. **Just-In-Time Origin Auto-Attachment (Cascade Setup):**
   During cascade initialization in `cascade_io.rs`, before verifying partition identity, the CLI checks if the target `origin_path` exists. If absent, it verifies the SHA-256 and PARTUUID of `/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json` against `/etc/ramshared/origin.conf`, reads the VHDX path, runs bounded `wsl.exe --mount --vhd <path> --bare` (timeout 10s), and waits up to 5s for the specified partition to appear. An attachment or identity failure aborts fail-closed.

### Discarded Alternatives

- **Windows Scheduled Task / Windows Service Auto-Attach:**
  *Rejected:* Out-of-band host configuration. Fails if the Windows task is disabled, deleted, or if the repository is cloned on a new machine. It leaves the Linux CLI brittle and dependent on external host state.
- **Requiring the user to type `systemd-run --scope ramshared up`:**
  *Rejected:* Poor UX, highly error-prone, leaks implementation details to the operator, and violates the "autonomous operation" requirement.
- **Disabling the `INVOCATION_ID` check in `cascade_io.rs`:**
  *Rejected:* Violates Kahneman anti-hang contracts (superprompt.md). The `INVOCATION_ID` is necessary for durable lifecycle tracking, cgroup containment, and guaranteed `swapoff`-first cleanup by systemd.

## 4. Functional Requirements (RF-N)

- **RF-1:** When `ramshared up` is invoked without `INVOCATION_ID` in an active systemd environment, it must automatically re-launch itself under `systemd-run --scope`.
- **RF-2:** If `systemd-run` is unavailable or fails to spawn the scope, `ramshared up` must exit with an explicit error code and message without touching devices or swap.
- **RF-3:** If `INVOCATION_ID` is already present, `ramshared up` must execute inline without recursive re-envelopment.
- **RF-4:** Before validating origin block devices, `cascade_io.rs` must probe whether the sealed `origin_path` is present.
- **RF-5:** If `origin_path` is missing, `cascade_io.rs` must execute bounded host attachment via WSL interop (`wsl.exe --mount --vhd <path> --bare`) with a strict 10-second timeout.
- **RF-6:** After issuing the host mount command, the CLI must poll for up to 5 seconds for `/dev/disk/by-partuuid/<PARTUUID>` to appear.
- **RF-7:** If the device appears, its GPT PARTUUID, parent disk identity, and swap UUID must be validated against `/etc/ramshared/origin.conf`. Any mismatch must result in immediate fail-closed termination.
- **RF-8:** If the host mount command times out, fails, or the device fails to appear, `ramshared up` must exit fail-closed with status code 1, leaving the host and existing swaps untouched.

## 5. Non-Functional Requirements (NFR-N)

- **NFR-1 (Safety & Anti-Hang):** Never proceed with NBD connection or `swapon` unless both `INVOCATION_ID` and the verified origin block device are present.
- **NFR-2 (Latency):** Scope envelopment must add <50ms overhead. Origin attachment (when needed) must complete within 3 seconds under normal host conditions.
- **NFR-3 (Idempotency):** Calling `ramshared up` when the origin VHDX is already attached must perform zero host mutations.
- **NFR-4 (Observability):** Scope delegation and origin attachment attempts must emit structured logs (`[up] auto-attaching origin VHDX via host interop...`, `[up] auto-enveloping in systemd transient scope...`).

## 6. Flows

### Happy Path (Cold Start after WSL Reboot)
1. User or boot service runs `sudo ramshared up`.
2. CLI checks `INVOCATION_ID`: absent. Checks `/run/systemd/system`: present.
3. CLI prints `[up] auto-enveloping execution in systemd transient scope...` and executes `systemd-run --scope -- ramshared up`.
4. In the child process under systemd scope, `INVOCATION_ID` is present.
5. Setup phase reads `/etc/ramshared/origin.conf`. Checks `/dev/disk/by-partuuid/<uuid>`: absent.
6. CLI prints `[up] origin VHDX detached; attempting bounded host attach...` and runs `wsl.exe --mount ... --bare` directly.
7. Origin SCSI disk appears; `/dev/disk/by-partuuid/<uuid>` resolves to the sealed origin partition.
8. Partition dev_t and swap UUID match manifest.
9. ZRAM and NBD tiers initialized.
10. `swapon /dev/zram0` (prio 200) and `swapon /dev/nbd0` (prio 100).
11. Status printed: `phase: Armed`, `protection: READY`. Exit 0.

### Alternate Path (Already Attached & In-Scope)
1. `ramshared up` runs inside a systemd service (`INVOCATION_ID` present).
2. Origin `/dev/disk/by-partuuid/<uuid>` already exists.
3. No host commands executed; proceeds immediately to cascade setup.

### Error Path (Host Interop Failure / Missing VHDX)
1. `origin_path` absent. CLI runs host mount command.
2. Host returns error (e.g. VHDX file deleted or moved).
3. Poll timeout expires (5s) without device appearing.
4. CLI emits `[up] error: origin VHDX could not be attached from host; aborting fail-closed`.
5. No daemon started, no swap touched. Exit 1.

## 7. Data / State Model

- Configuration parsed from `/etc/ramshared/origin.conf`:
  - `origin_path`: Linux block path (`/dev/disk/by-partuuid/<uuid>`)
  - `partuuid`: Expected partition UUID
  - `expected_swap_uuid`: Expected swap header UUID
- Host manifest at `/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json`:
  - `origin_vhdx`: Absolute Windows path (`C:\ProgramData\RamShared\ramshared-origin.vhdx`)
- Environment variables:
  - `INVOCATION_ID`: 32-character hexadecimal string injected by systemd.
  - `RAMSHARED_NO_AUTO_SCOPE`: Optional escape-hatch to bypass auto-envelopment in testing.

## 8. Interfaces

- **CLI:** `ramshared up [--vram MiB] [--zram MiB]` (preserves all existing CLI flags and syntax).
- **Host Interop Command:** `wsl.exe --mount --vhd <path> --bare`.

## 9. Dependencies and Risks

- **Dependencies:** Windows interop enabled in WSL (`/proc/sys/fs/binfmt_misc/WSLInterop`), `systemd-run` installed (standard on Ubuntu 24.04).
- **Risks:**
  - *Host interop hang:* Mitigated by strict 10s subprocess timeout (`timeout 10`).
  - *Double-envelopment loop:* Mitigated by checking `std::env::var("INVOCATION_ID")` and an explicit environment marker `RAMSHARED_SCOPED=1`.
- **Numeric Rollback Trigger:** Any failure to boot existing cascades or regression in existing unit tests (>0 test failures) triggers immediate rollback.

## 10. Implementation Strategy

1. **Slice 1 (Origin Auto-Attachment in cascade_io):** Add `ensure_origin_attached()` helper with bounded process execution and device polling.
2. **Slice 2 (Transparent Scope Envelopment in main.rs):** Add scope detection and `systemd-run` re-execution in CLI `up` dispatcher.
3. **Slice 3 (Verification & Tests):** Unit tests for auto-attach logic, mock host runners, and coverage gate >=80%.

## 11. Documents to Update

- `docs/specs/no-milestone/wsl2-autonomous-cascade-up/SPEC.md`
- `docs/specs/no-milestone/wsl2-autonomous-cascade-up/AUDIT-2.5.md`
- `MEMORY.md`

## 12. Out of Scope

- Modifying Windows kernel drivers or Hyper-V internals.
- Modifying `Manage-RamSharedOrigin.ps1` provisioning logic.
- Ublk transport on WSL2 (remains permanently rejected due to teardown freeze risk).

## 13. Acceptance Criteria

1. Running `sudo ./target/release/ramshared up` from a plain interactive bash terminal successfully activates the 3-tier cascade without requiring `systemd-run` prefix.
2. If `ramshared-origin.vhdx` was detached via `wsl --shutdown`, running `ramshared up` automatically attaches it and transitions to `phase: Armed`, `protection: READY`.
3. Unit tests cover auto-attachment and scope re-envelopment paths with >=80% slice coverage on modified files.
4. `./scripts/docs-check.sh` passes 100% green.

## 14. Validation Plan

- Unit tests in `crates/ramshared-cli/src/main.rs` and `crates/ramshared-cli/src/cascade/cascade_io.rs`.
- Slice coverage check: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/main.rs crates/ramshared-cli/src/cascade/cascade_io.rs --min 80`.
- Live E2E test on host: verify `ramshared down`, verify clean detach/re-attach, and verify `ramshared up` brings up all 3 tiers with `PASS_ZERO_PANIC`.
