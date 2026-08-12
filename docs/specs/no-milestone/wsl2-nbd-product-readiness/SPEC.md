# SPEC — WSL2 NBD-only product readiness

> SSDV3 Step 3 is **source-partial**. It implements bounded pure policy,
> manufactured release/preflight checks, and a local injected lifecycle
> contract. It does not contain a live WSL2 before → action → after run. The
> only honest product verdict remains `PARTIAL`; the environment-bound rows in
> this SPEC are not inferred from local tests.

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
- Manufactured installer rollback injection after every named post-write phase.

### Out now

- CI/workflow changes, release publication, host mutation, WSL shutdown/reboot,
  swap pressure, GPU pressure, or external communication.
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
| RF-NBD-6, NFR-NBD-4 | ITEM-5 — size promotion and benchmark matrix |
| RF-NBD-7, RF-NBD-8 | ITEM-6 — approval, no reboot, and Relay gates |
| RF-NBD-13 | ITEM-8 — exact legacy unit migration |
| RF-NBD-11, NFR-NBD-1..6 | ITEM-7 — safety, observability, and evidence |

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
| DT-NBD-9 | Reboot, WSL shutdown, and termination are forbidden in this product gate. | The NBD product must be safe on the running WSL surface. |
| DT-NBD-10 | Relay `--check` is mandatory and `--reap` is never automatic. | Upstream lifecycle health is a prerequisite, not a repair side effect. |
| DT-NBD-11 | A live daemon must resolve to the selected sealed release. | Presence of a binary is not deployment identity. |
| DT-NBD-12 | A failure is a stable refusal, not a retry loop. | Deterministic identity/capacity/lifecycle failures are not transient. |
| DT-NBD-13 | `crates/ramshared-cli/src/cascade/cascade_io.rs` owns the NBD teardown effects; `cascade/lifecycle.rs` only derives a read-only status view. | The executor must prove `swapoff` before NBD disconnect or daemon stop through an injected local seam; status derivation cannot own lifecycle effects. |
| DT-NBD-14 | A conflicting `ramshared-cascade.service` is never overwritten by the normal installer. Replacement is allowed only with both the named release approval and a second current SHA-256 approval for one inactive, disabled, root-owned regular legacy file; the installer copies it to a sealed immutable backup before atomic replacement. | The exact observed legacy artifact, rather than a path or name alone, is the authority boundary. A wrong hash, a symlink, unknown metadata, activity, enablement, backup mismatch, or any failed replacement refuses or restores the prior unit. |

## Atomicity/rollback

| Layer | Atomic operation | Rollback frontier |
| --- | --- | --- |
| Release filesystem | Copy to a new version directory, hash/verify, seal, then switch selector. | Before selector switch, delete only the unselected staging directory; after switch, point back to the prior sealed version. |
| Product state | `PRODUCT_OFF → READY` only after all preconditions; no partial ready. | Any failed gate leaves `BLOCKED` or the prior `PRODUCT_OFF` state. |
| NBD lifecycle | Attach, verify identity, then publish swap; teardown does swapoff before disconnect. | If swapoff fails, keep daemon/NBD alive; do not disconnect or kill. |
| ublk retirement | Stop/disable only an identified legacy service under approval. | If ownership or active device is ambiguous, make no service mutation; leave `BLOCKED`. Never unload module. |
| Legacy cascade unit migration | Hash-bind one inactive legacy unit, seal its backup, then atomically replace the unit. | Missing/stale/mismatched approval, non-regular metadata, backup mismatch, or a failed replacement leaves or restores the prior unit and removes the new release selection. |
| Host/WSL | No reboot, shutdown, termination, or host configuration change. | A request for any such action is a refusal before mutation. |
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
- [x] No reboot, WSL shutdown, process kill, module unload, or host pressure is
  hidden behind a readiness check.
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
| `scripts/safety/nbd-product-preflight.sh` | Implemented | Read-only Relay, release, capacity, service, and host gate. |
| `scripts/safety/test-nbd-product-preflight.sh` | Implemented | Static/manufactured refusal, idempotency, and every-post-write rollback test. |
| `scripts/safety/install-cascade-boot.sh` | Implemented | Sealed installer rollback markers and bounded cleanup frontier. |
| `scripts/safety/cascade-up.sh` / `cascade-down.sh` | Implemented shared lifecycle surface | NBD-only startup and safe teardown. |
| `scripts/package/build-linux-bundle.sh` | Implemented shared packaging surface | Stage only sealed version artifacts; no in-place install. |
| `docs/specs/no-milestone/wsl2-nbd-product-readiness/IMPL.md` | Create as `partial` | Exact local numbers and environment-bound gaps; no live claim. |
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
| Benchmark | size, condition, run, p50, p99, deviation, integrity | All cells have n≥3 and no corruption/ghost. |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Update only if the NBD-only product boundary changes in Step 3. |
| `docs/reliability/GAP-REGISTER.md` | Existing ublk deferred gap remains authoritative; update only with evidence. |
| `docs/BENCHMARKS.md` and results | Update only after the named matrix is run. |
| `validation.md` | Append only after live approved evidence. |
| `IMPL.md` | Create only after implementation gates. |
| `AGENTS.md`, `CLAUDE.md`, `.claude/rules/*` | N/A — no convention change. |
| `docs/INDEX.md` | Regenerate as a generated index for this pack. |

