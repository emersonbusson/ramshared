# IMPL — WSL2 NBD-only product readiness

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md`

## Status

`partial` · local static/manufactured gates green · cover ✓ · E2E partial ·
BINARY_MATCH partial. Attempt 11b proved the corrected 1 GiB disk-only cell
and NBD sample-one allocation, checksum, and occupancy, but failed while
republishing the NBD baseline for sample two. The local correction now includes
an explicit reconnect transaction and a daemon that serves two sequential
connection generations before explicit shutdown and passed its fresh
independent Gate A; it has not been deployed or rerun live. The complete ordered 1/2/4
GiB Windows/WSL2 matrix has not run on the corrected release, so this is not
DONE.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | ITEM-4 / RF-NBD-5,10,12 | Pure NBD readiness, capacity, state, and refusal model. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | ITEM-4 / RF-NBD-10,12 | Teardown executor proves swapoff before NBD disconnect/daemon stop through an injected plan. |
| `scripts/safety/nbd-product-preflight.sh` | ITEM-1..4,6 / RF-NBD-1..10,19 | Sealed installed-release, lower-sink, Relay, state, BINARY_MATCH, no-reboot, and live-seam gates. |
| `scripts/safety/install-cascade-boot.sh` | ITEM-2,8 / RF-NBD-2,13 | Attended lower-sink-bound install, provenance, immutable backup, selector transaction, and rollback frontiers. |
| `scripts/safety/nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14,15,17,18,19,20 | Disk/NBD cell, common zram topology, cgroup-before-start occupancy, exact scratch identity, exact device-name classification, sysfs-derived NBD capacity with bounded mkswap usable-size validation, explicit NBD reconnect baseline transaction, cleanup, comparison, and internal custody envelope. |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` | ITEM-5 / RF-NBD-15 | In-cgroup launcher and create-once start barrier. |
| `scripts/safety/nbd-benchmark-lib.sh` | ITEM-5 / RF-NBD-14,17 | Identity-bound scratch, disk republication, and exact NBD reconnect baseline helpers. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14..20 | Twenty-three manufactured/refusal suites for topology, identity, exact device-name classification, all supported sysfs capacities, bounded mkswap usable size, overflow/trailing-field refusal, cgroup bounds, disk and NBD reconnect republication, activity receipts, cleanup, seams, custody, and aggregation. |
| `scripts/p0/Start-CudaVramWorkload.ps1` | ITEM-5 / RF-NBD-16,17 | Fresh pair-scoped CUDA handshakes and unconditional cleanup. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | ITEM-5..7 / RF-NBD-6,16..20 | Bounded Windows/WSL controller, numeric headroom, strict NBD capacity/usable-size custody, pair custody, promotion order, watchdog classification, and public pair envelope. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | ITEM-5..7 / RF-NBD-16..20 | PowerShell static/manufactured contract and refusal checks, including strict NBD capacity/usable-size custody. |
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
- Harnesses: `bash scripts/safety/test-nbd-benchmark-cell.sh` → 23/23;
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
- DT-NBD-40 RED→GREEN: the shell reproducer first proved that
  `18446744073710600192` wrapped to a valid size in Bash; the Windows
  reproducer independently showed missing, malformed, wrong, and overflowing
  capacity/usable fields were accepted. Bounded decimal validation and strict
  Windows custody now make both named tests green, including 1/2/4 GiB and
  trailing-field cases. Independent Gate A remains required before commit.
- DT-NBD-41 RED→GREEN: the committed reproducer first reached the intended
  missing `nbd_reconnect_republish_swap_pair` function after all preceding
  cell contracts passed. The helper now proves lower-first NBD/zram removal,
  absence, an exact non-symlink Unix-socket endpoint, exact `nbd-client -unix`
  argv without `-persist`, mandatory `mkswap -L RAMSHARED`, zram/NBD publish
  order, and final exact topology. It refuses regular/mismatched sockets,
  lower swapoff, attach, mkswap, both swapon stages, and post-publish topology
  drift without retry or broad cleanup. The production NBD caller then
  re-derives identity and repeats `BINARY_MATCH`; disk-only retains the generic
  scratch republish path.
- DT-NBD-42 RED→GREEN: the focused injected runtime received only one reply
  from two sequential connection generations because the first balanced
  `Closed` terminated the simple worker. The corrected runtime treats zero
  live clients as quiescent, serves both generations against the same backend,
  and terminates only on an explicit shutdown request. An owned, cancelable,
  joined, nonblocking bridge carries production SIGINT/SIGTERM into a channel
  wake; a full/disconnected queue falls back to the same terminal atomic at the
  worker boundary. Focused tests prove two replies, idle wake,
  full/disconnected queue refusal, and owned-socket cleanup. Canonical coverage
  remains above threshold (`main.rs` 81.7%, `conn.rs` 96.5%). The fresh
  independent Gate A passed on this exact runtime/shell contract before sealing.
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
- Attempt 9b (2026-08-12, sealed release source
  `6c83be7`): the disk-only worker reached HOLD after allocating 3584 MiB and
  passed its checksum integrity proof. The persisted activity receipt measured
  zram `1048572 KiB`, NBD `2820804 KiB`, scratch `2820804 KiB`, and disk
  `0 KiB`, against the zram/second-tier thresholds of `1040384 KiB`. The
  scratch path was under `/var/lib/ramshared/nbd`, so substring classification
  counted the same scratch pages as NBD and rejected the legitimate disk
  control. Cleanup returned to `PRODUCT_OFF`; `/proc/swaps` retained only the
  pre-existing `/dev/sdc`. The exact-device classifier is now covered by
  `swap_device_classifier_requires_exact_device_names` and the local cell
  harness is 19/19; the live matrix remains open and no completion claim is
  made.
- Attempt 10 (2026-08-12, reviewed release source `38e49ff`, before the
  capacity/usable-size correction): the disk-only cell completed all three
  samples and passed its integrity, occupancy, and cleanup gates. The NBD
  cell reached `READY` with `BINARY_MATCH=PASS`, then refused before its first
  sample because `/proc/swaps` reported the valid 1 GiB usable size of
  `1048572 KiB` while the harness incorrectly required `1048576 KiB` as the
  block capacity. Cleanup was verified as `PRODUCT_OFF`, with only the
  pre-existing `/dev/sdc` remaining in `/proc/swaps`. The corrected contract
  now reads exact sectors from `/sys/block/nbdN/size`, permits at most 8 KiB
  of mkswap usable-size loss, keeps context `nbd.size_kib` at the exact tier,
  and records the observed usable size separately; no live rerun has been
  performed in this task.
- Attempt 11b (2026-08-12, reviewed release source `d1e270d`): disk-only
  completed 3/3 samples with integrity, occupancy, and cleanup. NBD sample one
  reached HOLD, passed its checksum, and recorded zram `1048572 KiB`, NBD
  `1048572 KiB`, and disk `450740 KiB` occupancy before
  `BASELINE_REPUBLICATION_FAILED`. The kernel receipt shows that
  `swapoff /dev/nbd0` caused `NBD_DISCONNECT`; this is expected for the
  deliberately non-persistent client and proves that generic swapoff/swapon
  cannot republish NBD. The DT-NBD-41 local correction is not live evidence;
  cleanup returned to `PRODUCT_OFF`, and a fresh approved rerun remains
  required.

## SPEC matrix → named tests

The corrected implementation has local coverage for its source/static/
manufactured names, including the 23 cell tests, 26 preflight suites, fresh
CUDA handshake/refusal checks, bounded WSL controller checks, exact
ratio/baseline mappings, public-pair custody, and
`sealed_bundle_contains_benchmark_runner_and_worker`, and both DT-NBD-40
names. The live rows
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
| RF-NBD-1..20 | ITEM-1..8 | pending — Attempt 11b used reviewed source `d1e270d`; local RED checkpoints `b2b9fe9` and `3eb8e70` plus the reconnect GREEN passed independent Gate A and still require sealing and a live rerun |
