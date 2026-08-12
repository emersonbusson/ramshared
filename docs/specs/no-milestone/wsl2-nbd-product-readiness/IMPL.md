# IMPL — WSL2 NBD-only product readiness

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md`

## Status

`partial` · cover ✓ · E2E partial (historical 1 GiB pilot only) · BINARY_MATCH
partial (historical pilot only). The corrected implementation and local
static/manufactured gates are green. The new ordered 1/2/4 GiB Windows/WSL2
matrix has not run on the reviewed release, so this is not DONE and no new
live claim is made.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | ITEM-4 / RF-NBD-5,10,12 | Pure NBD readiness, capacity, state, and refusal model. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | ITEM-4 / RF-NBD-10,12 | Teardown executor proves swapoff before NBD disconnect/daemon stop through an injected plan. |
| `scripts/safety/nbd-product-preflight.sh` | ITEM-1..4,6 / RF-NBD-1..10,19 | Sealed installed-release, lower-sink, Relay, state, BINARY_MATCH, no-reboot, and live-seam gates. |
| `scripts/safety/install-cascade-boot.sh` | ITEM-2,8 / RF-NBD-2,13 | Attended lower-sink-bound install, provenance, immutable backup, selector transaction, and rollback frontiers. |
| `scripts/safety/nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14,15,17,18,19,20 | Disk/NBD cell, common zram topology, cgroup-before-start occupancy, exact scratch identity, cleanup, comparison, and internal custody envelope. |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` | ITEM-5 / RF-NBD-15 | In-cgroup launcher and create-once start barrier. |
| `scripts/safety/nbd-benchmark-lib.sh` | ITEM-5 / RF-NBD-14,17 | Identity-bound scratch and swapoff-first helpers. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14..20 | Eighteen manufactured/refusal suites for topology, identity, cgroup bounds, exact republication, activity receipts, scratch/zram, occupancy, cleanup, seams, custody, and aggregation. |
| `scripts/p0/Start-CudaVramWorkload.ps1` | ITEM-5 / RF-NBD-16,17 | Fresh pair-scoped CUDA handshakes and unconditional cleanup. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | ITEM-5..7 / RF-NBD-6,16..20 | Bounded Windows/WSL controller, numeric headroom, pair custody, promotion order, watchdog classification, and public pair envelope. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | ITEM-5..7 / RF-NBD-16..20 | PowerShell static/manufactured contract and refusal checks. |
| `scripts/windows/Test-WindowsCiStatic.ps1` | ITEM-7 / RF-NBD-17..19 | Windows static wrapper includes the matrix harness. |
| `scripts/package/build-linux-bundle.sh` | ITEM-2,5 / RF-NBD-2,9,18 | Universal unbound input bundle with source identity and input manifest; binding occurs only in attended install. |
| `tools/ci/check-benchmark-evidence.test.mjs` | ITEM-7 / RF-NBD-11,18,20 | Public pair evidence schema validator fixture, 15/15 tests. |
| `docs/governance/capability-observations.generated.json` | ITEM-7 / RF-NBD-11 | Generated capability facts remain separate from live proof. |

## Validation (numbers)

- Tests: `cargo test -p ramshared-cli --all-targets` → 95 passed; `cargo test
  -p ramshared-tier --all-targets` → 52 passed.
- Named local lifecycle contracts: `swapoff_completes_before_nbd_disconnect`,
  `failed_swapoff_keeps_daemon_and_device_alive`, and
  `setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture` are
  present and pass in the CLI suite.
- Harnesses: `bash scripts/safety/test-nbd-benchmark-cell.sh` → 18/18;
  `bash scripts/safety/test-nbd-product-preflight.sh` → 26/26.
- Public evidence: `node --test tools/ci/check-benchmark-evidence.test.mjs`
  → 15/15, including the sanitized pair-envelope fixture and custody rules.
- Format/lint: `cargo fmt --all -- --check` → pass; `cargo clippy -p
  ramshared-cli -p ramshared-tier --all-targets -- -D warnings` → pass.
- Cover: `crates/ramshared-tier/src/nbd_readiness.rs` → 83.9% (251/299);
  `crates/ramshared-cli/src/cascade/cascade_io.rs` → 85.6% (1259/1471), both
  above the 80% per-file gate. The report JSON is local-only under `tmp/`.
- Generated/docs hygiene: capability-observation `--check`, `./scripts/docs-check.sh`,
  and `git diff --check` → pass in the current static slice.
- PowerShell: focused `Test-NbdBenchmarkMatrixStatic.ps1` → pass; complete
  `Test-WindowsCiStatic.ps1 -RepoRoot <repo>` → pass, including
  `windows_static_wrapper_includes_nbd_benchmark_harness` and
  `windows_static_suite_runs_named_static_harnesses`. No live CUDA or
  Windows/WSL matrix is claimed here.
- First live campaign attempt: exited before any benchmark cell because
  PowerShell 5.1 rejected a multi-character `TrimStart` argument in the matrix
  inventory writer. The new exact manufactured test
  `matrix_inventory_is_ps51_safe_and_repository_relative` reproduces and
  closes that pre-cell failure; the failed attempt retained `PRODUCT_OFF`.
- Second live campaign attempt: after deploying the inventory fix, selected
  release discovery failed before any cell. A diagnostic using the same
  `ProcessStartInfo` path proved that the Windows command-line encoder emitted
  one extra backslash before embedded quotes and corrupted the Bash argument.
  `windows_command_line_preserves_exact_wsl_shell_argument` now runs a real
  child process and requires the complex argument to arrive byte-exactly.
- Third live campaign attempt: the corrected CRT encoder preserved an ordinary
  child argument, but the nested Bash/Python discovery program was still
  reparsed by `wsl.exe` and exited 127 before any cell. Discovery no longer
  transports an inline shell program: bounded direct `readlink`, `cat`, and
  `sha256sum` argv return the sealed records, and PowerShell validates the exact
  installed manifest, input manifest, source state, and provenance.
- Fourth live campaign attempt: direct release discovery and campaign preflight
  passed, then the first disk-only cell refused before `mkswap` because GNU
  `stat %F` reports a newly created file as `regular empty file`. Scratch
  identity now uses stable numeric file-mode metadata and the manufactured test
  requires the identity to remain exact across empty-file allocation. The
  zero-byte, root-owned, inactive residue was moved to a root-only quarantine;
  it was never present in `/proc/swaps`.
- Fifth live campaign attempt: scratch identity and cleanup passed, but the
  disk-control gate treated a newly published zero-used zram as absent. The
  gate now validates one zram partition at priority 200 plus the exact scratch
  file at priority 100 and refuses duplicate/wrong-priority/NBD/ghost rows;
  initial usage is deliberately not an existence proxy.
- Sixth live campaign attempt: disk control demoted about 3 GiB, then memcg OOM
  killed the worker because hard `memory.max=1200 MiB` also charged valid
  swapcache/writeback. The contract now writes 1200 MiB to `memory.high` to
  force reclaim and writes `V+3072 MiB` to `memory.max` as an allocation-bound
  emergency ceiling. Cleanup again left only the pre-existing disk swap.
- Seventh live campaign attempt: the first 3584 MiB allocation reached HOLD and
  passed full integrity without OOM, but freed swapcache did not return below
  an arbitrary tolerance within 30 seconds. Between runs the exact same zram
  and lower devices are now removed lower-first and republished zram-first at
  priorities 200/100; identities and NBD BINARY_MATCH are revalidated.
- Eighth live campaign attempt: allocation and checksum again passed, then the
  disk-control occupancy predicate refused. The prior code discarded observed
  deltas on refusal, so the cell now writes the exact four activity deltas and
  both thresholds before making any occupancy decision.

## SPEC matrix → named tests

The corrected implementation has local coverage for all source/static/
manufactured names in SPEC § Required tests matrix, including the 18 cell
tests, 26 preflight suites, fresh CUDA handshake/refusal checks, bounded WSL
controller checks, exact ratio/baseline mappings, public-pair custody, and
`sealed_bundle_contains_benchmark_runner_and_worker`. The live rows
`nbd_lifecycle_before_action_after`, `relay_gate_before_action_after`, and
`NBD_BENCHMARK_MATRIX` remain environment-bound.

## E2E (historical pilot only)

The only live evidence currently retained is the supervised 1 GiB pilot under
[`evidence/2026-08-12-live/`](evidence/2026-08-12-live/):

- Before: `before.txt` observed `PRODUCT_OFF`, disk-only lower sink, Relay
  `CLEAN`, and no managed zram/NBD swap.
- Action: `action-sudo.txt` activated the sealed `v0.8.0-8-g0b09518` release.
- After: `after-active.txt` observed zram priority 200, NBD priority 100, disk
  priority -2, Relay `CLEAN`, and `BINARY_MATCH=PASS`; `binary-match.txt`
  records the resolved executable equality.
- This pilot is historical evidence only. It does not prove any new 1/2/4 GiB
  pair, n≥3 statistics, pair-scoped CUDA custody, or the corrected reviewed
  release. `validation.md` was intentionally not updated in this docs-only
  reconciliation.

## Gaps

`env-bound (blocker)` — clean reviewed-release deployment; legitimate
Windows/WSL2 before/action/after execution for P1/Q2/Q4
idle and bounded pairs; n=3 median/p99/deviation and backend comparisons;
per-cell occupancy, cleanup, Relay, and BINARY_MATCH receipts; root
`validation.md`; Gate B; PR checks and merge.
No reboot or WSL shutdown is part of this record.

## Rollback trigger

Rollback/refuse on any one observable failure: `swapoff` error; residual
managed/ghost swap or daemon; BINARY_MATCH mismatch; lower-tier capacity below
`V + max(ceil(0.10 × V), 512 MiB)`; any failed installer rollback phase; stale
selector/provenance/manifest; CUDA cleanup failure; timeout classified as
anything other than `RED/unverified_terminated`; or any live seam/host action.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-NBD-1..20 | ITEM-1..8 | pending — reviewed source revision `e8b0e62a4e39c8c015436e3c922df402c65457c0` plus the focused scratch-identity fix in this change; live matrix remains incomplete |