## Implementation order

No gaps are permitted between items:

1. **ITEM-1:** freeze NBD-only transport and enumerate legacy ublk service/device ownership.
2. **ITEM-2:** implement sealed-release selector and manifest/BINARY_MATCH checks.
3. **ITEM-3:** implement pure `PRODUCT_OFF`/`READY`/`BLOCKED` status model.
4. **ITEM-4:** implement capacity arithmetic, lower-tier selection, swapoff-first lifecycle, and refusal codes.
5. **ITEM-5:** implement 1/2/4 GiB promotion and the benchmark/evidence schema.
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
| `scripts/safety/nbd-product-preflight.sh` | `release_manifest_and_modes_are_verified` | static | #3 | Unsealed/mutated release refuses. |
| `scripts/safety/nbd-product-preflight.sh` | `binary_match_rejects_stale_or_deleted_daemon` | static/refusal | #3 | Live executable equals sealed manifest. |
| `scripts/safety/nbd-product-preflight.sh` | `relay_check_failure_blocks_readiness` | manufactured refusal | #13/#18 | Relay candidate/ambiguity blocks; no reap. |
| `scripts/safety/nbd-product-preflight.sh` | `reboot_and_shutdown_requests_are_refused` | static/refusal | #13 | No host restart command is executed. |
| `scripts/safety/nbd-product-preflight.sh` | `legacy_ublk_retirement_never_unloads_module` | static/refusal | #18 | No `rmmod`/`modprobe -r`; service ownership is bounded. |
| `scripts/safety/test-nbd-product-preflight.sh` | `product_off_ready_blocked_state_matrix` | manufactured | #13 | All state distinctions and reason codes match. |
| `scripts/safety/test-nbd-product-preflight.sh` | `capacity_sink_identity_and_alignment_refusals` | manufactured refusal | #13/#16 | Wrong/ambiguous sink and alignment refuse. |
| `scripts/safety/test-nbd-product-preflight.sh` | `n3_or_ublk_capability_does_not_promote_nbd_product` | manufactured refusal | #2 | Capability is not product readiness. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `installer_every_post_write_phase_rolls_back` | manufactured rollback | #13/#17 | Inject after each of the 20 named post-write markers; preserve prior selector/unit state and remove the new destination. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_unit_migration_requires_exact_hash_and_restores_on_failure` | manufactured approval/rollback | #3/#13/#17 | Default conflict refusal; only an inactive root-owned regular unit matching the supplied SHA-256 is backed up and atomically replaced; stale/mismatched metadata or injected failure retains/restores it. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `corrupt_published_legacy_backup_refuses_before_replacement` | manufactured refusal | #3/#13 | A post-publish backup hash mismatch refuses before replacement staging and leaves the legacy unit unchanged. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_backup_root_symlink_is_refused` | manufactured refusal | #3/#13 | A pre-existing backup-root symlink refuses before any backup write. |
| `scripts/safety/install-cascade-boot.sh` + `scripts/safety/test-nbd-product-preflight.sh` | `legacy_restore_reloads_systemd_after_daemon_reload` | manufactured rollback | #13/#16 | A failure after daemon reload restores the old unit and reloads systemd again. |
| `scripts/safety/cascade-up.sh` + `cascade-down.sh` | `nbd_lifecycle_before_action_after` | live E2E | #16/#17 | Approved NBD run, swapoff-first teardown, no ghost. |
| `scripts/safety/nbd-product-preflight.sh` | `relay_gate_before_action_after` | live E2E | #18 | Relay check is clean before and after; no automatic reap. |
| release/cascade surface | `NBD_BENCHMARK_MATRIX` | live benchmark | #9 | 1/2/4 GiB cells, n≥3, median/p99/deviation/integrity. |

