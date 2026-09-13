# IMPL — WSL2 Hyper-V VMBus Memory Headroom and Anti-Starvation Governor

## 1. Summary of Changes

Implemented a WSL2-aware dynamic safety floor within the RamShared native stress governor (`crates/ramshared-cli/src/stress.rs`). This prevents extreme memory allocation loops from depressing guest available memory below 600 MB on WSL2, protecting Hyper-V synthetic device ring buffers (`hv_vmbus`, `vmicvmswitch`, `hv_balloon`) from `GFP_ATOMIC` starvation and eliminating host-side watchdog VM terminations (`Hyper-V-VmSwitch Event 102/291`).

## 2. Traceability & Commits

| Commit | What was done | Why it was done | Details |
| :--- | :--- | :--- | :--- |
| `3f4a5f2` | `fix(stress): enforce WSL2 dynamic safety floor in multi-tier cascade` | Prevent Hyper-V watchdog VM resets under cascade stress on WSL2 | Added `is_wsl2()`, clamped `hard_floor` to ≥600 MB, adjusted `safe_alloc_mb` to floor-relative deltas, added unit test |

## 3. Evidence

- **Unit Tests**:
  `cargo test -p ramshared-cli --bin ramshared -- stress`
  ```text
  running 11 tests
  test stress::tests::computes_latency_percentiles_accurately ... ok
  test stress::tests::appends_telemetry_log_file ... ok
  test stress::tests::benchmark_archiving_and_formatting_tests ... ok
  test stress::tests::computes_telemetry_reading_accurately ... ok
  test stress::tests::parse_all_stress_flags_and_help ... ok
  test stress::tests::helper_probes_execute_safely ... ok
  test stress::tests::parses_stress_cli_argument_errors ... ok
  test stress::tests::parses_stress_cli_arguments_with_battery ... ok
  test stress::tests::rejects_thread_count_exceeding_physical_limits ... ok
  test stress::tests::wsl2_hard_floor_enforces_safety_ceiling ... ok
  test stress::tests::executes_micro_stress_runs_safely ... ok

  test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 254 filtered out; finished in 4.00s
  ```

- **Docs Check**:
  `./scripts/docs-check.sh`
  `✓ docs-check OK`
