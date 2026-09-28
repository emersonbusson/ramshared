# SPEC — Adapter-Bound VRAM Cache Safety and Fallback Contract

## 1. Closed scope

### In now

- `crates/ramshared-vram/src/lib.rs`: shared budget validity, adapter identity,
  current-use reserve, and runtime-headroom arithmetic.
- `crates/ramshared-wsl2d/src/gpu_budget.rs`: CUDA/Vulkan candidate policy,
  exact-LUID WDDM intersection, freshness checks, and exact adapter selection.
- `crates/ramshared-block/src/gpu_cache_worker.rs`: sparse allocation with a
  fresh budget check before each allocation.
- `crates/ramshared-block/src/isolated_origin.rs` and
  `crates/ramshared-block/src/ipc_cache_client.rs`: cache timeout/failure falls
  back to the authoritative origin.
- `crates/ramshared-wsl2d/src/main.rs`: refuse GPU-backed direct `--slices`
  actions before provider or device initialization.
- Rust status and dashboard paths publish only fresh, internally consistent,
  worker-bound GPU telemetry.

### Out now

- A hard timeout that interrupts a driver call already blocked in kernel
  context. Process isolation protects the origin data path; it cannot prove
  that a stuck child exits or that a physical GPU driver recovers.
- GPU-backed direct `--slices` operation. Its synchronous provider calls are
  refused by the planner. RAM-only broker operation remains available.
- `ublk` GPU lifecycle, Windows StorPort, and physical pressure qualification.
- Universal NVIDIA/AMD/Intel support or a freeze-free performance claim.

### Assumed-ready dependencies

- An opened, authoritative origin that passes its manifest and device identity
  checks.
- A CUDA or Vulkan provider that reports a fresh budget and usable adapter
  identity. WDDM only constrains a provider with an exact matching LUID.
- A local worker IPC channel with the configured read/write timeouts.

## 2. Traceability

| PRD requirement | Implementation / decision | Test / evidence |
| --- | --- | --- |
| RF-1 budget-bound admission | ITEM-1, DT-1 | `budget_target_preserves_reserve_after_existing_use`; `worker_budget_target_never_exceeds_current_available_headroom`; `mismatched_stale_future_and_malformed_budgets_are_rejected` |
| RF-2 origin correctness | ITEM-2, DT-2 | `cache_timeout_falls_back_to_origin`; `daemon_survives_abrupt_gpu_worker_kill` |
| RF-3 adapter identity | ITEM-1, DT-1 | `adapter_identity_matches_cross_api_only_through_shared_luid`; worker selection and reopen tests |
| RF-4 direct broker refusal | ITEM-3, DT-3 | `daemon_gpu_legacy_broker_refuses_before_backend_initialization` |
| RF-5 telemetry validity | ITEM-4 | active-worker budget telemetry and dashboard tests in `ramshared-wsl2d` / `ramshared-cli` |
| NFR-2 bounded product data path | ITEM-2, DT-2 | timeout fallback unit test; physical stuck-driver behavior remains environment-bound |

## 3. Technical decisions

| ID | Decision | Why |
| --- | --- | --- |
| DT-1 | A sample authorizes admission only when the source is driver-reported, the adapter identity exists, values are internally consistent, and the monotonic sample is not future-dated or older than 5 seconds. Cross-API matching uses only the same Windows LUID. | Local estimates, stale data, and unrelated adapters cannot safely describe current external GPU use. |
| DT-2 | GPU memory is a best-effort cache behind the authoritative origin. Cache reads have a bounded IPC wait; cache transport, worker, or protocol failure revokes the cache and uses the origin. | A GPU cache result must never be required to preserve acknowledged block data. |
| DT-3 | Reject GPU-backed `--slices` during action selection. Keep only its RAM backend until the synchronous GPU operations are either removed or moved behind a separately qualified origin-backed worker. | A post-hoc elapsed-time check cannot interrupt a driver call that has not returned. |
| DT-4 | For the origin cache, `safe_target = min(requested, capacity - reserve, live_available - reserve - 640 MiB)`, where `capacity = min(total, budget)` and `reserve = max(configured reserve, 20% of capacity)`. Subtractions saturate; each allocation rechecks the budget including the requested chunk. | Existing external use must not consume the reserve or runtime buffer. |
| DT-5 | When no trusted GPU candidate survives ranking and exact reopen/revalidation, start in origin-only mode. | A false GPU capacity is less safe than a disabled cache. |

## 4. Atomicity and rollback

### Atomicity frontier

- Budget selection and `--slices` refusal occur before GPU provider initialization,
  socket binding, NBD attach, or swap mutation.
- Cache admission is best effort. The origin operation remains authoritative;
  cache state cannot turn an origin error into success.
- Cache teardown revokes the client before the worker supervisor attempts
  termination. A child stuck in kernel driver code may survive the bounded
  user-space escalation; the daemon must report unavailable/stuck rather than
  claim complete release.

### Rollback

- **Userspace/daemon:** revert the planner, worker, and budget changes together;
  keep the origin-only path as the safe operating mode.
- **Kernel/module:** N/A — no kernel module or kernel code is changed here.
- **Host/persistent:** the refusal occurs before broker/device effects. Existing
  origin contents are not changed by a cache operation.
- **Forward-only:** N/A for this source change. A live host activation is not
  part of this patch.

## 5. Kahneman map

