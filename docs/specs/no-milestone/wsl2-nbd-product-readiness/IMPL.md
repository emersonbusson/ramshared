# IMPL — WSL2 NBD-only product readiness

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md`

## Status

`partial` · local static/manufactured gates green · cover ✓ · E2E complete ·
BINARY_MATCH complete for this matrix. Attempt30 is the latest sealed live
attempt, on source `a365bda0daf89a9707159b86efca8c1ba1ac760b` and installed
manifest `a848034f99da1a5205d84a4b844ac071ed91303a190172b8f1208c0ca5d6bb3a`.
All 12 P1/P2/P4 idle/bounded disk-only/NBD cells and all 36 samples passed.
Every NBD cell retained `BINARY_MATCH=PASS`; each bounded pair held one CUDA
context across disk then NBD and released it without force. All cells and the
terminal preflight returned `PRODUCT_OFF`. Six public pair records validate as
`BASELINE` candidates; they remain nonpromotable only because a prior canonical
baseline is absent. The implementation remains `partial` until Gate B, hosted
required checks, PR review, and merge complete.
Attempt 16 identified a Q4 timeout-budget mismatch; later pre-22b attempts
sealed and exercised the Q4 correction, Attempt 21 exposed the NBD recovery
self-deadlock, and Attempt 22b stopped at Q2. Attempt 15
proved that both corrected 1 GiB idle cells
completed all three samples with integrity, occupancy, and per-cell
`PRODUCT_OFF`; the disk/NBD contexts recorded the same `zram0` usable-size
receipt (`1048572 KiB`), priority, algorithm, and identity. The controller
then emitted a false `comparison_zram_topology_mismatch` because it compared
that observed usable size with nominal capacity `1048576 KiB`. DT-NBD-43 now
accepts only the bounded observed interval and retains exact disk/NBD tuple
equality; Attempt 19 sealed it and exercised the equality in all four complete
P1/P2 pairs. Attempt30 subsequently completed the ordered 1/2/4 GiB
Windows/WSL2 matrix; the remaining work is repository/PR governance rather
than live qualification.
Fresh independent Gate A passed the prior frozen connection-preserving
candidate, the DT-NBD-43 delta, and the frozen DT-NBD-44 seven-file candidate
before a new sealed campaign. Attempt 17 then reproduced a false
`cell_timeout_budget_mismatch`: an equal summary budget with a different JSON
property order failed the old string comparison. The semantic strict helper
now canonicalizes the validated tier tuple, while real value/type drift still
refuses. The focused named test is green, and fresh independent Sol Gate A
passed this additional frozen five-file delta.

Attempt 19 (2026-08-13, sealed source `fed085b`, release manifest
`bb2b38e279dcc954c4ff9aed872721d68837a512fb861d0d6cc203cf22858a32`) ran from
the campaign artifact under the host temporary root. All eight P1/P2 cells
passed with their three samples and `PRODUCT_OFF`; Q4 idle disk-only also
passed 3/3. The ordered Q4 idle NBD cell refused at
`gpu_headroom_shortfall` (`4311 < 4608`) before producing a PASS result,
leaving 9/12 completed PASS cells. The matrix summary contains a tenth,
REFUSED entry; the two Q4 bounded cells did not run. The post-campaign
preflight remained `PRODUCT_OFF` with Relay clean. An active OBS recording was
discovered after the refusal and was intentionally not touched; no host-app
remediation or retry was performed. This is valid refusal/partial evidence,
not a complete matrix or promotion claim.

Attempt 20 (2026-08-13, same sealed source and release manifest) completed all
eight P1/P2 cells and both Q4 idle cells, each with three samples and
`PRODUCT_OFF`, for 10/12 completed PASS cells. The Q4 bounded pair admitted
pre-CUDA headroom (`5181 >= 5120`) and reached CUDA ready, then refused at
`gpu_headroom_shortfall_after_cuda_ready` (`4594 < 4608`); the Q4 bounded NBD
cell did not run. CUDA released without force. Terminal preflight remained
`PRODUCT_OFF` with Relay clean. This is valid bounded refusal/partial evidence,
not a complete matrix or promotion claim; at that revision a fresh approved
campaign remained required for the missing Q4 bounded NBD cell. Attempt30
later supplied the complete matrix and root validation.

Attempt 21 (2026-08-13, same sealed source and release manifest) admitted all
six pairs and completed 11/12 cells with three-sample PASS results. Q4 bounded
disk passed 3/3; Q4 bounded NBD passed samples one and two. Before sample three,
the daemon's free-floor recovery path synchronously executed
`swapon -p 100 /dev/nbd0` on the same single thread that serves NBD requests.
The child `swapon` blocked reading `/dev/nbd0`, while the only thread that
could answer those requests waited for that child. Kernel receipts recorded
NBD EIO and stuck reads; the third worker could not reach HOLD before the
600-second sample deadline. To preserve the daily host and avoid the
controller's documented last-resort `wsl --terminate`, the operator changed
only that cell's cgroup `memory.high` from 1200 MiB to its already bounded
7168 MiB `memory.max`. The worker then reached HOLD at only 6016 of the
required 6656 MiB and wrote checksum integrity PASS for those 6016 MiB; it did
not complete sample three, and this out-of-band containment makes the cell and
campaign non-promotable. The cell exited on `SAMPLE_TIMEOUT`, the matrix recorded one
RED `wsl_controller_failed` result and `unverified_unknown`, and CUDA released
without force. A separate pinned read-only postflight immediately afterward
proved the exact release and input manifests, Relay PASS, `PRODUCT_OFF`, and
no managed/ghost swap, daemon, worker, or cgroup residue; only the pre-existing
`/dev/sdc` swap remained. No Windows or WSL restart occurred. At that revision,
the synchronous recovery activation had to be replaced by a bounded
asynchronous state machine; `47889e0` later completed and audited that
correction.

Attempt 22b (2026-08-13, pre-fix source) independently reproduced a Q2
timeout-calibration defect without running a live workload in this correction
slice: the controller/cell contract still treated Q2 as `120 s` per sample and
`900 s` per cell, while the bounded Q2 hold requires `240/1020`. Timeout
cleanup left partial HOLD/integrity artifacts; those artifacts were not a
completed sample and were not promoted. This is retained as RED defect
evidence only. That historical correction used P1 `120/900`, Q2 `240/1020`,
and Q4 `600/2100` tuples; Attempt27 supersedes it with distinct HOLD and
integrity-finalization policy caps. The named manufactured
`partial_timeout_integrity_not_promoted` regression verifies the completion
boundary without expanding the result schema. Attempt22b itself was live on
sealed source `63bd3be`; no live WSL, NBD, swap, CUDA, cgroup, reboot, or
termination action ran during the subsequent correction slice.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | ITEM-4 / RF-NBD-5,10,12 | Pure NBD readiness, capacity, state, and refusal model. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | ITEM-4 / RF-NBD-10,12 | Teardown executor proves swapoff before NBD disconnect/daemon stop through an injected plan. |
| `scripts/safety/nbd-product-preflight.sh` | ITEM-1..4,6 / RF-NBD-1..10,19 | Sealed installed-release, lower-sink, Relay, state, BINARY_MATCH, no-reboot, and live-seam gates. |
| `scripts/safety/install-cascade-boot.sh` | ITEM-2,8 / RF-NBD-2,13 | Attended lower-sink-bound install, provenance, immutable backup, selector transaction, and rollback frontiers. |
| `scripts/safety/nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14,15,17,18,19,20 | Disk/NBD cell, common zram topology, cgroup-before-start occupancy, exact scratch identity, exact device-name classification, sysfs-derived NBD capacity with bounded mkswap usable-size validation, connection-preserving NBD baseline transaction, cleanup, comparison, and internal custody envelope. |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` | ITEM-5 / RF-NBD-15 | In-cgroup launcher and create-once start barrier. |
| `scripts/safety/nbd-benchmark-lib.sh` | ITEM-5 / RF-NBD-14,17 | Identity-bound scratch, disk republication, and connection-preserving NBD baseline helpers. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | ITEM-5,7 / RF-NBD-14..20 | Forty-four manufactured/refusal suites for topology, identity, exact device-name classification, all supported sysfs capacities, bounded mkswap usable size, overflow/trailing-field refusal, cgroup bounds, disk and connection-preserving NBD republication, activity receipts, repeated-signal cleanup, exhaustive preflight seam denial, custody-frontier faults, failure-receipt create-once publication, and aggregation. |
| `scripts/p0/Start-CudaVramWorkload.ps1` | ITEM-5 / RF-NBD-16,17 | Fresh pair-scoped CUDA handshakes and unconditional cleanup. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | ITEM-5..7 / RF-NBD-6,16..20 | Bounded Windows/WSL controller, numeric headroom, strict NBD capacity/usable-size custody, pair custody, promotion order, watchdog classification, and a public pair envelope that derives metrics from fresh on-disk summaries, binds the exact emitted comparison artifact, and keeps YELLOW/RED pair decisions separate from immutable PASS cell summaries. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | ITEM-5..7 / RF-NBD-16..20 | PowerShell static/manufactured contract and refusal checks, including strict NBD capacity/usable-size custody, cached-summary and published-artifact mutation refusal, YELLOW/RED public-writer custody, and watchdog/CUDA failure composition. |
| `scripts/windows/Test-WindowsCiStatic.ps1` | ITEM-7 / RF-NBD-17..19 | Windows static wrapper includes the matrix harness. |
| `scripts/package/build-linux-bundle.sh` | ITEM-2,5 / RF-NBD-2,9,18 | Universal unbound input bundle with source identity and input manifest; binding occurs only in attended install. |
| `tools/ci/check-benchmark-evidence.mjs` | ITEM-7 / RF-NBD-11,18,20 | Repository-side public-evidence validator. It binds candidate and custody SHA-256 values to the exact `pair-comparison.json` artifact bytes, recomputes controller-rounded public ratios, and enforces the exact baseline-verdict/public-decision mapping before a copied candidate can qualify. |
| `tools/ci/check-benchmark-evidence.test.mjs` | ITEM-7 / RF-NBD-11,18,20 | Public pair evidence validator fixtures, 21/21 tests including comparison-content, candidate-hash, missing-artifact, schema, release, timeout, rounded-ratio, zero-disk-stddev, decision-mapping, and numeric refusal coverage. |
| `docs/governance/capability-observations.generated.json` | ITEM-7 / RF-NBD-11 | Generated capability facts remain separate from live proof. |

## Validation (numbers)

- Tests: `cargo test -p ramshared-cli --all-targets` → 95 passed; `cargo test
  -p ramshared-tier --all-targets` → 52 passed.
- Named local lifecycle contracts: `swapoff_completes_before_nbd_disconnect`,
  `failed_swapoff_keeps_daemon_and_device_alive`, and
  `setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture` are
  present and pass in the CLI suite.
- Harnesses: `bash scripts/safety/test-nbd-benchmark-cell.sh` → expected 48/48
  on the current Attempt26 candidate; `bash scripts/safety/Test-CascadePressureIntegrityWorker.sh`
  → 3 named integrity checks; `bash scripts/safety/test-nbd-product-preflight.sh`
  → currently verified
  33/33, including exact three-observation userspace-zombie liveness and
  fail-closed live/malformed/race cases.
- Public evidence: Node 24.15.0 `node --test --experimental-test-coverage
  --test-coverage-include=tools/ci/check-benchmark-evidence.mjs
  --test-coverage-lines=80 --test-coverage-branches=80
  --test-coverage-functions=80 tools/ci/check-benchmark-evidence.test.mjs`
  → 21/21, 95.37% lines, 80.60% branches, and 94.23% functions. The fixture
  covers sanitized pair-envelope schema, duplicate keys, candidate/custody
  comparison-byte binding, controller-rounded ratios including the zero-disk-
  stddev `null` case, exact baseline-decision mapping, missing artifacts, and
  semantic refusal fronts. The stddev-zero case was RED in TDD and is now
  GREEN.
- Format/lint: `cargo fmt --all -- --check` → pass; `cargo clippy -p
  ramshared-cli -p ramshared-tier --all-targets -- -D warnings` → pass.
- Cover: `crates/ramshared-tier/src/nbd_readiness.rs` → 83.9% (251/299);
  `crates/ramshared-cli/src/cascade/cascade_io.rs` → 85.6% (1259/1471), both
  above the 80% per-file gate. The report JSON is local-only under `tmp/`.
- Generated/docs hygiene: capability-observation `--check`, `./scripts/docs-check.sh`,
  and `git diff --check` → pass in the current static slice.
- PowerShell: focused `Test-NbdBenchmarkMatrixStatic.ps1` → pass after the
  stddev-zero correction, including YELLOW/RED publication without summary
  mutation. The complete `Test-WindowsCiStatic.ps1 -RepoRoot <repo>` wrapper
  was green before that Node change and is pending a same-tree rerun. No live
  CUDA or RamShared/WSL product matrix is claimed here.
- DT-NBD-40 RED→GREEN: the shell reproducer first proved that
  `18446744073710600192` wrapped to a valid size in Bash; the Windows
  reproducer independently showed missing, malformed, wrong, and overflowing
  capacity/usable fields were accepted. Bounded decimal validation and strict
  Windows custody now make both named tests green, including 1/2/4 GiB and
  trailing-field cases. The current frozen-tree Gate A includes this contract
  and passed.
- DT-NBD-41 RED→GREEN: Attempt 12b proved the prior model wrong: after the
  sample-one `swapoff`, the kernel emitted no `NBD_DISCONNECT`, so a second
  `nbd-client` attach targeted an already connected device and caused
  `BASELINE_REPUBLICATION_FAILED`. Checkpoint `1b1739b` changed the fixture
  first and failed on the missing
  `nbd_preserved_connection_republish_swap_pair`. The helper now proves
  lower-first NBD/zram swap removal and absence, performs no attach/detach,
  requires `mkswap -L RAMSHARED`, publishes zram/NBD in 200/100 order, and
  validates exact topology. The production caller re-derives equal identity
  and repeats pinned `BINARY_MATCH`; disk-only retains the generic scratch
  republish path. After Attempt 13 exposed another generic refusal, checkpoint
  `7b33d20` required stable stage receipts. All eight transaction frontiers and
  forged child output are now executed; cardinality-safe parsing emits one
  owned reason or one fallback. Fresh independent Gate A passed this frozen
  diagnostic-custody candidate.
- DT-NBD-42 RED→GREEN: the focused injected runtime received only one reply
  from two sequential connection generations because the first balanced
  `Closed` terminated the simple worker. The corrected runtime treats zero
  live clients as quiescent, serves both generations against the same backend,
  and terminates only on an explicit shutdown request. An owned, cancelable,
  joined, nonblocking bridge carries production SIGINT/SIGTERM into a channel
  wake; a full/disconnected queue falls back to the same terminal atomic at the
  worker boundary. Focused tests prove two replies, idle wake,
  full/disconnected queue refusal, and owned-socket cleanup. Canonical coverage
  remains above threshold (`main.rs` 81.7%, `conn.rs` 96.5%). Fresh independent
  Gate A passed both that daemon-runtime revision and the later frozen
  connection-preserving DT-NBD-41 shell candidate.
- DT-NBD-43 RED→GREEN: Attempt 15 captured equal disk/NBD zram receipts with
  `size_kib=1048572`, yet the comparison helper hardcoded nominal
  `1048576` and returned `comparison_zram_topology_mismatch`. The focused
  PowerShell 5.1 manufactured contract first failed at that hardcoded gate.
  The minimal correction accepts only canonical PS5 JSON integral CLR types
  (`SByte`, `Byte`, `Int16`, `Int32`, or `Int64`) in the inclusive observed
  usable-size interval `[1048568, 1048576]`. It executes accepted
  lower/observed/upper values, refuses `1048567`, noncanonical, overflowing,
  raw JSON decimal/exponent values, and otherwise-valid cross-pair zram size
  drift. Fresh independent Gate A passed the frozen delta; it has not been
  deployed or rerun live.
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
  cannot republish NBD. At that revision, the DT-NBD-41 local correction was
  not live evidence; cleanup returned to `PRODUCT_OFF`, and the next approved
  attempt supplied the required sealed rerun.
- Attempt 12b (2026-08-12, reviewed release source `1aaeb5b`): disk-only
  completed 3/3 with integrity, occupancy, and `PRODUCT_OFF`. NBD sample one
  reached HOLD and passed SHAKE256 integrity with zram and NBD each observing
  `1048572 KiB`. The next baseline transaction returned
  `BASELINE_REPUBLICATION_FAILED`; the kernel log contains the original attach
  and the later cleanup disconnect, but no disconnect at the sample boundary.
  Therefore the failed operation was the helper's second attach of an already
  connected `/dev/nbd0`, not daemon loss. The Windows controller stopped
  promotion, completed bounded stream drainage, and cleanup left only the
  pre-existing `/dev/sdc` swap with no RamShared process. The connection-
  preserving DT-NBD-41 correction was local-only at that revision and required
  a new sealed release plus a fresh approved matrix rerun, supplied by the
  following attempts.
- Attempt 13 (2026-08-12, reviewed release source `73e1977`): disk-only again
  completed 3/3 with integrity, occupancy, and cleanup. NBD sample one reached
  HOLD and passed integrity, then returned the same generic
  `BASELINE_REPUBLICATION_FAILED`; cleanup was bounded and left only
  `/dev/sdc`. A separate supervised no-pressure lifecycle diagnostic proved
  that the same connected `/dev/nbd0` retained its kernel PID after swapoff,
  accepted `mkswap`, and republished at zram/NBD priorities 200/100. This
  isolates the remaining failure to a post-load frontier that the generic
  reason concealed. Checkpoint `7b33d20` now requires stable per-stage
  republication reasons; at that revision its GREEN implementation was
  diagnostic custody, not completion evidence, and required the sealed fresh
  run performed by the following attempt.
- Attempt 14 (2026-08-12, reviewed release source `875fe4b`): disk-only again
  completed 3/3, and NBD sample one again passed HOLD/integrity. The new stage
  custody isolated the refusal to `NBD_REPUBLICATION_ZRAM_RECORD_INVALID`.
  Cleanup left only `/dev/sdc`. The zram runtime record is CLI cleanup state,
  not immutable sample identity; checkpoint `ff5e58e` now requires the cell to
  capture the one validated `/dev/zramN` from live topology before the first
  worker and use that exact identity across samples. The local GREEN is not
  live evidence. Checkpoint `724ab7f` additionally reproduced that the capture
  must be followed by a full exact zram/NBD pair gate before context, cgroup,
  or worker admission. Fresh independent Gate A passed the frozen local 24/24
  GREEN with zero findings; sealing and a fresh run were required at that
  revision and were subsequently exercised by later attempts.
- Attempt 15 (2026-08-12, reviewed release source `033291e`): both 1 GiB idle
  cells completed 3/3 samples with checksum and occupancy `PASS`; the NBD cell
  also retained `BINARY_MATCH=PASS`. Their sealed contexts carried the same
  zram tuple: `zram0`, `1048572 KiB`, priority `200`, `lzo-rle`, and identity
  `7a3d17559d1543238daef389ad12b526d314f7452993b64a4319866487dc2fd7`.
  Each cell reached verified `PRODUCT_OFF`. The NBD teardown receipt shows
  only the pre-existing `/dev/sdc`, a dead daemon, no ghost swap, and no broad
  kill; no reboot or WSL shutdown occurred. The pair controller nevertheless
  stopped at `comparison_zram_topology_mismatch`, a false RED caused by its
  nominal-size hardcode. This does not qualify the pair or advance 2/4 GiB;
  At that revision, DT-NBD-43 was pending sealing and rerun; Attempt 19 later
  exercised its exact equality in all four complete P1/P2 pairs.
- Attempt 16 (2026-08-12, reviewed release source `f9c70fe`): P1/Q2 disk/NBD
  pairs passed their bounded cells in both idle and bounded conditions. Q4 idle disk sample one passed,
  but allocation-to-HOLD was observed at about 335 seconds and Q4 sample two
  reached the former fixed `SAMPLE_TIMEOUT=120` after about 281 seconds.
  Bounded cleanup completed cleanly; this is not a product or cleanup failure.
  The then-current DT-NBD-44 implementation derived P1/Q2 as 120-second
  samples with a 900-second cell floor, and Q4 as 600-second samples with a
  2100-second cell deadline. The hard ceilings remained 600/2100 seconds and
  Q4 CUDA required 4320 seconds. The
  tuple is recorded in context, summary, exact-allowlist internal custody,
  comparison, and public-pair candidate evidence; manufactured internal-envelope
  tuple tamper refuses. TDD RED then GREEN is Bash cell 28/28 and the complete
  Windows PowerShell 5.1 static harness; fresh independent Sol Gate A passed
  the frozen seven-file candidate. No live pressure, CUDA, reboot, WSL
  shutdown, or terminate action ran during that correction cycle. Attempt 19
  later sealed the correction and exercised it through Q4 idle disk; the
  complete same-run matrix remained incomplete at that revision. Attempt30
  later supplied it.
- Attempt 17 (2026-08-12, unsealed local candidate): the Q4 timeout-budget
  correction exposed a second false RED in the Windows cell controller. The
  summary and controller held the same four strict fields, but PowerShell's
  property-order-preserving JSON differed and returned
`cell_timeout_budget_mismatch`. The named manufactured test
`cell_timeout_budget_property_order_is_semantic` first failed against that
raw comparison (RED), then passed after `Assert-CellTimeoutBudgetMatch`
reused `Get-StrictCellTimeoutBudget` plus canonical JSON; it also refuses a
real value mismatch and a noncanonical numeric type. The focused PowerShell
5.1 parser/static/docs gates and fresh independent Sol Gate A are green. No
  live pressure, CUDA, reboot, WSL shutdown, or terminate action ran during
  that correction cycle; Attempt 19 later ran the sealed semantic comparison.

Attempt 18 then exposed a separate contract mismatch before CUDA startup:
the controller's valid Q4 `CudaMaxHoldSec=4320` was rejected by the workload's
stale `[ValidateRange(1,3600)]` parameter cap. The new named test
`cuda_workload_hold_cap_matches_q4_timeout_budget` is RED against that cap and
GREEN after the minimal correction: the handshake seam accepts exactly 4320
seconds and refuses 4321, without allocating CUDA memory or widening the cap
unboundedly. The focused PowerShell 5.1 static harness and fresh independent
Sol Gate A pass; no live CUDA, pressure, reboot, WSL shutdown, or terminate
  action ran during that correction cycle. Attempt 19 later ran the sealed
  cap implementation and exercised the CUDA workload in the P1/P2 bounded
  pairs; Q4 idle used no CUDA context and Q4 bounded did not run. The complete
  same-run matrix remains required.

## SPEC matrix → named tests

The corrected implementation has local coverage for its source/static/
manufactured names, including the 44 cell tests, 32 preflight suites, fresh
CUDA handshake/refusal checks, bounded WSL controller checks, exact
ratio/baseline mappings, DT-NBD-43 bounded zram usable-size/equality checks,
DT-NBD-44 tier-derived timeout-budget custody and tamper refusals,
the `cell_timeout_budget_property_order_is_semantic` strict semantic-equality
refusal test, and the exact bounded
`cuda_workload_hold_cap_matches_q4_timeout_budget` controller/workload cap
test,
the `watchdog_cuda_composition` primary/secondary failure-preservation test and
the `watchdog_cuda_serialization_sanitized` full-result private-diagnostic
redaction test,
public-pair custody, and
the `partial_timeout_integrity_not_promoted` timeout/integrity completion
boundary regression,
`sealed_bundle_contains_benchmark_runner_and_worker`, and both DT-NBD-40
names. The live rows
`nbd_lifecycle_before_action_after`, `relay_gate_before_action_after`, and
`NBD_BENCHMARK_MATRIX` remain environment-bound.

## E2E evidence custody

The only live evidence currently tracked in the repository is the supervised
1 GiB pilot under
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

Attempts 19–22b are retained only under their host campaign roots while they
remain partial/noncanonical. Attempt 22b is the latest live result and proves
only four P1 PASS cells plus the Q2 timeout/refusal and terminal `PRODUCT_OFF`;
its matrix and failure hashes are custody references, not public promotion
evidence.

## Gaps

## Attempt26/Attempt27 integrity-finalization policy hardening

The first P1 idle disk-only run on sealed source `4b5f554` reached the complete
`3584 MiB` HOLD and was stopped while final checksum evidence was still being
emitted under the old five-second cleanup bound. The receipt is retained as
RED diagnostic evidence only: it does not establish an NBD, WSL2, or OOM
failure, and independent cleanup returned to `PRODUCT_OFF`.

Attempt27 observed allocation-to-HOLD at `85.149780 s`; its integrity
finalization is right-censored at `>34.044357 s`. It remains RED diagnostic
evidence, not a finalization performance claim. The source-only correction
separates HOLD containment from a finalization policy that starts only after
TERM has been issued at the observed HOLD boundary: P1/P2/P4 use
`120/120/1020`, `240/240/1740`, and `600/600/3900`
(hold/finalization/cell) seconds. `CudaMaxHoldSec=7920` is an admission cap;
each bounded pair passes and seals its own exact CUDA hold `2160/3600/7920`
(`idle=0`), while the workload refuses `7921`. Runtime, controller, context,
summary, internal envelope, public custody, and checker carry the same strict
tuple. A finalization timeout is RED, nonpromotable, stop-first, and can
record `PRODUCT_OFF` only after exact cleanup custody. A genuinely stuck worker
still takes the bounded TERM/KILL abnormal path and remains non-promotable. The
worker suite has three named checks; the integrated cell suite is expected to
be 48 checks, and the secondary TERM/INT fixture exercises
16 combinations per suite run. The 48-combination figure is historical total
coverage from three prior suite runs, not the canonical per-run metric. This
is source-only: there has been no live revalidation or new sealed release.
Gate A, resealing, and the complete live matrix remain open.

## Attempt28 finalization-only cgroup-high correction

Attempt28 (2026-08-13, sealed source `3794a30`) is the first P1 idle
disk-only RED after the finalization deadline policy. Its sole run allocated
and held `3584 MiB`, then ended `SAMPLE_INTEGRITY_DEADLINE_EXCEEDED` after the
120-second finalization cap. The sealed process receipt records cgroup
`oom_kill` `0→0`; no cgroup `oom_kill` increment was observed in that run. No NBD cell ran, so
the attempt makes no NBD claim. Because the worker log ends at HOLD with no
final scan progress, a slow final SHA-256 scan while `memory.high=1200 MiB` is
a strong inference, not a calibrated duration or a proven sole cause.

The source-only correction validates regular, non-symlink numeric cgroup limit
files after occupancy and before TERM; it requires the exact 1200 MiB high and
tier hard maximum, raises only high to that existing maximum, re-reads high
and max, and records the sanitized per-run high/max, relaxation PASS, and
monotonic OOM context. It does not change timeouts or `memory.max`. Each next run explicitly
restores and re-reads high before allocation. The worker logs bounded 512 MiB
verification progress without publishing a result early. The 48-check local
cell suite covers valid transition, refusal, drift, reset, ordering, timeout
nonpromotion, and cleanup preservation; live revalidation, resealing, and the
complete matrix remain pending.

## Attempt29 P1 allocation-to-HOLD correction

Attempt29 (2026-08-14, sealed source `a60c898`) stopped on the first P1 idle
disk-only cell. Run one completed all `3584 MiB`, HOLD, checksum, occupancy,
and cleanup; allocation-to-HOLD took `114056 ms` and the independent integrity
window retained 86 seconds. Run two reached only `2048/3584 MiB` before the
120-second HOLD deadline and emitted no completed sample. The matrix stopped
before NBD or any bounded condition, so CUDA was neither launched nor expected.
The matrix summary and inventory SHA-256 values are
`3f85c9948dc8c733b06351c029bc7a2a1512574cdc1ee8fdd8abfe41b78ef33e` and
`e1d62c1c7a0d349624a8b68a309830495b67b2a2aa3c5efdd24b20a55b558fa9`;
all 36 inventory records verified. Terminal pinned preflight returned
`PRODUCT_OFF`, no public evidence was emitted, and `/dev/sdc` was untouched.

The TDD successor raises only P1 allocation-to-HOLD from 120 to 240 seconds,
retains P1 finalization at 120 seconds, and derives P1 cell/CUDA bounds of
`1380/2880 s`. P2 remains `240/240/1740/3600`; P4 remains
`600/600/3900/7920`. Shell 48/48, the PowerShell static suite, and the Node
public-evidence suite 22/22 passed locally. Attempt30 then committed, sealed,
installed, and exercised this exact policy through the complete matrix below.

## Attempt30 complete sealed matrix

Attempt30 (2026-08-14, sealed source
`a365bda0daf89a9707159b86efca8c1ba1ac760b`) completed the canonical Windows/
WSL2 matrix without retries or manual runtime intervention. All 12 P1/P2/P4
idle/bounded disk-only/NBD cells and all 36 samples passed integrity,
occupancy, and cleanup. Every NBD cell retained `BINARY_MATCH=PASS`. Each
bounded pair held one CUDA context across its disk-only and NBD cells and
released it without force; the Q4 bounded NBD cell visibly used about
`5.1/6.1 GiB` dedicated VRAM before release.

The matrix summary SHA-256 is
`42fa3e1a00dd7e7c16f0c92196f69622ac9212c9fb889e858f6e40769af292af`.
The 551-entry inventory SHA-256 is
`58a959fd82d29b6c503382a98d82a4bbf57bb90dc94ffaf2fdc2dfa6e985aece`;
all listed byte counts and hashes verified. All six copied public pair records
pass the repository validator and are published as `BASELINE` candidates with
`promotable=false`, because no earlier canonical baseline exists. This is a
baseline-availability limitation, not a live qualification failure.

Every cell and the terminal pinned preflight returned `PRODUCT_OFF`. The
terminal preflight artifact SHA-256 is
`1c65c0ac0e645c8062ca99addd740dbd444ddb6ce28bbc14e8aa61ebe6b88e3c`.
Both product services are inactive and disabled; no managed swap, daemon,
worker, CUDA process, benchmark cgroup, or NBD attachment remains. The
pre-existing `/dev/sdc` swap was not modified. Live qualification is complete;
Gate B, hosted required checks, PR review, and merge remain open.

## Attempt25 source-only zombie liveness hardening

The installed source revision `d4efe59` exposed and refused 171 unrelated
pre-existing zombie processes as `PROC_EXE_UNREADABLE`; it did not implement
the exact DT-NBD-47 zombie rule. The source-only successor at that revision
implemented that rule: the same PID must report `Z` in `/proc/<pid>/status`, `Z` in
`/proc/<pid>/stat`, then `Z` again in `status`, with matching `Pid`/`Tgid` and
`Kthread: 0`. A live, malformed, unreadable, or race-to-live entry is refused
fail-closed; a stale daemon PID still blocks `PRODUCT_OFF`. The manufactured
preflight suite was 33/33. Later sealed releases integrated the rule, and
Attempt30 passed the pinned preflight and complete matrix.

## Attempt24 source-only terminal-authority hardening

RED added manufactured zram, hard-link alias, deleted executable, unreadable
proc, kernel-thread, and disappearance cases. GREEN makes preflight the only
`PRODUCT_OFF` authority, records two 15-second exact preflights per epoch, and
publishes owned no-replace logs plus an fsync/hard-link receipt before sealing.
TERM/INT preserve 143/130 through common cleanup. The source-only closure also
keeps worker TERM/KILL grace intervals within 1–30 seconds, denies those
configuration seams (and the failure-receipt race seam) in live mode, rejects
ambiguous or contradictory `PRODUCT_OFF` authority lines, and exercises
stubborn-worker and nonzero-`down` cleanup through the production control flow.
The local cell suite is 44/44 and no live product command ran; the manufactured
tests did create short-lived subprocesses and temporary fixtures. Step 3
remains partial.

Before public-pair eligibility, the Windows controller reopens both exact
cell-result roots and compares fresh context, summary, inventory, and internal
envelope fingerprints with the cached pair inputs. It derives public metrics
from those returned fresh summaries and rejects a cached summary field, status,
or receipt mutation. A deleted or replaced custody artifact rejects the direct
writer before it creates `public-evidence`.

The public writer emits its sanitized `pair-comparison.json` first, hashes its
exact on-disk bytes, and carries that one digest in both `pair-custody.json`
and the candidate record. Its final local artifact binding rejects a changed
comparison artifact; the repository validator independently repeats the same
raw-byte cross-binding after a candidate is copied into the repository.

The custody publication frontier now treats `evidence-envelope.json` as the
single atomic commit marker. `artifact-inventory.json` is preliminary until
that no-clobber envelope link exists; inventory-only or envelope-only debris,
including an inventory left by SIGKILL, is rejected by the validator. A
handled envelope publication failure or race removes only the inventory inode
whose device/inode/size/hash was recorded by this transaction and preserves
foreign artifacts before cleanup writes its diagnostic receipt.

`repository-governance (blocker)` — the async NBD recovery correction
(`47889e0`), non-destructive watchdog correction (`63bd3be`), later timeout
calibrations, and complete live matrix are implemented and evidenced.
Attempt30 supplies complete-matrix median/p99/deviation and backend
comparisons; final per-cell occupancy, cleanup, Relay, BINARY_MATCH receipts,
and root `validation.md`. Gate B, hosted PR checks, review, and merge remain.
No reboot or WSL shutdown is part of this record.

## Rollback trigger

Rollback/refuse on any one observable failure: `swapoff` error; residual
managed/ghost swap or daemon; BINARY_MATCH mismatch; lower-tier capacity below
`V + max(ceil(0.10 × V), 512 MiB)`; any failed installer rollback phase; stale
selector/provenance/manifest; CUDA cleanup failure; timeout classified as
anything other than `RED/watchdog_timeout_red/unverified_unknown`; or any live
seam/host action.

## Traceability

| RF | ITEM | commit |
| --- | --- | --- |
| RF-NBD-1..20 | ITEM-1..8 | partial — Attempt30 on sealed source `a365bda` completed 12/12 cells and 36/36 samples with exact BINARY_MATCH, custody, CUDA, and PRODUCT_OFF evidence; Gate B, hosted checks, PR review, and merge remain |
