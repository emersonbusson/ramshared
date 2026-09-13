# SPEC — WSL2 Hyper-V VMBus Memory Headroom and Anti-Starvation Governor

## 1. Closed Scope

- **In Now**:
  - Detection of Microsoft WSL2 execution environment in `crates/ramshared-cli/src/stress.rs`.
  - Fail-closed dynamic floor enforcement: `hard_floor = opts.min_ram_mb.max(600).max(dynamic_kernel_floor)` under WSL2.
  - Sizing of `safe_alloc_mb` strictly relative to `hard_floor` to eliminate race-to-zero allocations.
  - Unit tests asserting `multi_tier_floor >= 600` under WSL2.
  - Upstream RFC design document detailing kernel-level VMBus atomic headroom for `microsoft/WSL2-Linux-Kernel`.
- **Out Now**:
  - Modifying Windows-side closed-source `wslhost.exe` binary.
  - Rewriting Linux kernel core vmscan / page allocator.
- **Assumed-Ready Dependencies**:
  - `ramshared-cli` stress engine (`crates/ramshared-cli/src/stress.rs`).
  - Linux `/proc/meminfo`, `/proc/pressure/memory`, and `/proc/sys/kernel/osrelease`.

## 2. Traceability

| PRD Requirement | SPEC Implementation Item |
| :--- | :--- |
| `RF-1` (WSL2 Detection) | `ITEM-1` (`is_wsl2()` probe in `stress.rs`) |
| `RF-2` (Dynamic Floor Clamping) | `ITEM-2` (`multi_tier_floor` calculation in `run()`) |
| `RF-3` (Floor-Relative Sizing) | `ITEM-3` (`safe_alloc_mb` delta calculation) |
| `RF-4` (Upstream RFC & Contribution) | `ITEM-4` (Upstream patch and RFC spec in living docs) |

## 3. Technical Decisions

| # | Decision | Why |
| :--- | :--- | :--- |
| `DT-1` | **WSL2 probe via `/proc/sys/kernel/osrelease` and `WSLInterop`** | Standard, zero-cost heuristic already validated across `crates/ramshared-cli/src/cascade/mod.rs`. |
| `DT-2` | **Enforce 600 MB floor on WSL2** | Hyper-V synthetic buses (`vmicvmswitch`, `hv_balloon`, `hv_vmbus`) require at least 500 MB headroom during direct reclaim; 600 MB provides a fail-closed 100 MB safety cushion. |
| `DT-3` | **Make `safe_alloc_mb` strictly floor-relative** | Prevents chunk allocation when available RAM is close to floor (e.g. `avail_mb <= hard_floor + 20`), avoiding overshoot into the danger zone. |
| `DT-4` | **Native RFC Proposal for Microsoft WSL** | Recommends kernel patch reserving `GFP_ATOMIC` pools for Hyper-V channels in `drivers/hv/vmbus_drv.c` and introducing `guestHeadroomMb=600` in `.wslconfig`. |

## 4. Atomicity and Rollback

- **Atomicity Frontier**: 
  - Userspace CLI: The safety floor calculation executes atomically in memory at every 150–200 ms sampling interval before any allocation vector is resized.
  - Kernel / Host: Sysctl settings (`vm.min_free_kbytes`) remain intact; no persistent state changes to `/dev/nbd0` or swap extents.
- **Rollback**:
  - If the 600 MB floor is too restrictive on systems with small total RAM (e.g. 2 GB WSL2 instances), users can supply `--min-ram-mb <N>` or run in standard single-tier mode. Rollback trigger: inability to allocate at least 1 GB swap on a 16 GB host.

## 5. Kahneman Map

| ITEM / Stage | # | Question | Min Evidence | Abort |
| :--- | :--- | :--- | :--- | :--- |
| `ITEM-2` (Floor Calc) | `#13` | Does the governor fail-closed and refuse to allocate when `avail_mb <= 600` on WSL2? | `cargo test -p ramshared-cli --bin ramshared -- stress::tests::wsl2_hard_floor_enforces_safety_ceiling` | Any floor calculation returning <600 MB on WSL2 |
| `ITEM-3` (Alloc Clamp) | `#16` | Can rapid allocation loops overshoot the floor under high memory churn? | Live micro-stress run halting at `avail_mb >= hard_floor` | Available RAM dipping below `hard_floor - 50 MB` |