| Item | # | Question | Minimum evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-1 budget admission | #13 | Does a valid identified sample admit safe bytes, and do future/stale/estimated/mismatched samples refuse? | Named budget admission tests plus 80% slice coverage | Any invalid sample produces a nonzero target |
| ITEM-2 origin fallback | #16 | When a worker stalls or exits, does the request use origin without waiting beyond IPC policy? | `cache_timeout_falls_back_to_origin` and `daemon_survives_abrupt_gpu_worker_kill` | Origin read/write result differs or caller remains blocked after timeout |
| ITEM-3 direct broker refusal | #13 | Do all GPU backend values refuse before provider initialization while RAM remains selectable? | `daemon_gpu_legacy_broker_refuses_before_backend_initialization` plus `daemon_plan_routes_validated_actions_without_starting_a_backend` | GPU provider is initialized or a socket/device effect precedes refusal |

## 6. Security checklist

- [x] Privilege: no new privilege boundary; existing origin/NBD daemon permissions remain validated by their owning flow.
- [x] User/host copy: worker frame and cache chunk lengths are bounded before allocation/copy.
- [x] Flags/IOCTL codes: GPU API ioctl validation belongs to the CUDA/Vulkan/DXG provider crates; this change does not add ioctl values.
- [x] Info-leak: telemetry contains adapter identifiers and budgets, not kernel addresses.
- [x] IRQ/atomic: N/A — all code in this SPEC runs in userspace.
- [x] Lifetime: worker client revocation and cache allocation release are tested; complete release after an uninterruptible driver call is not claimed.
- [x] Hot-unplug/device-gone: provider errors fail admission or revoke cache; origin remains usable.
- [x] Host safety: direct synchronous GPU `--slices` is refused; live pressure testing requires a separate safe admission and supervised harness.
- [x] Shared-hardware cushion: reserve is subtracted from both capacity and current live headroom, and each chunk rechecks admission.
- [ ] Bounded DMA/foreign call: the parent origin data path has bounded IPC waits, but the kernel cannot guarantee interruption of a driver call inside a stuck child. Physical stuck-driver behavior remains a qualification gate.
- [ ] Cooperative cascade spillover: only cache-to-origin fallback is source-tested; simultaneous physical ZRAM/cache/SSD pressure and teardown evidence remain open.
- [x] Replayable operations: cache revocation is idempotent and repeated cache failure remains origin-safe.

## 7. Files to modify

| File | Change | Named verification |
| --- | --- | --- |
| `crates/ramshared-vram/src/lib.rs` | Central budget and identity contract | `budget_target_preserves_reserve_after_existing_use`; `adapter_identity_matches_cross_api_only_through_shared_luid` |
| `crates/ramshared-cuda/src/vram_impl.rs` | CUDA adapter implements the shared budget and memory traits | `test_vram_error_conversion_out_of_range`; `test_vram_error_conversion_provider`; `mock_driver_exercises_memory_and_mapping_raii` |
| `crates/ramshared-wsl2d/src/gpu_budget.rs` | Candidate selection, reserve sizing, WDDM composition | `direct_broker_slice_preserves_live_reserve_canary_and_alignment`; `mismatched_stale_future_and_malformed_budgets_are_rejected` |
| `crates/ramshared-block/src/gpu_cache_worker.rs` | Live per-allocation admission | `worker_budget_target_never_exceeds_current_available_headroom`; `worker_respects_headroom_floor` |
| `crates/ramshared-block/src/isolated_origin.rs` | Origin fallback contract | `cache_timeout_falls_back_to_origin` |
| `crates/ramshared-wsl2d/src/main.rs` | Refuse synchronous GPU direct broker | `daemon_gpu_legacy_broker_refuses_before_backend_initialization`; `daemon_survives_abrupt_gpu_worker_kill` |

## 8. Coverage and validation matrix

| Production path | Test | Kind | Kahneman | Coverage |
| --- | --- | --- | --- | --- |
| `ramshared-vram/src/lib.rs` | named budget and identity tests above | unit | #13 | >=80% changed logic |
| `ramshared-wsl2d/src/gpu_budget.rs` | freshness, WDDM, slice, and adapter selection tests | unit | #13 | Slice coverage is owned by the isolated GPU cache-worker SPEC. |
| `ramshared-block/src/gpu_cache_worker.rs` | target, allocation, and revoke tests | unit | #16/#17 | >=80% changed logic |
| `ramshared-block/src/isolated_origin.rs` | `cache_timeout_falls_back_to_origin` | unit | #16 | >=80% changed logic |
| `ramshared-wsl2d/src/main.rs` | direct broker refusal and worker-loss tests | unit/integration | #13 | >=80% changed logic |
| exact installed product path | worker allocation → origin fallback → teardown | live | #16 | OPEN — requires host/hardware qualification |

Required source commands:

```sh
cargo fmt --all -- --check
cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d
cargo clippy -p ramshared-vram -p ramshared-block -p ramshared-cuda -p ramshared-dxg -p ramshared-vulkan -p ramshared-wsl2d --all-targets -- -D warnings
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/vram_impl.rs --min 80
```

Live `before → action → after` evidence for GPU allocation, cache revocation,
origin fallback, and host teardown remains OPEN for physical NVIDIA, AMD, and
Intel adapters. Software Vulkan tests are not a substitute.

## 9. Observability

The worker heartbeat and `status --json` may expose adapter key/LUID, backend,
budget, usage, available bytes, and sample age only when the snapshot passes
identity, freshness, and consistency checks. No watchdog trip event or automatic
Tier 3 spillover event is claimed by this SPEC.

## 10. Living documents

| Document | Required action |
| --- | --- |
| `ARCHITECTURE.md` | Keep reserve formulas surface-specific and describe GPU as a revocable cache. |
| `docs/reliability/GAP-REGISTER.md` | Keep physical vendor, worker teardown, and pressure qualification PARTIAL. |
| `docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md` | Keep process-boundary limitations and origin fallback evidence aligned. |
