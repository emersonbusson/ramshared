# SPEC — WSL2 NBD-only product readiness

> SSDV3 Step 3 is **live-partial**. The sealed 1 GiB NBD pilot and its
> before → action → after/BINARY_MATCH evidence are historical pilot evidence.
> The corrected benchmark implementation now passes the local
> source/static/manufactured gates, including cgroup admission, topology
> parity, size-dependent occupancy, pair-scoped CUDA custody, bounded WSL
> calls, live-seam refusal, and evidence custody. The ordered 1/2/4 GiB live
> benchmark matrix has not run on the reviewed release and remains the
> blocking live gate; local or manufactured results are never promoted into
> missing cells.
>
> The first independent Gate A found benchmark-contract blockers in the first
> harness revision. Those corrections are implemented and covered by the
> named local tests below. The final independent Gate A passed on the frozen
> pre-commit workspace. Until the reviewed release and legitimate live
> evidence pass, the matrix remains **NO-GO** for a product claim.

## Closed scope

### In now

- NBD-only WSL2 product transport and explicit refusal of ublk product use.
- Sealed release identity under `/opt/ramshared/releases/<version>`.
- `PRODUCT_OFF` versus `READY` versus `BLOCKED` state semantics.
- Lower-tier capacity formula, 1/2/4 GiB promotion, Relay, approval, and
  no-reboot gates.
- Swapoff-first teardown, `BINARY_MATCH`, named tests, rollback, and evidence.
- The real cascade teardown executor's pure/injected lifecycle plan and local
  refusal ordering contracts.
- A Windows-owned shared-host benchmark controller and a sealed WSL cell
  runner for disk-only/NBD comparisons.
- Manufactured installer rollback injection after every named post-write phase.
- A pair-scoped benchmark contract: identical zram base, distinct second tier,
  size-dependent occupancy proof, one CUDA context per bounded pair, bounded
  WSL calls, and sanitized context/hash evidence.

### Out now

- CI/workflow changes, release publication, Windows reboot, unsupervised swap
  pressure, or external communication.
- ublk product transport, ublk swap, `rmmod`, `modprobe -r`, or any module
  unload. A loaded `ublk_drv` module is tolerated as inert capability residue.