## 6. Security Checklist (Pre-Impl)

- [x] **Privilege**: N/A (Runs within current user permissions; sysctl verification is read-only).
- [x] **User/host copy**: N/A (Internal memory calculations).
- [x] **Flags/IOCTL codes**: Validated CLI arguments clamp within safe numeric ranges.
- [x] **Info-leak**: No kernel or physical pointers leaked in telemetry logs.
- [x] **IRQ/atomic**: Prevents kernel atomic allocation exhaustion (`GFP_ATOMIC`).
- [x] **Host safety**: Strictly honors `AGENTS.md` anti-skynet rule on avoiding unsupervised thrash pressure on WSL2.
- [x] **Shared-hardware cushion**: Enforces mathematical 600 MB host reservation floor.

## 7. Files to CREATE / MODIFY / DELETE

### MODIFY

**`crates/ramshared-cli/src/stress.rs`**
- **Purpose**: Implement WSL2-aware floor and relative allocation sizing.
- **RF / DT**: `RF-1`, `RF-2`, `RF-3` / `DT-1`, `DT-2`, `DT-3`.
- **Symbols**: `pub fn is_wsl2() -> bool`, `pub fn run(opts: &StressOptions) -> Result<(), String>`.
- **Before → After**: Hardcoded `MULTI_TIER_HARD_FLOOR_MB = 200` replaced by dynamic `is_wsl2()` clamped `multi_tier_floor = opts.min_ram_mb.max(600).max(dynamic_kernel_floor)`.
- **Required tests**: `stress::tests::wsl2_hard_floor_enforces_safety_ceiling`.
- **Cover target**: ≥80% business logic on modified slice.

## 8. Observability

| Signal | Where | Level / Type |
| :--- | :--- | :--- |
| `🛑 RAM FLOOR REACHED` | stdout / CLI log | Informational notice indicating safe halt |
| `multi_tier_floor` | Telemetry log | Numeric field in telemetry log payload |

## 9. Living Docs

| Document | Action |
| :--- | :--- |
| `docs/specs/no-milestone/wsl2-vmbus-resilience/PRD.md` | Created |
| `docs/specs/no-milestone/wsl2-vmbus-resilience/SPEC.md` | Created |
| `docs/specs/no-milestone/wsl2-vmbus-resilience/AUDIT-2.5.md` | Created (Step 2.5) |
| `docs/INDEX.md` | Updated via `node tools/generate-docs-index.mjs` |

## 10. Implementation Order

- **`ITEM-1`**: Add `is_wsl2()` helper in `crates/ramshared-cli/src/stress.rs`.
- **`ITEM-2`**: Update `hard_floor` calculation in `run()` to clamp to `max(600, dynamic_kernel_floor)` when `is_wsl2()` is true.
- **`ITEM-3`**: Adjust `safe_alloc_mb` to be relative to `hard_floor + 200` and `hard_floor + 20`.
- **`ITEM-4`**: Add unit test `wsl2_hard_floor_enforces_safety_ceiling` and run full regression suite.

## 11. Required Tests Matrix

| Production Path | Test Name | Kind | Kahneman | Cover |
| :--- | :--- | :--- | :--- | :--- |
| `crates/ramshared-cli/src/stress.rs` | `stress::tests::wsl2_hard_floor_enforces_safety_ceiling` | unit | #13 | ≥80% |
| `crates/ramshared-cli/src/stress.rs` | `stress::tests::executes_micro_stress_runs_safely` | unit | #16 | ≥80% |
| `crates/ramshared-cli/src/stress.rs` | `stress::tests::parses_stress_cli_arguments_with_battery` | unit | #9 | ≥80% |

## 12. Validation Checklist

- [x] `cargo fmt` and `cargo test -p ramshared-cli --bin ramshared -- stress` passes 11/11 tests.
- [x] Every matrix row has a real test name.
- [x] Kahneman critical rows have executable test evidence.
- [x] `./scripts/docs-check.sh` passes 100% green.
