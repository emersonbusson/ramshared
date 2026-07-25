# IMPL — Autonomous Windows broker service packaging and supervision

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-autonomous-broker-service/SPEC.md

## Status

implemented · cover ✓ · E2E ✓ · BINARY_MATCH ✓

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-broker/src/lease.rs` | ITEM-2/RF-1,RF-5 | Shared sender-bound lease state. |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | ITEM-2/RF-5 | Linux integration and wire-equivalence tests. |
| `crates/ramshared-winbroker/` | ITEM-3/4 | Native SCM broker, authenticated product/status pipes, immutable config and evidence. |
| `crates/ramshared-winsvc/src/{config,ipc,package,runtime}.rs` | ITEM-5/6 | Fixed pipe client, safe broker-loss lifecycle and transactional package plans. |
| `crates/ramshared-winsvc/src/main.rs` | ITEM-6 | Two-service install/repair/start/stop/status/uninstall controller. |
| `scripts/windows/Run-GuestBrokerService.ps1` | ITEM-4 | ACL, protocol, recovery and BINARY_MATCH VM drill. |
| `scripts/windows/Run-GuestProductPackage.ps1` | ITEM-6 | Transaction and immutable package harness. |
| `scripts/windows/Run-GuestAutonomousLifecycle.ps1` | ITEM-8 | VM cold boot, storage, SHA, broker-loss and residue drill. |
| `scripts/windows/Run-HostAutonomousLifecycle.ps1` | ITEM-9 | Three-boot physical campaign with watchdog and collision refusal. |
| `scripts/windows/Test-AutonomousBrokerStatic.ps1` | ITEM-7 | Static daily-surface and single-installer assertions. |

## Validation (numbers)

- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.
- Tests → broker 40; winbroker 19; winsvc 120 pass + 1 CUDA ignore;
  wsl2d library/binary/integration suites pass with hardware/root cases guarded.
- Cover → `lease.rs` 98.7% (149/151); winbroker `lib.rs` 81.5%
  (286/351); winsvc config 96.7% (348/360), IPC 83.3% (20/24),
  package 89.0% (258/290), runtime 88.9% (568/639).
- SPEC matrix → every named Rust test and all five PowerShell harness surfaces
  are present; static harness PASS.
- Package E2E → FreshInstall, Repair, ManufacturedRollback,
  UninstallRefusal and CleanUninstall PASS for VM `R:` and physical `S:`
  manifests.
- VM E2E → three healthy runs: readiness median 11,556 ms, p99 19,784 ms;
  product stop median 4,949 ms, p99 5,060 ms; 9/9 SHA rounds match.
  `BrokerLossOnline` stopped safely in 4,838 ms with no reconnect/residue.
- Physical E2E → three cold boots with manifest SHA
  `0F6DFD...C1F1A`: readiness median 1,164 ms, p99 1,165 ms; product stop
  median 2,810 ms, p99 3,049 ms; 9/9 SHA rounds match; residue 0;
  forced kills 0.
- BINARY_MATCH → broker, winsvc and loaded driver SHA matched the active
  manifest in VM; the physical harness rejected a driver mismatch before
  install and used the same immutable manifest for all three boots.
- Evidence → `evidence/{vm-final,package-final,physical-final}/`; rejected
  attempts retained under `evidence/physical-failed-*`.

## Gaps

closed

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
| RF-1, RF-2, RF-3, RF-5 | ITEM-3/4 | `2435a3d` |
| RF-3, RF-4, RF-5, RF-6 | ITEM-5 | `2435a3d` |
| RF-1, RF-2, RF-6, RF-7, RF-8 | ITEM-6/7/8/9 | `2435a3d` + evidence commit |