- `validation.md`, release documentation, or `MEMORY.md` edits.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-NBD-1, RF-NBD-3 | ITEM-1 — transport and legacy service boundary |
| RF-NBD-2, RF-NBD-9 | ITEM-2 — sealed release and binary identity |
| RF-NBD-4 | ITEM-3 — state model and status |
| RF-NBD-5, RF-NBD-10, RF-NBD-12 | ITEM-4 — capacity and swapoff-first lifecycle |
| RF-NBD-6, RF-NBD-14, RF-NBD-15, RF-NBD-16, RF-NBD-17, RF-NBD-20 | ITEM-5 — ordered size promotion, paired topology, occupancy, and comparison |
| RF-NBD-7, RF-NBD-8, RF-NBD-19 | ITEM-6 — approval, no reboot, Relay, and live-seam boundary |
| RF-NBD-11, RF-NBD-18 | ITEM-7 — safety, observability, context, and public evidence |
| RF-NBD-13 | ITEM-8 — exact legacy unit migration |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-NBD-1 | NBD is the only WSL2 product transport. | It is the supported day-one path and avoids the measured ublk teardown risk. |
| DT-NBD-2 | `/opt/ramshared/releases/<version>` is sealed and immutable after publication. | A live binary/config drift would invalidate evidence and rollback. |
| DT-NBD-3 | A separate atomic selector may change; a sealed version may not. | Selection rollback is bounded without rewriting release content. |
| DT-NBD-4 | Retire legacy ublk service wiring but never unload `ublk_drv`. | Service ownership is product policy; module lifetime is a host/kernel concern. |
| DT-NBD-5 | `PRODUCT_OFF`, `READY`, and `BLOCKED` are distinct states. | Safe quiescence is not positive readiness. |
| DT-NBD-6 | Use `L >= V + max(ceil(0.10V), 512 MiB)` with free absorbable bytes. | Demotion must have a numeric lower-tier sink, not nominal capacity. |
| DT-NBD-7 | Promotion order is 1 GiB, 2 GiB, then 4 GiB. | The pilot limits risk and prevents prestige-driven size selection. |
| DT-NBD-8 | Read-only gates need no approval; mutation needs current scoped approval. | Inspection and mutation have different authority boundaries. |
| DT-NBD-9 | The product action never requests reboot, WSL shutdown, or termination. The separately approved Windows watchdog may terminate only the selected distro after an outer deadline, and that cell is `RED`, never success evidence. | Product lifecycle and last-resort shared-host containment are different authority boundaries. |
| DT-NBD-10 | Relay `--check` is mandatory and `--reap` is never automatic. | Upstream lifecycle health is a prerequisite, not a repair side effect. |
| DT-NBD-11 | A live daemon must resolve to the selected sealed release. | Presence of a binary is not deployment identity. |
| DT-NBD-12 | A failure is a stable refusal, not a retry loop. | Deterministic identity/capacity/lifecycle failures are not transient. |
| DT-NBD-13 | `crates/ramshared-cli/src/cascade/cascade_io.rs` owns the NBD teardown effects; `cascade/lifecycle.rs` only derives a read-only status view. | The executor must prove `swapoff` before NBD disconnect or daemon stop through an injected local seam; status derivation cannot own lifecycle effects. |
| DT-NBD-14 | A conflicting `ramshared-cascade.service` is never overwritten by the normal installer. Replacement is allowed only with both the named release approval and a second current SHA-256 approval for one inactive, disabled, root-owned regular legacy file; the installer copies it to a sealed immutable backup before atomic replacement. | The exact observed legacy artifact, rather than a path or name alone, is the authority boundary. A wrong hash, a symlink, unknown metadata, activity, enablement, backup mismatch, or any failed replacement refuses or restores the prior unit. |
| DT-NBD-15 | Benchmark tier size is an exact runtime input to the selected sealed release, bound into the activation approval as `activate:<version>:vram=<MiB>:zram=1024`; accepted VRAM sizes are exactly 1024, 2048, and 4096 MiB. | Size promotion must not rewrite immutable release configuration or permit arbitrary host pressure. |
| DT-NBD-16 | One sample is a single-worker sequential anonymous-memory allocation in 64 MiB chunks using deterministic `shake256-v1` bytes, measured as `allocation_to_hold_ms` from the parent start receipt through the emitted `HOLD` receipt and followed by full checksum verification after release. | The workload is reproducible and intentionally incompressible, so zram cannot create a false tier-capacity result; it does not claim a block-I/O size or kernel queue depth. |
| DT-NBD-17 | Every disk-only and NBD cell has exactly three fresh samples. Report milliseconds, nearest-rank p99, median, population standard deviation, per-run checksums, swap deltas, and terminal state. | Three runs are the minimum contract and aggregation semantics must not drift between hosts. |
| DT-NBD-18 | For each size/condition pair, run disk-only control before NBD with the same worker, allocation `V + 2560 MiB`, cgroup `memory.high=1200 MiB`, emergency `memory.max=V+3072 MiB`, 64 MiB allocation chunks, one worker thread, and Windows GPU condition. Each sample begins after used-swap returns to its recorded baseline. | `memory.high` forces reclaim; using it as the hard `memory.max` makes valid swapcache/writeback trigger a memcg OOM before HOLD. The larger emergency maximum remains allocation-bounded, while occupancy receipts prove actual demotion. |
| DT-NBD-19 | Promotion is strictly P1 → Q2 → Q4. A refusal, timeout, corruption, ghost swap, failed BINARY_MATCH, or incomplete prior cell stops all larger sizes. | Larger cells cannot conceal a smaller-cell regression or safety refusal. |
| DT-NBD-20 | Before each bounded cell, Windows requires `free_vram_mib >= tier_mib + external_workload_mib + 512`. The bounded condition is exactly a 512 MiB generic CUDA allocation; idle uses zero. | The headroom gate is numeric and keeps at least 512 MiB uncommitted for the desktop/driver. |
| DT-NBD-21 | The campaign terminal state is `PRODUCT_OFF` after every cell and after any handled failure that reaches a verified cleanup. A watchdog deadline is the explicit exception: it is `RED/unverified_terminated`, not `PRODUCT_OFF`, and requires independent revalidation before any later action. The prior 1 GiB pilot may be restored only by a separate version-and-size-scoped approval after the complete matrix is validated. | Independent cells and failure cleanup must not depend on an inherited active product, and a distro termination must not be mistaken for cleanup. |
| DT-NBD-22 | A daemon-stop test fixture must first publish the observed post-swapoff state. It may not wait on a child after the production guard correctly refused stop for a still-published managed swap. | Test cleanup must model the production ordering contract and must never strand CI indefinitely. |
| DT-NBD-23 | Disk-only control owns one newly created 8 GiB scratch swapfile under the already validated lower-tier sink, at priority 100. Creation uses no-follow/exclusive semantics and records canonical path plus device/inode; cleanup revalidates identity, runs exact `swapoff`, proves `/proc/swaps` absence, then removes only that file. | The existing WSL disk swap has insufficient free capacity for the 4 GiB workload, and a broad or reusable scratch target would be destructive. A failed swapoff preserves the file and stops promotion. |
| DT-NBD-24 | Each disk/NBD comparison pair starts from the same clean product-owned 1 GiB zram tier at priority 200. Disk-only adds only its fresh exact 8 GiB scratch swapfile at priority 100; NBD adds only its exact `V`-sized NBD tier at priority 100. The pre-existing host lower sink remains untouched and is not the compared second tier. | A disk control that omits zram measures a different cascade and cannot be a control for NBD. A reusable or broad scratch target can damage host state. |
| DT-NBD-25 | Every sample starts a launcher already admitted to the cgroup with `memory.high=1200 MiB` and `memory.max=V+3072 MiB`, uses `V + 2560 MiB`, and releases a create-once start barrier only after membership is verified. The worker holds the allocation and emits a size-specific occupancy proof; the contract fields include both cgroup thresholds, `allocation_to_hold_ms`, `allocation_chunk_bytes`, and `worker_threads=1`. | Writing a worker PID into `cgroup.procs` after it has begun allocating permits unbounded host pressure. A hard 1200 MiB maximum kills valid reclaim; a fixed allocation or I/O labels cannot prove tier occupancy. |
| DT-NBD-26 | A bounded condition creates exactly one CUDA context/workload per size/condition pair, holds it across disk-only then NBD, and releases it only after both terminal observations. Idle pairs use no CUDA context. | Measuring the two backends under different GPU snapshots confounds backend latency with external pressure and invalidates the comparison. |
| DT-NBD-27 | Each pair and sample records sanitized execution context: UTC timestamp, branch/commit/dirty state, sealed release version and manifest digest, script hashes, kernel, GPU total/free/utilization/temperature, RAM and swap baseline, lower-tier identity/free capacity, exact command parameters, pair ID, watchdog outcome, and terminal classification. | A number without state, identity, or command cannot be reproduced or audited; private paths and host identity must not enter public evidence. |
| DT-NBD-28 | Fixture roots, swap/PID files, lower-sink paths, and fake GPU/process seams are test-only. Approved live mode must run with canonical sealed paths and reject any `RAMSHARED_*` fixture override; a seam-bearing artifact is invalid evidence. | Environment injection can make a fake product appear healthy and cannot close a live gate. |
| DT-NBD-29 | Every Windows-to-WSL process invocation has an explicit timeout and captured exit/stdout/stderr: selected-release discovery, each cell, and the outer watchdog termination. Argument/path handling must not depend on unbounded shell invocation. | A synchronous unbounded `wsl.exe` call can hang before the watchdog is armed, leaving no valid containment record. |
| DT-NBD-30 | Ready/release/approval artifacts use fresh create-once paths for each pair; a pre-existing handshake is a refusal. Cleanup is idempotent only after the expected owner identity is revalidated. | Reused handshakes can falsely report a CUDA allocation or release and cross-contaminate cells. |
| DT-NBD-31 | Promotion occurs only after a complete disk/NBD pair has passed integrity, occupancy, cleanup, and terminal-state checks. A deadline termination is `RED/unverified_terminated`, not `PRODUCT_OFF`, and stops all later promotion. | A controller exit or distro termination does not prove swapoff, daemon identity, or clean state. |
| DT-NBD-32 | Every complete pair reports `nbd_vs_disk_median_ratio` and `nbd_vs_disk_p99_ratio`. An optional historical baseline is compared only when its workload schema and environment fingerprint (kernel, GPU model/driver, zram/cgroup sizes, tier, condition, and command contract) match exactly. A matching cell is `RED` for median latency regression above 15% or p99 regression above 25%, and `YELLOW` for median above 10%, p99 above 15%, or population deviation above 2x; an absent baseline yields `BASELINE_CANDIDATE`, while a mismatched baseline is `NOT_COMPARABLE`, never a fabricated regression verdict. | Pair ratios quantify the backend tradeoff now; fingerprinted historical thresholds make later evolution/regression measurable without comparing different machines or workload contracts. |
| DT-NBD-33 | A live campaign supplies one exact expected 40-hex source commit. Bounded selected-release discovery reads sealed `SOURCE_COMMIT` and `SOURCE_TREE_STATE`; the campaign refuses unless the tree state is `clean` and the selected commit equals the expected reviewed commit. | `BINARY_MATCH` alone can still benchmark a correctly loaded but stale release; source identity binds deployment, evidence, and reviewed code. |
| DT-NBD-34 | The pair-scoped CUDA deadline must be at least `2 * per_cell_outer_timeout + 120 seconds`; configuration refuses before CUDA or WSL action when the inequality is false. | One CUDA context spans disk and NBD. A shorter hold can self-expire during a legitimate second cell and manufacture a backend failure. |
| DT-NBD-35 | `PlanOnly` is a local candidate-plan calculation and records `terminal_state=unobserved_plan_only`; it performs no product-state observation and must never assert `PRODUCT_OFF`. Only a live campaign after a successful bounded `PRODUCT_OFF` preflight may record that terminal state. | GPU planning is not lifecycle evidence; a pre-existing pilot can be active even when no live observation has occurred. |
| DT-NBD-36 | The no-argument builder produces a universal CI-compatible input bundle with blank lower-sink binding, input `SHA256SUMS`, and source identity. It has no `--lower-sink` mode. The attended installer requires `--lower-sink SAFE_ABSOLUTE_DIRECTORY` with the existing exact release approval; it validates canonical non-symlink directory identity and capacity metadata before and after copy, copies the verified input tree to staging, preserves the input manifest as `INPUT_BUNDLE_SHA256SUMS`, derives staging configuration, records the input-manifest digest and source/bound-sink identity in `INSTALL_PROVENANCE.json`, regenerates deterministic installed `SHA256SUMS`, writes the installed-manifest receipt, validates, seals, and atomically publishes. Every post-write phase is injection-tested and rolls back. | CI/release publication must not capture one host private identity. Binding only during the attended transaction preserves a universal input artifact while eliminating selector and environment-path TOCTOU on the installed host. |
| DT-NBD-37 | A successful cell writes a sanitized `context.json` schema 2, complete SHA-256 inventory, and an internal `ramshared-nbd-cell-evidence/v1` custody envelope. The context records UTC start, pair ID, full normalized argv, exact installed-release manifest and input-bundle manifest digest, source/script/zram/lower-tier identity, and a cell-local watchdog outcome. The inventory verifies every listed regular artifact; its own file and the internal envelope are excluded to avoid a recursive hash cycle, while the internal envelope hashes context, summary, and inventory. A disk-only cell has `BINARY_MATCH=N/A` and is never represented as public `ramshared-evidence/v1`. | A process exit and a partial inventory do not establish reproducible custody. A disk control cannot truthfully satisfy the public schema's `lifecycle.binary_match=true`, so cell custody and public pair evidence must be separate. |
| DT-NBD-38 | Windows creates exactly one strict `ramshared-evidence/v1` envelope for a completed disk/NBD pair only after both cell custody records validate, the NBD cell reports `BINARY_MATCH=PASS`, comparison succeeds, and pair cleanup is complete. The public envelope has `lifecycle.binary_match=true`; it carries only sanitized pair facts and SHA-256 custody references. `BASELINE_CANDIDATE` maps to a non-promotable `BASELINE` decision, `NOT_COMPARABLE` maps to a non-promotable `INCOMPARABLE` decision, compatible green maps to a qualified `PASS`, and red/yellow remain non-promotable. | Public evidence is a claim about the paired NBD path, not the disk control alone. Mapping the controller's comparison vocabulary explicitly prevents a candidate or incomparable row from being presented as a public pass. |
| DT-NBD-39 | Selected-release discovery records the exact installed `SHA256SUMS` digest as `installed_manifest_sha256`, validates `INSTALLED_MANIFEST_SHA256`, and records the preserved input-bundle manifest digest from `INSTALL_PROVENANCE.json`. Cell argv always binds `--expected-manifest-sha256` to the installed release manifest, never to a bundle-input digest; it accepts only `/opt/ramshared/releases/VERSION`, verifies the installed manifest, provenance/input digest, and exact bound sink before mutation, and never re-resolves `current` or invokes selector-derived lifecycle wrappers. The controller places generated public-pair artifacts under the campaign root with only repository-relative target names, labels them `candidate/noncanonical`, and does not append `docs/benchmarks/results.jsonl` or claim canonical evidence until the artifacts are copied into the repository and `node tools/ci/check-benchmark-evidence.mjs --check` validates them. | A bundle that produced an installed release and the installed release itself are different custody objects. The strict repository validator cannot validate host-local campaign files, so publication has an explicit copy-and-validate boundary. |
| DT-NBD-40 | Treat `/proc/swaps` usable size and `/sys/block/nbdN/size` capacity as untrusted decimal identity inputs. Before any Bash arithmetic, the cell rejects noncanonical, over-width, or out-of-range text against trusted tier-derived bounds. The exact contract is `capacity_sectors == tier_mib * 2048` and `usable_size_kib in [tier_mib * 1024 - 8, tier_mib * 1024]`. Context/custody records retain all three distinct values: canonical block `size_kib`, observed `capacity_sectors`, and observed `usable_size_kib`. Windows parses both observed fields as strict, non-overflowing integral values and enforces the same relation in `Get-NbdIdentity` and `Assert-CellEvidence`. | A 20-digit `/proc/swaps` value can wrap in Bash arithmetic and falsely equal a valid size. Omitting either observed field from Windows custody permits an incomplete or substituted NBD identity to reach comparison/public-evidence code. |

