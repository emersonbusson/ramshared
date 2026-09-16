# SPEC — WSL2 VMBus Anti-Fragmentation Governor and Dedicated Ring Pool Resilience

> SSDV3 Step 2 · PRD: `docs/specs/no-milestone/wsl2-vmbus-anti-fragmentation-governor/PRD.md`

## 1. Closed Scope

- **In Now**:
  - `parse_buddyinfo_order_7_chunks` and `read_buddyinfo_order_7` in `crates/ramshared-cli/src/stress.rs`.
  - Governor buddy interlock checking order-7 free chunks in `zone Normal` on WSL2.
  - Update `WSL2_MIN_PHYSICAL_HEADROOM_MB` from 600 MB to 1024 MB.
  - Standalone kernel patch `docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch` for `microsoft/WSL2-Linux-Kernel`.
  - Unit tests covering buddyinfo parsing, refusal on depleted chunks, and elevated headroom.
- **Out Now**:
  - Recompiling or replacing the active Windows kernel driver on the host during this step.
  - Modifying closed-source Windows hypervisor binaries.
- **Assumed-Ready Dependencies**:
  - `/proc/buddyinfo` kernel interface on Linux.
  - Existing `is_wsl2()` detection in `ramshared-cli`.

## 2. Traceability

| PRD | SPEC |
| --- | --- |
| `RF-1` | `ITEM-1`, `ITEM-2` |
| `RF-2` | `ITEM-2`, `ITEM-3` |
| `RF-3` | `ITEM-2` |
| `RF-4` | `ITEM-2` |
| `RF-5` | `ITEM-4` |
| `NFR-1..4` | `ITEM-1`, `ITEM-2`, `ITEM-3`, `ITEM-4` |

## 3. Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| `DT-1` | **Monitor Zone `Normal` order 7 (512 KiB) specifically** | Hyper-V `vmbus_alloc_ring` explicitly requests `order 7` (`alloc_pages(..., 7)`). `DMA32` zone is ignored for synthetic device rings. |
| `DT-2` | **Interlock trip point set to $< 8$ chunks** | 8 chunks equals 4 MiB of contiguous reserve. This leaves sufficient margin for incoming VMBus sockets (`hvs_probe`) without prematurely halting normal test ramps. |
| `DT-3` | **Raise `WSL2_MIN_PHYSICAL_HEADROOM_MB` to 1024 MB** | Previous 600 MB headroom only gave 88 MB margin above `min_free_kbytes` (512 MB). 1024 MB gives 512 MB of extra working headroom for kswapd memory compaction. |
| `DT-4` | **Kernel patch: virtual fallback (`vzalloc`) in `vmbus_alloc_ring`** | Virtual memory allocation does not require contiguous physical pages; if `alloc_pages(..., 7)` fails, `vzalloc` succeeds even under 100% physical fragmentation. |

## 4. Atomicity and Rollback

- **Atomicity Frontier**:
  - Userspace: The buddyinfo probe is evaluated per governor tick before memory allocation is committed.
  - Kernel patch: Formatted as an independent unified diff ready for `git am`.
- **Rollback**:
  - Revert git commit if buddyinfo parsing misidentifies zone layout or if false interlock triggers occur.

## 5. Kahneman Map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| Buddy parsing | #13 | Does parser reject empty/corrupted buddyinfo lines safely? | `cargo test -p ramshared-cli parse_buddyinfo` | Any panic on malformed lines. |
| Interlock trigger | #16 | Does governor halt immediately when order-7 is exhausted? | `cargo test -p ramshared-cli order_7_interlock` | Continuation of allocation when chunks $< 8$. |
| Headroom | #17 | Is total physical headroom guaranteed $\ge 1024\text{ MB}$? | `cargo test -p ramshared-cli wsl2_hard_floor` | Headroom $< 1024\text{ MB}$. |

## 6. Security Checklist (Pre-Impl)

- [x] Privilege: Parsing `/proc/buddyinfo` is read-only and unprivileged; compaction trigger is best-effort.
- [x] User/host copy: N/A.
- [x] Flags/IOCTL codes: N/A.
- [x] Info-leak: N/A.
- [x] Host safety: Prevents guest fragmentation from causing Windows HCS hypervisor deadlocks.
- [x] Shared-hardware cushion: Enforces 1024 MB physical reserve on WSL2.
- [x] Replayable ops: Idempotent parsing on every sample tick.

## 7. Files to CREATE / MODIFY / DELETE

### CREATE

**`docs/upstream/patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`**
- Purpose: Upstream patch for `microsoft/WSL2-Linux-Kernel` providing `vzalloc` fallback on ring allocation failure.
- RF / DT: RF-5, DT-4.

### MODIFY

**`crates/ramshared-cli/src/stress.rs`**
- Purpose: Add `/proc/buddyinfo` parser, update `WSL2_MIN_PHYSICAL_HEADROOM_MB`, and wire buddy interlock into `run_stress`.
- RF / DT: RF-1, RF-2, RF-3, RF-4, DT-1, DT-2, DT-3.
- Before -> After:
  `WSL2_MIN_PHYSICAL_HEADROOM_MB = 600` -> `WSL2_MIN_PHYSICAL_HEADROOM_MB = 1024`
  Add `parse_buddyinfo_order_7_chunks(content)`
  In `run_stress`: check order 7 chunks on WSL2; halt if $< 8$.
- Tests: `test_buddyinfo_order_7_parsing`, `test_buddyinfo_order_7_interlock`, `test_wsl2_headroom_floor`.
- Cover target: $\ge 80\%$.

## 8. Implementation Order

- `ITEM-1`: Implement buddyinfo parsing functions and unit tests in `stress.rs` (RED -> GREEN).
- `ITEM-2`: Wire order-7 interlock and raise `WSL2_MIN_PHYSICAL_HEADROOM_MB` to 1024 MB in `stress.rs`.
- `ITEM-3`: Formulate upstream kernel patch `0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch`.
- `ITEM-4`: Run full test suites, slice coverage, and docs-check.

## 9. Required Tests Matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-cli/src/stress.rs` | `tests::test_buddyinfo_order_7_parsing` | unit | #13 | $\ge 80\%$ |
| `crates/ramshared-cli/src/stress.rs` | `tests::test_buddyinfo_order_7_interlock_threshold` | unit | #16 | $\ge 80\%$ |
| `crates/ramshared-cli/src/stress.rs` | `tests::test_wsl2_headroom_floor_enforces_1024_mb` | unit | #17 | $\ge 80\%$ |

## 10. Validation Checklist

- [x] `cargo fmt` / `clippy -D warnings` / `cargo test -p ramshared-cli`
- [x] Cover gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/stress.rs --min 80`
- [x] `./scripts/docs-check.sh`
- [x] Every matrix row has a real test name
- [x] Kahneman critical rows have executable evidence
