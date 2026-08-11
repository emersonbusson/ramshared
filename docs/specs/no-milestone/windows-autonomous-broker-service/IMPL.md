# IMPL — Autonomous Windows broker service packaging and supervision

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-autonomous-broker-service/SPEC.md

## Status

partial · cover ✓ · E2E env-bound · BINARY_MATCH ✗

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-broker/src/lease.rs` | ITEM-2/RF-1,RF-5 | Shared sender-bound lease state. |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | ITEM-2/RF-5 | Linux integration and wire-equivalence tests. |
| `crates/ramshared-winbroker/` | ITEM-3/4 | Native SCM broker, authenticated product/status pipes, immutable JSONL and Windows Event Log evidence. |
| `crates/ramshared-winsvc/src/{config,ipc,package,runtime}.rs` | ITEM-5/6 | Fixed pipe client, safe broker-loss lifecycle and transactional package plans. |
| `crates/ramshared-winsvc/src/main.rs` | ITEM-6 | Two-service install/repair/start/stop/status/uninstall controller. |
| `scripts/windows/Run-GuestBrokerService.ps1` | ITEM-4 | ACL, protocol, recovery and BINARY_MATCH VM drill. |
| `scripts/windows/Run-GuestProductPackage.ps1` | ITEM-6 | Transaction and immutable package harness. |
| `scripts/windows/Run-GuestAutonomousLifecycle.ps1` | ITEM-8 | VM cold boot, storage, SHA, broker-loss and residue drill with current PID/run Online identity and fail-closed Event 153/recovery queries. |
| `scripts/windows/Copy-RamSharedProtectedEvidence.ps1` | ITEM-8 | Protected SYSTEM evidence copy with canonical containment, reparse and resource bounds, PowerShell 5.1-safe staged publication/cleanup, and paired hashes. |
| `scripts/windows/Run-HostAutonomousLifecycle.ps1` | ITEM-9 | Corrected one-approval-per-reboot physical campaign with exact current-run disk identity, fail-closed pagefile/stop gates, bounded child cleanup, and intended-payload integrity. |
| `scripts/windows/Test-HostAutonomousLifecycleStatic.ps1` | ITEM-9 | PowerShell 5.1 manufactured safety tests for physical orchestration without host mutation. |
| `scripts/windows/Test-AutonomousBrokerStatic.ps1` | ITEM-7 | Static daily-surface and single-installer assertions. |

## Validation (numbers)

- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
- Tests → broker 40; winbroker 19; winsvc 120 pass + 1 CUDA ignore;
  wsl2d library/binary/integration suites pass with hardware/root cases guarded.
- Cover → `lease.rs` 98.7% (149/151); winbroker `lib.rs` 97.3%
  (286/294); winsvc config 96.7% (348/360), IPC 83.3% (20/24),
  package 89.0% (258/290), runtime 88.9% (568/639), evidence 97.2%
  (353/363).
- SPEC matrix → every named Rust test and all five PowerShell harness surfaces
  are present; static harness PASS.
- Corrected physical-harness manufactured tests → 12/12 named PASS under
  Windows PowerShell 5.1; parser exit 0. The process-tree case timed out and
  terminated its own temporary parent and grandchild as intended.
- TDD RED → `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy
  Bypass -File scripts/windows/Test-HostAutonomousLifecycleStatic.ps1` →
  exit 1, `missing production function Get-Sha256Hex`, before the production
  harness was changed. GREEN → the same command → exit 0, 12/12 named PASS.
- Windows static aggregate →
  `Test-WindowsCiStatic.ps1 -RepoRoot <repo>` → exit 0; all nine mutating
  switches (`Install`, `Service`, `Vm`, `Hardware`, `Gpu`, `Pressure`,
  `Shutdown`, `Reboot`, `PhysicalHost`) refused non-zero.
- Final broker matrices → Peer, RetryBudget and Boundary PASS after the Event
  Log implementation; native broker SHA
  `EE7C102F...D746911`; readiness 476–671 ms; blocked stop 254–266 ms;
  partial-frame refusal 10,021 ms.
- Package E2E → FreshInstall, Repair, ManufacturedRollback,
  UninstallRefusal and CleanUninstall PASS for VM `R:` and physical `S:`
  manifests.
- Historical VM E2E (superseded for current closure) → three healthy runs: readiness median 11,556 ms, p99 19,784 ms;
  product stop median 4,949 ms, p99 5,060 ms; 9/9 SHA rounds match.
  `BrokerLossOnline` stopped safely in 4,838 ms with no reconnect/residue.
  These rows predate DT-22 and do not prove the current PID/run identity or
  fail-closed provider-query contracts.
- Historical physical E2E (superseded for current closure) → three cold boots with manifest SHA
  `0F6DFD...C1F1A`: readiness median 1,164 ms, p99 1,165 ms; product stop
  median 2,810 ms, p99 3,049 ms; 9/9 SHA rounds match; residue 0;
  forced kills 0. Those rows predate DT-18–DT-21 and do not prove the
  corrected intended-payload, exact identity, fresh-approval, or cleanup
  contracts.
- Historical BINARY_MATCH → broker, winsvc and loaded driver SHA matched the active
  manifest in VM; the physical harness rejected a driver mismatch before
  install and used the same immutable manifest for all three boots. A corrected
  physical run must re-prove all three matches.
- Current PowerShell safety slice → parser 4/4; `Test-WindowsDiskCounterAuditStatic.ps1`
  30/30 PASS lines; `Test-GuestProductOnlineStatic.ps1` and
  `Test-GuestExhaustiveStatic.ps1` PASS. Manufactured raw `..` copy and legacy
  `-Run` invocations refused before copy/controller action.
- Protected-copy live E2E → the initial positive run exposed the PowerShell
  5.1 `Split-Path -LiteralPath ... -Parent` incompatibility and empty staging
  leak. The corrected command published 90 files/945,121 bytes; independent
  recomputation matched all 90 source/destination hashes and inventory SHA-256
  `03c714cc7a262bdcb5c82eae1fb4fca2ff84c6b41ab436863997bcc32dd19cd8`.
  Seven validated empty staging directories were removed; zero remain.
- Current loaded driver is not the candidate: DriverStore image SHA-256
  `033ce3f8be36b0dc01d731ea29c6af8259a11d398f7e1f863bf01aa1786dd1c9`
  versus current test-signed `.8` candidate
  `5e4ff79148274ec1a029a057714f0066389b6e103f5425e5c3c2aab1adb07a55`.
  Inf2Cat and SignTool `/pa` passed; the unsigned Code Analysis build was
  `816dc594aacf4b97e1c7f3130a546f8038b226f09494a40d87a61cad8678f1dd`.
- Evidence → indexed in `evidence/README.md`; final post-change broker proof is
  `evidence/vm-final/broker-final-matrices.json`; rejected attempts retained
  under `evidence/physical-failed-*`.

## Gaps

Open/environment-bound: rerun the corrected disposable-VM lifecycle, including
provider failure injection, exact current-run BINARY_MATCH, and a positive
SYSTEM protected-copy inventory. Then execute three supervised physical cold boots through
the corrected harness, with a fresh explicit approval for each boot, and
capture current BINARY_MATCH, intended-payload hashes, exact Online/disk
identity, supported-stop results, zero residue, and final cleanup. Until that
campaign is recorded, this SPEC is partial. Demand-start remains the accepted
policy. Production signing/Secure Boot distribution remains an independent
repository release gate and is not reclassified by this implementation.

## Rollback trigger

Any unauthorized pipe admission; foreign-holder lease release; product-pipe
reconnect after Online EOF; artifact/config/BINARY_MATCH mismatch; physical
target-letter collision; non-zero residue; SHA mismatch; consumer stop over
30 seconds; product stop over 45 seconds; watchdog activation; or any forced
kill.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-1, RF-5 | ITEM-2 | `2435a3d` |
| RF-1, RF-2, RF-3, RF-5 | ITEM-3/4 | `2435a3d`, `ad15c33` |
| RF-3, RF-4, RF-5, RF-6 | ITEM-5 | `2435a3d` |
| RF-1, RF-2, RF-6, RF-7, RF-8 | ITEM-6/7/8/9 | `2435a3d`, `e6b759f` |
| RF-1–RF-8 | Living docs and final evidence audit | `e917e3c` |
| RF-2, RF-4, RF-6, RF-8 | DT-22 fail-closed VM evidence hardening | pending supervised commit |