## Atomicity/rollback

| Layer | Atomic operation | Rollback frontier |
| --- | --- | --- |
| Release filesystem | Copy to a new version directory, hash/verify, seal, then switch selector. | Before selector switch, delete only the unselected staging directory; after switch, point back to the prior sealed version. |
| Product state | `PRODUCT_OFF → READY` only after all preconditions; no partial ready. | Any failed gate leaves `BLOCKED` or the prior `PRODUCT_OFF` state. |
| NBD lifecycle | Attach, verify identity, then publish swap; teardown does swapoff before disconnect. | If swapoff fails, keep daemon/NBD alive; do not disconnect or kill. |
| ublk retirement | Stop/disable only an identified legacy service under approval. | If ownership or active device is ambiguous, make no service mutation; leave `BLOCKED`. Never unload module. |
| Legacy cascade unit migration | Hash-bind one inactive legacy unit, seal its backup, then atomically replace the unit. | Missing/stale/mismatched approval, non-regular metadata, backup mismatch, or a failed replacement leaves or restores the prior unit and removes the new release selection. |
| Benchmark pair | Establish the same 1 GiB zram baseline, run disk-only with its exact fresh 8 GiB scratch, clean it, then run NBD with exact `V`; bounded CUDA context spans both. | Any topology drift, occupancy shortfall, failed identity cleanup, or pair mismatch invalidates the pair and stops promotion. |
| Host/WSL | No product-triggered reboot, shutdown, termination, or host configuration change. | The Windows outer watchdog is separate containment; if its bounded deadline terminates the selected distro, record `RED/unverified_terminated`, do not assert `PRODUCT_OFF`, and stop promotion until independent revalidation. |
| Evidence | Write one sanitized result after each bounded phase. | A failed write invalidates the gate; do not infer success from process exit alone. |

The installer has the following **post-write phase markers**. The manufactured
harness copies the installer into a temporary fixture, injects one `false`
immediately after exactly one marker, and then proves that the old selector and
unit state remain unchanged and the new destination is absent. The markers are
test metadata only; production code has no test environment switch.

| Marker | Post-write phase | Manufactured rollback frontier |
| --- | --- | --- |
| `release-roots-prepared` | Product and release roots prepared after trap installation. | No selected release is changed. |
| `staging-directory-created` | New version staging directory exists. | Remove staging only. |
| `release-copied` | Source release copy completed. | Remove staging only. |
| `input-bundle-manifest-copied` | Verified universal input manifest preserved in staging. | Remove staging only. |
| `lower-sink-bound` | Reviewed lower-sink identity written into staging configuration. | Remove staging only; the input bundle remains unmodified. |
| `installed-provenance-recorded` | Source/input-manifest/lower-sink provenance recorded. | Remove staging only. |
| `installed-manifest-regenerated` | Deterministic installed-release manifest regenerated after binding. | Remove staging only. |
| `installed-manifest-receipt-recorded` | Installed-manifest digest receipt written. | Remove staging only. |
| `staging-owner-normalized` | Staging ownership normalized. | Remove staging only. |
| `staging-directories-sealed` | Staging directory modes sealed. | Remove staging only. |
| `staging-executables-sealed` | Staging executable modes sealed. | Remove staging only. |
| `staging-files-sealed` | Staging non-executable modes sealed. | Remove staging only. |
| `staging-manifest-verified` | Copied release manifest verified. | Remove staging only. |
| `staging-seal-verified` | Copied release owner/mode tree verified. | Remove staging only. |
| `destination-published` | Verified staging directory atomically renamed and marked published. | Remove new destination; prior selector/unit remain. |
| `unit-staged` | New unit file staged when prior unit was absent. | Remove unit staging and new destination; absent prior unit remains absent. |
| `unit-linked` | New unit hard link published and ownership flag set. | Remove only the owned new unit and destination. |
| `unit-staging-removed` | Unit staging path removed after successful link. | Remove only the owned new unit and destination. |
| `legacy-unit-backed-up` | Exact legacy unit backup was sealed before replacement. | Retain the old unit; remove the staged release and leave the backup as immutable forensic evidence. |
| `legacy-unit-staged` | Replacement unit was staged after the sealed backup. | Retain the old unit and remove staging/release; do not publish the replacement. |
| `legacy-unit-replaced` | Replacement unit was atomically published. | Restore only the root-owned immutable backup whose SHA-256 still equals the approved legacy hash; otherwise report rollback failure without inferring recovery. |
| `selector-staged` | New selector symlink staged. | Remove selector staging and new destination; prior selector remains. |
| `selector-owner-normalized` | Staged selector ownership normalized. | Remove selector staging and new destination; prior selector remains. |
| `selector-published` | Atomic selector replacement completed. | Restore prior selector, then remove new destination. |
| `daemon-reloaded` | Unit metadata reload completed. | Restore prior selector and remove owned destination/unit state. |