### NBD environment-bound matrix

| TestName / evidence | Local source result | Live status | Why it remains partial |
| --- | --- | --- | --- |
| `nbd_lifecycle_before_action_after` | Injected ordering/refusal contract only | `PARTIAL` / not run | No approved WSL2 NBD before → action → after run or `/proc/swaps` capture exists. |
| `relay_gate_before_action_after` | Read-only fixture refusal coverage only | `PARTIAL` / not run | No approved Relay environment before/action/after evidence exists. |
| `NBD_BENCHMARK_MATRIX` | Matrix schema only | `PARTIAL` / not run | No 1/2/4 GiB cells, n≥3 statistics, integrity, or terminal-state evidence exists. |
| `BINARY_MATCH` | Static stale/deleted daemon refusal only | `N/A` / not run | No live selected release and daemon process are in scope for this turn. |

### Benchmark matrix

Each cell has at least three independent runs on the declared WSL2 surface.
Record block size, queue depth, workload direction, `V`, lower-tier `L`,
VRAM free, NBD transport, p50/median, p99, standard deviation or equivalent
deviation, checksum result, ghost-swap result, and terminal state.

| Cell | Tier size | Condition | Required comparison |
| --- | ---: | --- | --- |
| P1-NBD-IDLE | 1 GiB | NBD + zram + lower sink, idle host | Disk-only control at same protocol/load. |
| P1-NBD-BOUNDED | 1 GiB | Same path with an explicitly approved bounded generic GPU condition | No host instability; demotion/teardown remains green. |
| Q2-NBD-IDLE | 2 GiB | Same matrix after P1 promotion | P1 result and disk-only control. |
| Q2-NBD-BOUNDED | 2 GiB | Same bounded condition | Capacity and integrity remain green. |
| Q4-NBD-IDLE | 4 GiB | Same matrix after Q2 promotion | Q2 result and disk-only control. |
| Q4-NBD-BOUNDED | 4 GiB | Same bounded condition | No promotion on partial or environment-bound result. |

The matrix is not a license to pressure the shared daily host. If the declared
surface cannot provide the bounded condition, mark that cell `PARTIAL` and do
not infer a product claim from idle rows.

## Validation checklist

Local Step 3 checks are source/static/manufactured only. A passing local check
does not complete an environment-bound row:

- [ ] `cargo fmt --all -- --check`, `cargo clippy -D warnings`, and the named
  pure-Rust package tests.
- [ ] `node tools/ci/check-rust-slice-coverage.mjs` for every new business-logic
  file, with minimum 80% per file; no coverage claim for shell orchestration.
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
  matrix.
- [ ] Manufactured release install proves owner/mode/hash immutability and all
  20 named post-write selector/unit/destination rollback frontiers.
- [ ] Readiness proves NBD-only, no active ublk service/device, and no module
  unload.
- [ ] `BINARY_MATCH` proves the live daemon resolves to the sealed version
  (`N/A`/not run in this local slice).
- [ ] Relay `--check` passes before and after; no automatic reap is used
  (`PARTIAL`/not run in this local slice).
- [ ] No reboot, WSL shutdown/termination, host pressure, or external write.
- [ ] Live E2E is before → action → after on WSL2 itself, with checksum,
  `/proc/swaps`, daemon, capacity, and terminal-state evidence.
- [ ] 1 GiB pilot is complete before 2 GiB; 2 GiB complete before 4 GiB.
- [ ] `IMPL.md` remains `partial` until environment-bound cells are complete;
  no root validation record is created without live evidence.

## Out of SPEC

- Any implementation of ublk, kernel module unload, custom-kernel boot, or
  native VRAM memory tier.
- Any host reboot, WSL shutdown, remote approval, external issue update, or
  package release.
- Any claim that capability, unit installation, or a generated report alone
  equals product readiness.