Observable rollback triggers are: manifest/hash mismatch; mutable release
content; `BINARY_MATCH` failure; NBD or ghost swap in the wrong phase; lower
tier shortfall; failed swapoff; active/ambiguous ublk service; Relay refusal;
missing approval; reboot request; or any unbounded action. Preserve the prior
sealed release and return a stable refusal code.

## Kahneman map

| # | Question | Required evidence | Abort |
| --- | --- | --- | --- |
| #2 | Does a loaded ublk module prove product transport? | `NBD_ONLY_PRODUCT_SCOPE_REFUSAL` | Any ublk service/device in the ready path. |
| #3 | Is the selected artifact and capacity measured now? | `RELEASE_MANIFEST_MATCH`, `LOWER_TIER_CAPACITY_FORMULA` | Stale/nominal/ambiguous data. |
| #9 | Is each benchmark claim numeric? | `NBD_BENCHMARK_MATRIX` | Missing units, p99, deviation, or n<3. |
| #13 | Are legitimate and refusal boundaries paired? | `PRODUCT_OFF_IS_NOT_READY`, `CAPACITY_SHORTFALL_REFUSAL`, `RELAY_GATE_REFUSAL` | Any fail-open classification. |
| #15 | Is retry justified as transient? | `DETERMINISTIC_GATE_NO_RETRY` | Repeating identity, capacity, or service failures. |
| #16 | Can demotion exhaust its sink? | `LOWER_TIER_CAPACITY_FORMULA`, `SWAPOFF_FIRST_DEMOTION` | Shortfall, swapoff error, or active references. |
| #17 | Is activation/deactivation idempotent? | `NBD_LIFECYCLE_IDEMPOTENCY` | Double attach, double swap, or selector drift. |
| #18 | Is the decision made by the owning layer? | `UBLK_RETIREMENT_NO_MODULE_UNLOAD`, `RELAY_OWNER_BOUNDARY` | Guest code unloads a host-owned module or repairs Relay automatically. |

## Security checklist

- [x] Release files are root-owned, sealed, hash-verified, and selected by an
  explicit version; untrusted paths cannot become a ready binary.
- [x] Status/preflight is read-only by default; mutation requires a scoped
  approval and records no secret.
- [x] `/proc/swaps` and process/device identities are re-read at each lifecycle
  boundary to avoid stale or replaced NBD identities.
- [x] Unknown capacity, Relay, service, or binary data fails closed.
- [x] No command accepts a wildcard service/device target; ublk retirement is
  based on an enumerated, identity-bound set.
- [x] Product code never reboots, shuts down, or terminates WSL; the separate
  approved watchdog is bounded and emits `RED/unverified_terminated` in the
  static/manufactured contract. A live watchdog audit remains open.
- [x] The local harness proves worker admission before allocation; no
  post-start PID write is accepted as cgroup containment. Live occupancy proof
  remains open.
- [x] Manufactured approved-mode execution rejects fixture roots, fake
  swap/PID files, lower-sink overrides, and other test seams. The live seam
  exclusion still needs a real campaign receipt.
- [x] The local controller bounds every WSL call and uses fresh handshakes; a
  timeout is not inferred as cleanup. Live call receipts remain open.
- [x] Public evidence excludes usernames, private paths, tokens, addresses,
  raw logs, and machine-specific identifiers.
- [ ] Kernel/uAPI/IRQ/lock lifetime: N/A for this source-only slice;
  any future kernel surface requires its own SPEC and platform gate.

## Files create/modify/delete

The following are the actual source-partial paths. Local source/static evidence
does not establish a live WSL2 product result.

| Path | Action | Contract / test owner |
| --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | Implemented | Pure state, capacity, and refusal model. |
| `crates/ramshared-tier/src/cascade.rs` | Implemented shared integration | NBD-only transport and priority/lifecycle integration. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | Implemented | The actual NBD teardown executor and its injected `swapoff → disconnect → daemon-stop` contract. `cascade/lifecycle.rs` remains read-only status derivation and does not own those effects. |
| `crates/ramshared-wsl2d/src/state.rs` | Not changed by this slice | Daemon identity/terminal-state handoff remains separately owned. |
| `scripts/safety/nbd-product-preflight.sh` | Implemented | Read-only Relay, release, capacity, service, and host gate; exact sealed-release binding validates the immutable lower-sink binding without emitting its path. |
| `scripts/safety/test-nbd-product-preflight.sh` | Implemented | Static/manufactured refusal, idempotency, every-post-write rollback, and produced-bundle-to-sealed-preflight test. |
| `scripts/safety/install-cascade-boot.sh` | Implemented | Sealed installer rollback markers and bounded cleanup frontier. |
| `scripts/safety/cascade-up.sh` / `cascade-down.sh` | Implemented shared lifecycle surface | NBD-only startup and safe teardown. |
| `scripts/safety/cascade_pressure_integrity_worker.py` | Implemented | Deterministic incompressible benchmark pattern without changing the legacy default. |
| `scripts/safety/nbd-benchmark-cell.sh` | Implemented against DT-NBD-24..31, DT-NBD-36..37 | Root-only sealed disk/NBD pair cell, identical zram base, exact scratch/NBD second tier, start-barrier cgroup admission, size-dependent occupancy, three samples, exact aggregation, cleanup, BINARY_MATCH, selector-free reviewed-release binding, and complete sanitized custody. Local gates pass; live validity remains unproven. |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` | Implemented against DT-NBD-25/30 | Minimal in-cgroup launcher with create-once ready/start receipts; no allocation may begin before parent membership proof. |
| `scripts/safety/nbd-benchmark-lib.sh` | Implemented against DT-NBD-23/24 | Shared read-only scratch identity predicates and exact swapoff-first cleanup transaction used by live code and manufactured failure tests. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | Implemented against DT-NBD-24..31 | Manufactured aggregation, topology parity, start-barrier, occupancy, approval, ordering, residual-swap, checksum, seam, custody, and timeout refusals; no live pressure. |
| `scripts/p0/Start-CudaVramWorkload.ps1` | Implemented harness dependency | One bounded CUDA context with create-once ready/release handshakes and unconditional resource cleanup on success, timeout, or startup failure. It is a harness component, not a product service; real CUDA execution remains environment-bound. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | Implemented against DT-NBD-26..31 | Shared-host approval, numeric GPU-headroom gate before each pair and after CUDA ready, one CUDA context per pair, bounded WSL calls, strict promotion, outer watchdog classification, and sanitized matrix evidence. Plan/manufactured paths pass; live matrix is not run. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | Implemented against DT-NBD-26..31 | Plan-only/manufactured PowerShell 5.1 contracts, bounded-call and one-context checks, watchdog RED/unverified-termination checks, and forbidden-host-action checks. Focused static suite passes. |
| `scripts/package/build-linux-bundle.sh` | Implemented shared packaging surface | No-argument generic CI/source-distributable input archive: explicitly unbound, source-identified, and input-manifested. It never binds a sink or installs in place. |
| `docs/governance/capability-observations.generated.json` | Generated observation | Regenerated and checked by `node tools/ci/generate-capability-observations.mjs --check`; records capability facts only and never substitutes for live benchmark evidence. |
| `scripts/windows/Test-WindowsCiStatic.ps1` | Updated Windows static wrapper | Includes the NBD matrix static harness in the complete PowerShell syntax/static sweep; final wrapper rerun remains a pre-commit check. |
| `docs/specs/no-milestone/wsl2-nbd-product-readiness/IMPL.md` | Updated as `partial` | Exact local numbers and environment-bound gaps; no new live claim. |
| `validation.md` | Append later | Only after approved live before/action/after evidence. |

Delete no existing code, unit, module, release artifact, validation record, or
`MEMORY.md` entry as part of this SPEC. “Retire” means remove the legacy ublk
product ownership and startup path, not unload the kernel module.

## Observability

Every gate emits one sanitized record with:

| Signal | Required fields | Pass condition |
| --- | --- | --- |
| Release | version, manifest digest, owner/mode result, selector | Exact sealed release selected. |
| Product | state, readiness reason, transport, daemon PID | State matches all observed postconditions. |
| Capacity | `V`, `L`, margin, sink identity, units | Formula passes with free bytes. |
| Relay | script version/result, candidate count, reason | Read-only `--check` returns clean. |
| Identity | `/proc/<pid>/exe` resolved path and digest | Equals selected release manifest. |
| Swap | exact device, size, used, priority, phase | NBD absent before disconnect/stop. |
| Legacy ublk | unit/device inventory and module state | No active product owner; no unload attempted. |
| Benchmark | pair ID, mode, size, condition, run, `allocation_to_hold_ms`, `allocation_chunk_bytes`, `worker_threads`, `median_allocation_to_hold_ms`, `p99_allocation_to_hold_ms`, `population_stddev_allocation_to_hold_ms`, checksums, zram/second-tier deltas, integrity | All cells have n≥3, size-specific occupancy proof, and no corruption/ghost. |
| Comparison | NBD/disk median and p99 ratios, optional baseline fingerprint, per-cell deltas, `BASELINE_CANDIDATE`/`NOT_COMPARABLE`/`GREEN`/`YELLOW`/`RED` | Ratios are finite and positive; a historical verdict exists only for an exact compatible fingerprint. |
| Execution context | UTC start, pair ID, branch, commit, dirty state, full normalized argv, exact reviewed sealed root/version/manifest digest, script hashes, kernel, and zram identity | The result can be reproduced and independently tied to the reviewed release without leaking private host identity. |
| Capacity/headroom | `V`, lower sink type/identity digest, free absorbable `L`, formula margin, GPU free before pair and after CUDA-ready, external workload and reserve | Capacity/headroom gates are numeric, current, tied to the pair rather than a stale plan, and cannot redirect a scratch path through an environment seam. |
| Containment | WSL call name, start/deadline/exit, stdout/stderr artifact hashes, watchdog outcome, terminal classification | Every call is bounded; `unverified_terminated` is RED and never a clean-state claim. |
| Seam provenance | canonical sealed paths, live-seam refusal result, fixture/test-mode marker | Approved live evidence contains no fixture override or env-injected identity. |
| Evidence custody | context/summary/inventory digests, each required artifact's bytes/digest, schema-valid internal `ramshared-nbd-cell-evidence/v1` envelope | Every non-recursive regular artifact is accounted for; only the Windows-owned completed-pair step may derive a public envelope. |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Update only if the NBD-only product boundary changes in Step 3. |
| `docs/reliability/GAP-REGISTER.md` | Existing ublk deferred gap remains authoritative; update only with evidence. |
| `docs/BENCHMARKS.md` and results | Update only after the named matrix is run. |
| `validation.md` | Append only after live approved evidence. |
| `IMPL.md` | Keep the numeric Step 3 record current; it remains `partial` until the live matrix closes. |
| `AGENTS.md`, `CLAUDE.md`, `.claude/rules/*` | N/A — no convention change. |
| `docs/INDEX.md` | Regenerate as a generated index for this pack. |

## Implementation order

No gaps are permitted between items:

1. **ITEM-1:** freeze NBD-only transport and enumerate legacy ublk service/device ownership.
2. **ITEM-2:** implement sealed-release selector and manifest/BINARY_MATCH checks.
3. **ITEM-3:** implement pure `PRODUCT_OFF`/`READY`/`BLOCKED` status model.
4. **ITEM-4:** implement capacity arithmetic, lower-tier selection, swapoff-first lifecycle, and refusal codes.
5. **ITEM-5:** implement the pairwise 1/2/4 GiB promotion contract: identical
   1 GiB zram, exact disk scratch versus exact NBD second tier, cgroup
   start-barrier and size-occupancy proof, exact labels, n≥3 evidence, and
   promotion ordering.
6. **ITEM-6:** wire explicit approval, no-reboot refusal, and Relay `--check` gate.
7. **ITEM-7:** run local source/static/manufactured tests and record an explicit
   `partial` `IMPL.md`; run approved live E2E before any root validation record
   or readiness claim.
8. **ITEM-8:** add an exact SHA-256-bound legacy-unit migration with backup and
   rollback tests; retain normal conflict refusal when that separate approval is absent.

## Required tests matrix

These are contractual names for the source-partial implementation. Exact local
command results and counts belong in `IMPL.md`; the live rows below remain
environment-bound and are not inferred from a manufactured test.

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover / pass condition |
| --- | --- | --- | --- | --- |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `nbd_only_transport_is_the_only_ready_value` | unit | #2 | ublk is never a product-ready transport. |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `lower_tier_formula_uses_ten_percent_or_512_mib` | unit | #3/#16 | Exact ceiling formula and units. |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `capacity_shortfall_refuses_before_mutation` | unit/refusal | #13/#16 | Shortfall, stale, overflow, and unknown sink refuse. |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `product_off_is_not_ready_alias` | unit/refusal | #13 | Off state remains distinct from ready state. |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `deterministic_gate_failure_is_not_retried` | unit/refusal | #15 | Identity/capacity/service failure is one bounded attempt. |
| `crates/ramshared-tier/src/nbd_readiness.rs` | `activation_and_deactivation_are_idempotent` | unit | #17 | Replayed commands produce one effect. |
| `crates/ramshared-tier/src/cascade.rs` | `ublk_service_is_not_a_product_dependency` | integration/refusal | #2/#18 | NBD path does not start or select ublk. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | **TestName: `swapoff_completes_before_nbd_disconnect`** | injected local contract | #16 | The executor emits `swapoff` before NBD disconnect and daemon stop, without opening a device. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | **TestName: `failed_swapoff_keeps_daemon_and_device_alive`** | injected local refusal | #13/#16 | An injected NBD `swapoff` error emits neither disconnect nor daemon stop and preserves the lifecycle records. |
| `crates/ramshared-cli/src/cascade/cascade_io.rs` | **TestName: `setup_new_cascade_uses_only_temp_runtime_and_direct_child_fixture`** | bounded regression | #15/#16 | The fixture publishes the post-swapoff observation before exact-child stop and completes within 20 seconds without a leaked child. |
| `scripts/safety/nbd-product-preflight.sh` | `release_manifest_and_modes_are_verified` | static | #3 | Unsealed/mutated release refuses. |
| `scripts/safety/nbd-product-preflight.sh` | `binary_match_rejects_stale_or_deleted_daemon` | static/refusal | #3 | Live executable equals sealed manifest. |
| `scripts/safety/nbd-product-preflight.sh` | `relay_check_failure_blocks_readiness` | manufactured refusal | #13/#18 | Relay candidate/ambiguity blocks; no reap. |
| `scripts/safety/nbd-product-preflight.sh` | `reboot_and_shutdown_requests_are_refused` | static/refusal | #13 | No host restart command is executed. |
| `scripts/safety/nbd-product-preflight.sh` | `legacy_ublk_retirement_never_unloads_module` | static/refusal | #18 | No `rmmod`/`modprobe -r`; service ownership is bounded. |
| `scripts/safety/test-nbd-product-preflight.sh` | `product_off_ready_blocked_state_matrix` | manufactured | #13 | All state distinctions and reason codes match. |
| `scripts/safety/test-nbd-product-preflight.sh` | `capacity_sink_identity_and_alignment_refusals` | manufactured refusal | #13/#16 | Wrong/ambiguous sink and alignment refuse. |
| `scripts/safety/test-nbd-product-preflight.sh` | `n3_or_ublk_capability_does_not_promote_nbd_product` | manufactured refusal | #2 | Capability is not product readiness. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `installer_every_post_write_phase_rolls_back` | manufactured rollback | #13/#17 | Inject after each of the 25 named post-write markers; preserve prior selector/unit state and remove the new destination. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_unit_migration_requires_exact_hash_and_restores_on_failure` | manufactured approval/rollback | #3/#13/#17 | Default conflict refusal; only an inactive root-owned regular unit matching the supplied SHA-256 is backed up and atomically replaced; stale/mismatched metadata or injected failure retains/restores it. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `corrupt_published_legacy_backup_refuses_before_replacement` | manufactured refusal | #3/#13 | A post-publish backup hash mismatch refuses before replacement staging and leaves the legacy unit unchanged. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_backup_root_symlink_is_refused` | manufactured refusal | #3/#13 | A pre-existing backup-root symlink refuses before any backup write. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_restore_reloads_systemd_after_daemon_reload` | manufactured rollback | #13/#16 | A failure after daemon reload restores the old unit and reloads systemd again. |
| `scripts/safety/Test-CascadePressureIntegrityWorker.sh` | `shake256_pattern_is_deterministic_and_incompressible` | unit/manufactured | #3/#9 | Equal inputs produce equal bytes and the sample refuses the compressible legacy pattern. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_aggregation_is_exact_and_requires_three_runs` | manufactured | #9/#13 | Median, nearest-rank p99, population deviation, units, and n=3 are exact; missing/duplicate/nonfinite samples refuse. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `disk_control_and_nbd_candidate_share_one_workload_contract` | static/manufactured | #3/#9/#16 | Both modes use one 1 GiB zram base, the same `V + 2560 MiB` allocation, `memory.high=1200 MiB`, `memory.max=V+3072 MiB`, `allocation_chunk_bytes=67108864`, `worker_threads=1`, and disk-only precedes NBD; only the second tier differs. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `cgroup_high_forces_reclaim_without_hard_limit_oom` | manufactured/static | #3/#13/#16 | The cgroup writes the 1200 MiB reclaim threshold to `memory.high`, writes the allocation-derived emergency bound to `memory.max`, and every sample/context carries both exact values. |
| `scripts/safety/nbd-benchmark-lib.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `sample_baseline_republication_is_exact_and_ordered` | manufactured/refusal | #3/#13/#16 | Between runs, the exact lower and zram swaps are removed lower-first and republished zram-then-lower at priorities 200/100; cardinality, type, ghost absence, and identity are checked before and after. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `activity_refusal_persists_exact_observed_deltas` | manufactured/static | #3/#13/#18 | Before any occupancy verdict, the cell persists exact observed zram/NBD/disk/scratch deltas and configured thresholds so a RED cannot discard the measurement needed to diagnose it. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `swap_device_classifier_requires_exact_device_names` | manufactured | #3/#13/#18 | Classify only `/dev/zramN` as zram and `/dev/nbdN` or `/dev/ublkbN` as the managed second tier; every other swap path, including a directory component containing `nbd`, remains disk. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_start_barrier_and_size_occupancy_contract` | manufactured/refusal | #3/#13/#16 | Worker admission is proven before allocation; each tier size has the expected zram/second-tier occupancy delta; a fixed-size or post-start cgroup sample refuses. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_cleanup_refuses_ghost_or_residual_swap` | manufactured refusal | #13/#16/#17 | Baseline drift, checksum failure, failed swapoff, live daemon, NBD/ublk ghost, or missing BINARY_MATCH cannot pass. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `disk_control_scratch_is_exclusive_identity_bound_and_swapoff_first` | manufactured refusal | #13/#16/#17 | Existing/symlink/foreign scratch refuses; cleanup rechecks device/inode and never removes after failed swapoff. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_live_seams_are_unavailable_in_approved_mode` | manufactured refusal | #13/#18 | Fixture roots, fake swap/PID files, and lower-sink overrides cannot enter an approved live invocation. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `reviewed_release_binding_and_evidence_custody_refuse_drift` | manufactured refusal | #3/#13/#16 | Exact root/version/source/installed-manifest/pair binding rejects selector and identity drift; every required artifact and hash chain is validated before a cell result can pass. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `cell_custody_envelope_is_internal_and_nonpromotable` | manufactured refusal | #3/#13/#16 | A cell emits only `ramshared-nbd-cell-evidence/v1`; disk `BINARY_MATCH=N/A` or any public-v1 cell envelope is refused. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `evidence_inventory_hashes_and_internal_cell_envelope_are_validated` | manufactured/refusal | #3/#13/#18 | Every nonrecursive cell artifact is inventoried and hash-bound; tamper, unlisted content, or a public-v1 cell envelope refuses. |
| `scripts/safety/test-nbd-benchmark-cell.sh` | `cell_context_writer_is_unique_and_schema_v2` | static/refusal | #3/#18 | Exactly one live context writer and call remain, and the emitted context schema is exactly 2. |
| `scripts/safety/nbd-benchmark-cgroup-launch.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `benchmark_start_barrier_launcher_is_in_cgroup_before_exec` | manufactured/refusal | #13/#16 | The exact launcher PID is admitted before the worker starts; a fresh barrier is required before exec. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `nbd_second_tier_identity_is_observed_and_separate_from_lower_sink` | manufactured/identity | #3/#16/#18 | The NBD identity is derived from the exact swap row, block device, size, priority, server PID, and manifest-matched daemon; the installed lower-sink identity remains separate. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `nbd_identity_accepts_bounded_mkswap_overhead_and_exact_usable_size` | manufactured/identity | #3/#13/#18 | The exact NBD block capacity comes from the sysfs sector count, while `/proc/swaps` usable KiB may differ by at most 8 KiB of mkswap metadata allowance; malformed or wrong capacity and excessive loss refuse. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `nbd_usable_size_overflow_refuses_before_bash_arithmetic` | manufactured/refusal | #3/#13/#16/#18 | The exact `/proc/swaps` usable-size fixture `18446744073710600192` refuses as untrusted over-range text before Bash arithmetic can wrap it; a legitimate bounded usable size remains independent of exact block capacity. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `windows_nbd_capacity_and_usable_size_are_strict_custody_fields` | manufactured/refusal | #3/#13/#16/#18 | `Get-NbdIdentity` and `Assert-CellEvidence` require strict integral, non-overflowing `capacity_sectors` and `usable_size_kib`; the capacity is exactly `tier_mib * 2048`, usable size is within the inclusive 8 KiB mkswap interval, and missing, malformed, wrong, over-range, or excessive-loss values refuse before custody/comparison promotion. |
| `scripts/safety/nbd-benchmark-cell.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `nbd_second_tier_identity_refuses_missing_duplicate_foreign_and_substitution` | manufactured refusal | #13/#16/#18 | Missing, duplicate, foreign, size/priority/PID/executable/hash mismatch, and lower-sink-hash substitution all refuse. |
| `scripts/package/build-linux-bundle.sh` + `scripts/safety/test-nbd-product-preflight.sh` | **TestName: `sealed_bundle_contains_benchmark_runner_and_worker`** | manufactured package/preflight | #3/#13 | A no-argument universal bundle contains every runner/worker dependency, source identity, and input manifest but no host binding; an attended fake-root installer derives a bound sealed release that passes read-only manufactured preflight without an environment lower-sink seam. |
| `scripts/p0/Start-CudaVramWorkload.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `cuda_workload_uses_fresh_handshakes_and_finally_releases_context` | static/manufactured refusal | #13/#16/#17 | Ready/release files are create-once; startup failure, deadline, and normal completion release device memory/context. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | `matrix_promotes_only_after_complete_prior_pair` | plan/manufactured refusal | #13/#15 | Exact P1/Q2/Q4 order; refusal or partial stops larger tiers, and a pair is disk then NBD under one snapshot. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | `wsl_release_discovery_and_cells_are_bounded` | static/manufactured refusal | #13/#16 | Selected-release discovery, every cell, and containment termination have explicit deadlines and captured outcomes. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `reviewed_release_preflight_and_deactivation_remain_pinned_after_selector_flip` | manufactured/refusal | #13/#16/#17 | Full preflight and deactivation use the reviewed exact release and pinned CLI before/after mutation; a selector flip never executes another release. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `manufactured_nbd_identity_behavior` | manufactured/refusal | #3/#13/#16 | Windows validates the exact NBD device tuple, tier size, priority, server, daemon/hash, and separation from the installed lower sink. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `matrix_inventory_is_ps51_safe_and_repository_relative` | manufactured | #3/#18 | PowerShell 5.1 writes normalized relative inventory paths without a multi-character `TrimStart` conversion failure. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `windows_command_line_preserves_exact_wsl_shell_argument` | manufactured | #3/#13/#18 | The Windows PowerShell 5.1 process launcher preserves an argument containing spaces, quotes, dollar signs, and backslashes byte-exactly instead of corrupting the bounded WSL shell command. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `selected_release_discovery_uses_direct_pinned_argv` | manufactured/refusal | #3/#13/#18 | Selected-release discovery uses only bounded direct Linux argv reads, validates the pinned installed/input manifests and provenance in PowerShell, and never passes an inline shell program through `wsl.exe`. |
| `scripts/safety/nbd-benchmark-lib.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `scratch_identity_is_stable_for_empty_and_allocated_regular_file` | manufactured/refusal | #3/#13/#18 | Scratch identity uses stable numeric metadata, rejects symlinks/non-regular files, and remains exact when the newly created empty file is allocated before publication. |
| `scripts/safety/nbd-benchmark-lib.sh` + `scripts/safety/test-nbd-benchmark-cell.sh` | `disk_control_accepts_fresh_zero_used_zram_with_exact_topology` | manufactured/refusal | #3/#13/#18 | Disk control accepts one freshly published zero-used zram at priority 200 plus the exact scratch at priority 100, while refusing duplicates, wrong priorities, NBD, and ghost rows. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `installed_release_and_input_bundle_manifests_are_distinct` | manufactured refusal | #3/#13 | Discovery records installed and exposed input-bundle manifests separately; cell argv binds only the installed release manifest. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `plan_only_terminal_state_is_unobserved` | plan/manufactured refusal | #1/#13 | Plan-only output never claims `PRODUCT_OFF`; only bounded live preflight supplies that observation. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` | `one_cuda_context_covers_one_disk_nbd_pair` | static/manufactured refusal | #3/#9 | One bounded CUDA process/context starts once per size/condition pair, remains ready across both modes, and releases only after NBD terminal evidence. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `pair_ratios_and_compatible_baseline_thresholds_are_exact` | manufactured comparison/refusal | #3/#9/#13 | Positive finite NBD/disk ratios are exact; absent baseline is a candidate, mismatch is not comparable, and compatible 10/15/25% plus 2x thresholds classify deterministically. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `public_pair_evidence_requires_nbd_binary_match_and_comparison` | manufactured refusal | #3/#13/#16 | No public pair envelope is emitted until two internal custody records, NBD `BINARY_MATCH=PASS`, comparison, and cleanup all pass. |
| `scripts/windows/Invoke-NbdBenchmarkMatrix.ps1` + `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `public_pair_evidence_maps_baseline_candidate_incomparable_and_pass_exactly` | manufactured | #3/#9/#13 | `BASELINE_CANDIDATE→BASELINE`, `NOT_COMPARABLE→INCOMPARABLE`, and compatible green→qualified `PASS`; non-pass evidence never promotes. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` + `tools/ci/check-benchmark-evidence.test.mjs` | `public_pair_evidence_matches_repository_validator_fixture` | pure fixture | #3/#18 | A copied, sanitized public-pair fixture is accepted by the unmodified `check-benchmark-evidence.mjs` logic; campaign-root output remains candidate/noncanonical until that copy-and-validate boundary. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `bounded_cell_requires_numeric_gpu_headroom` | manufactured refusal | #3/#13 | Require `tier + 512 MiB external + 512 MiB reserve`; unknown/short headroom refuses before WSL action. |
| `scripts/windows/Test-NbdBenchmarkMatrixStatic.ps1` | `watchdog_timeout_is_red_and_unverified_termination` | manufactured refusal | #13/#16 | Only timeout containment may call `wsl --terminate`; it is recorded `RED/unverified_terminated`, never asserted `PRODUCT_OFF`, never retried, and never promoted. |
| `scripts/windows/Test-WindowsCiStatic.ps1` | `windows_static_wrapper_includes_nbd_benchmark_harness` | static | #3/#18 | The complete PowerShell wrapper parses and executes the NBD matrix static test and the CUDA harness checks. |
| `tools/ci/generate-capability-observations.test.mjs` + `docs/governance/capability-observations.generated.json` | `capability_observations_are_deterministic_and_checked` | generated/static | #3/#18 | `node tools/ci/generate-capability-observations.mjs --check` passes; generated capability facts remain separate from live benchmark evidence. |
| `scripts/safety/cascade-up.sh` + `scripts/safety/cascade-down.sh` | `nbd_lifecycle_before_action_after` | live E2E | #16/#17 | Approved NBD run, swapoff-first teardown, no ghost. |
| `scripts/safety/nbd-product-preflight.sh` | `relay_gate_before_action_after` | live E2E | #18 | Relay check is clean before and after; no automatic reap. |
| release/cascade surface | `NBD_BENCHMARK_MATRIX` | live benchmark | #9 | 1/2/4 GiB cells, n≥3, median/p99/deviation/integrity. |

### NBD environment-bound matrix

| TestName / evidence | Local source result | Live status | Why it remains partial |
| --- | --- | --- | --- |
| `nbd_lifecycle_before_action_after` | Injected ordering/refusal plus live pilot | `PASS` at 1 GiB | Live before/action/after exists; larger-size lifecycle remains coupled to the matrix. |
| `relay_gate_before_action_after` | Manufactured refusals plus live read-only gate | `PASS` for the pilot | Relay was clean before/after without automatic reap. |
| `NBD_BENCHMARK_MATRIX` | Corrected cell/preflight/controller implementation; cell 18/18, preflight 26/26, public-evidence validator 15/15, complete Windows static wrapper, and focused independent Gate A passes | `PARTIAL` / `NO-GO` | No complete 1/2/4 GiB disk-only/NBD n=3 matrix has run on the reviewed sealed release; legitimate live before/action/after evidence remains required. |
| `BINARY_MATCH` | Static refusals plus live selected release | `PASS` for the pilot | Each matrix NBD cell must repeat the identity proof. |

### Benchmark matrix

Each cell has exactly three independent runs on the declared WSL2 surface.
Record `allocation_to_hold_ms`, `allocation_chunk_bytes`,
`worker_threads`, workload direction, `V`, lower-tier `L`, lower-tier identity,
VRAM free before pair and after CUDA-ready, NBD/disk transport, p50/median,
p99, standard deviation or equivalent deviation, per-run checksums, occupancy
deltas, ghost-swap result, watchdog outcome, and terminal state. Do not label
the memory-allocation chunk as `block_size` or the worker count as
`queue_depth`.

The shared workload contract is `shake256-v1`, 64 MiB sequential chunks,
`worker_threads=1`, a 1200 MiB cgroup reclaim threshold plus allocation-derived
emergency maximum, and an allocation held at
`V + 2560 MiB`. A cgroup-resident launcher must release a fresh start barrier
only after its own membership is verified. Disk-only and NBD use those exact
inputs and the same fresh 1 GiB zram tier at priority 200. The disk control
uses one newly created exact 8 GiB scratch swapfile at priority 100; NBD uses
one exact `V`-sized NBD tier at priority 100. The pre-existing WSL lower sink
remains untouched and is not the compared second tier. The NBD block-device
capacity must independently equal the cell tier size before sampling; the
`/proc/swaps` usable-size field may be up to 8 KiB smaller because of mkswap
metadata and is recorded separately. Both observed fields are untrusted text
until bounded against the trusted tier: capacity is exactly `tier_mib * 2048`
sectors and usable size is in `[tier_mib * 1024 - 8, tier_mib * 1024]` KiB.
The cell must reject before evaluating an over-width decimal value in Bash
arithmetic; Windows must carry and revalidate the same three-value identity in
its internal custody gate.

For `bounded`, one Windows-owned 512 MiB CUDA context is created after the
numeric headroom gate and held across disk-only then NBD for the same pair. GPU
free headroom is rechecked immediately before the pair and after the CUDA-ready
receipt. `idle` uses no external CUDA context. A sample timeout is 120 seconds;
each cell has its configured outer deadline, and the CUDA hold deadline must
remain at least two cell deadlines plus 120 seconds. Selected-release discovery
and watchdog termination have their own bounded deadlines.

| Cell | Tier size | Condition | Required comparison |
| --- | ---: | --- | --- |
| P1-NBD-IDLE | 1 GiB | NBD + zram + lower sink, idle host | Disk-only control at same protocol/load. |
| P1-NBD-BOUNDED | 1 GiB | Same path with an explicitly approved bounded generic GPU condition | No host instability; demotion/teardown remains green. |
| Q2-NBD-IDLE | 2 GiB | Same matrix after P1 promotion | P1 result and disk-only control. |
| Q2-NBD-BOUNDED | 2 GiB | Same bounded condition | Capacity and integrity remain green. |
| Q4-NBD-IDLE | 4 GiB | Same matrix after Q2 promotion | Q2 result and disk-only control. |
| Q4-NBD-BOUNDED | 4 GiB | Same bounded condition | No promotion on partial or environment-bound result. |

The matrix is not a license to pressure the shared daily host. If the declared
surface cannot provide the bounded condition, mark the pair `PARTIAL` and do
not infer a product claim from idle rows. If the outer watchdog terminates the
selected distro, mark the pair `RED/unverified_terminated`; do not mark it
`PRODUCT_OFF`, do not infer cleanup, and stop all promotion until an
independent revalidation observes the terminal state.

## Validation checklist

Local Step 3 checks are source/static/manufactured only. A passing local check
does not complete an environment-bound row:

- [x] `cargo fmt --all -- --check`, `cargo clippy -D warnings`, and the named
  pure-Rust package tests pass locally; exact counts are recorded in `IMPL.md`.
- [x] `node tools/ci/check-rust-slice-coverage.mjs` passes for each touched
  business-logic file at minimum 80%; no coverage claim is made for shell
  orchestration.
- [ ] The exact NBD pure-model owner is `wsl2-nbd-product-readiness` and its
  per-file gate is:

```bash
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-tier --files crates/ramshared-tier/src/nbd_readiness.rs --min 80 --report-json tmp/wsl2-nbd-product-readiness-cov.json
```

  `crates/ramshared-tier/src/lib.rs` is intentionally not a second line
  coverage owner: its exact two-line N3/NBD module declaration projection is
  owned by `microsoft-native-vram-memory-tier-n3-module-export-glue` in the N3
  SPEC and must not gain a re-export or any other glue.
- [ ] Static/manufactured tests cover legitimate and refusal pairs in the
  corrected matrix implementation. The exact DT-NBD-40 tests
  `nbd_usable_size_overflow_refuses_before_bash_arithmetic` and
  `windows_nbd_capacity_and_usable_size_are_strict_custody_fields` are a
  current local Gate A `NO-GO` until their RED reproducers are made green; no
  earlier local-green result substitutes for them.
- [x] Manufactured release install proves owner/mode/hash immutability and all
  25 named post-write selector/unit/destination rollback frontiers.
- [x] Readiness proves NBD-only, no active ublk service/device, and no module
  unload in the local refusal contract.
- [x] Static/manufactured `BINARY_MATCH` refusal and the historical 1 GiB pilot
  prove the selected daemon identity; every new live NBD cell remains open.
- [x] Relay `--check` refusal and no-automatic-reap behavior are covered
  locally; a fresh before/after gate for the new campaign remains open.
- [ ] No product-triggered reboot, WSL shutdown, or termination. If the
  separately approved outer watchdog terminates the selected distro after its
  deadline, evidence says `RED/unverified_terminated`, not `PRODUCT_OFF`, and
  promotion stops.
- [ ] Live E2E is before → action → after on WSL2 itself, with checksum,
  `/proc/swaps`, daemon, capacity, and terminal-state evidence.
- [x] The local pair contract starts the same 1 GiB zram, differs only in its
  second tier, and uses one CUDA context for both bounded cells; live execution
  remains open.
- [x] Local tests prove worker start-barrier membership and size-dependent
  `V + 2560 MiB` occupancy under the 1200 MiB `memory.high` threshold and
  `V + 3072 MiB` emergency maximum; labels are
  `allocation_to_hold_ms`, `allocation_chunk_bytes`, and `worker_threads`.
- [x] Local tests prove every WSL call and handshake is bounded/fresh and that
  fixture seams are rejected in approved mode; live receipts remain open.
- [x] Local custody tests record execution context, release/script hashes,
  lower-tier capacity, exact command, pair identity, and watchdog result; no
  live claim is inferred from the fixture.
- [ ] The ordered live matrix is still pending: 1 GiB must complete before
  2 GiB, and 2 GiB before 4 GiB, with n≥3 per pair.
- [x] `IMPL.md` is `partial` with current local numbers; no new root validation
  record is created without live evidence.

## Out of SPEC

- Any implementation of ublk, kernel module unload, custom-kernel boot, or
  native VRAM memory tier.
- Any product-triggered host reboot, WSL shutdown, or WSL termination. The
  separately approved Windows outer watchdog is a harness containment boundary
  only; a fired watchdog is always `RED/unverified_terminated` and never a
  product result.
- Remote approval, external issue update, or package release.
- Any claim that capability, unit installation, or a generated report alone
  equals product readiness.
