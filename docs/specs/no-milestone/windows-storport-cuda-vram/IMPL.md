# IMPL — Windows StorPort I/O backed by CUDA VRAM

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md

## Status

**partial** · cover ✓ · E2E partial (CUDA probe live) · BINARY_MATCH N/A (Windows-only; no `ramsharedd`)

Linux unit/cover gates green. **ITEM-2 live CUDA probe** proven on host GPU (WSL libcuda /
RTX 2060): allocate 512 MiB, three-offset patterns, free restore within 64 MiB.
Full Windows StorPort product path (WDK/SCM/Verifier/physical nvcuda product bind) remains
**environment-bound** and does **not** close DONE.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-winsvc/src/config.rs` | ITEM-1 / DT-2 | Closed product `WinDriveConfig` + deny_unknown_fields + reserve floor |
| `crates/ramshared-winsvc/src/evidence.rs` | ITEM-1 / DT-10 | Schema-1 JSONL evidence writer + percentiles + redaction |
| `crates/ramshared-winsvc/src/runtime.rs` | ITEM-4 / DT-7–12 | Phase machine, reverse unwind, idempotent stop, CLI parser |
| `crates/ramshared-winsvc/src/driver_link.rs` | ITEM-3 / DT-4 | `QueueAccess` + `InMemoryQueue` + owned SQE/WRITE snapshots |
| `crates/ramshared-winsvc/src/broker_tenant.rs` | RF-3 / DT-8–9 | Granted-bytes equality; release flush; release 2× once |
| `crates/ramshared-winsvc/src/service.rs` | ITEM-5 / DT-8 | Gate A → lock → Gate B → flush/dismount → destroy |
| `crates/ramshared-winsvc/src/windows_driver.rs` | ITEM-3 | VirtualAlloc mapped queue + OVERLAPPED IOCTL `WindowsDriverLink` |
| `crates/ramshared-winsvc/src/windows_host.rs` | ITEM-5 | Elevation, reparse-safe config open, pagefile CIM, volume lock, CNG SHA-256 |
| `crates/ramshared-winsvc/src/cuda_probe.rs` | ITEM-2 / DT-3 | Shared probe-cuda path (libcuda/nvcuda) |
| `crates/ramshared-winsvc/src/main.rs` | ITEM-5 / DT-1 | Product CLI; probe-cuda wired; lab PS1 removed |
| `crates/ramshared-winsvc/winsvc.example.toml` | ITEM-1 | Storage-only example config |
| `crates/ramshared-cuda/src/probe.rs` | ITEM-2 / DT-3 | Three-offset probe planning + patterns |
| `crates/ramshared-cuda/src/driver.rs` | ITEM-2 | `Device::ordinal()` retained |
| `drivers/windows/ramshared/queue.{h,c}` | ITEM-3 / DT-5–6 | Owner, rundown, slot states, reserved/ring validation |
| `drivers/windows/ramshared/control.c` | ITEM-3 | Owner IOCTL checks; zero-input lengths |
| `drivers/windows/ramshared/virtdisk.{h,c}` | ITEM-3 / RF-4 | OwnerProcess; reserved disk params; VPD 0x00/0x80 |
| `scripts/windows/Install-RamSharedService.ps1` | ITEM-5 | Rust product installer |
| `scripts/windows/Install-RamSharedLabService.ps1` | RF-6 | Lab C# installer (`-LabVm`, `LAB_ONLY=1`) |
| `scripts/windows/Format-RamSharedLun.ps1` | ITEM-6 / DT-11 | Exact identity; no size-only fallback |
| `scripts/windows/Measure-RamSharedDiskIo.ps1` | ITEM-6 | SHA-256 rounds + p50/p95/p99 |
| `scripts/windows/Invoke-WinDriveIoctlValidation.ps1` | ITEM-3 | Named verdict scaffold (lab) |
| `scripts/windows/Invoke-CudaStorageDrill.ps1` | ITEM-6 | Storage-only drill scaffold |

## Validation (numbers)

### Unit / fmt / clippy

| Cmd | Exit |
| --- | --- |
| `cargo fmt -p ramshared-winsvc -p ramshared-cuda -- --check` | 0 |
| `cargo clippy -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets -- -D warnings` | 0 |
| `cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets` | 0 (winsvc lib: 72 tests) |

### Cover gate

```text
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winsvc \
  --files crates/ramshared-winsvc/src/config.rs,crates/ramshared-winsvc/src/evidence.rs,\
crates/ramshared-winsvc/src/driver_link.rs,crates/ramshared-winsvc/src/broker_tenant.rs,\
crates/ramshared-winsvc/src/runtime.rs,crates/ramshared-winsvc/src/service.rs --min 80 \
  --report-json tmp/windows-storport-cuda-vram-cov.json
```

| Path | Lines % |
| --- | --- |
| `broker_tenant.rs` | 85.9% |
| `config.rs` | 95.5% |
| `driver_link.rs` | 86.9% |
| `evidence.rs` | 94.4% |
| `runtime.rs` | 86.8% |
| `service.rs` | 84.1% |

CUDA probe planning:

| Path | Lines % |
| --- | --- |
| `crates/ramshared-cuda/src/probe.rs` | 80.0% |

Report JSON: `tmp/windows-storport-cuda-vram-cov.json`, `tmp/windows-storport-cuda-vram-cuda-probe-cov.json`.

N/A cover (SPEC): `windows_driver.rs`, `windows_host.rs` (E2E Windows), driver C (WDK/Verifier), PowerShell drills.

### SPEC matrix → TestName present

| TestName | Present |
| --- | --- |
| `parse_product_config`, `reject_unknown_backend`, `reject_queue_data_area_over_4mib`, `reserve_cannot_lower_policy_floor` | yes |
| `append_preserves_prior_rows`, `nearest_rank_percentiles_are_deterministic`, `stable_error_redacts_payload` | yes |
| `roundtrip_write_read_flush`, `unknown_flags_return_einval`, `write_uses_owned_payload_snapshot`, `invalid_slot_does_not_touch_backend` | yes |
| `granted_bytes_must_equal_requested`, `release_flushes_before_session_close`, `release_twice_writes_once`, `lease_denied` | yes |
| `no_fallback_after_cuda_failure`, `failure_after_register_unwinds_reverse`, `deterministic_failure_is_not_retried`, `stop_twice_has_one_effect`, `ambiguous_crash_state_is_not_replayed`, `cuda_watchdog_does_not_destroy_stuck_context` | yes |
| `pagefile_active_refuses_before_mutation`, `pagefile_query_error_refuses_before_mutation`, `gate_b_failure_resumes_online_before_destroy`, `pagefile_absent_tears_down_cleanly`, `stop_refusal_preserves_online_state` | yes |
| `probe_cuda_allocates_roundtrips_and_restores` | **live PASS** on WSL GPU (libcuda); Windows nvcuda product bind still lab |
| IOCTL / Verifier / VPD live verdicts | env-bound (WDK lab VM) |
| `storage_only_cuda_three_rounds_sha256` + drill refusals | env-bound (physical Windows GPU + approved drill) |

### E2E

| Gate | Status |
| --- | --- |
| Linux pure path | unit/cover green |
| Live CUDA probe (DT-3) | **PASS** — see evidence log (512 MiB, 3 offsets, free restore 0 delta) |
| `cargo build -p ramshared-winsvc --target x86_64-pc-windows-msvc` | **compiles** (lib+bin typecheck); **link fails** — no `link.exe` (MSVC Build Tools env-bound) |
| WDK Release / InfVerif / SDV / Verifier | env-bound |
| Checkpointed VM IOCTL drill | scaffold + full `WindowsDriverLink` code; live verdicts env-bound |
| Physical Windows StorPort 3-round SHA-256 | env-bound; requires `-ApprovePhysicalHost` |
| BINARY_MATCH `ramsharedd` | **N/A** (Windows-only slice) |

**before → action → after (ITEM-2 probe):**

1. **Before:** free VRAM ≈ 5351931904 B (RTX 2060)  
2. **Action:** `cargo test -p ramshared-winsvc probe_cuda_allocates_roundtrips_and_restores -- --ignored` and `./target/release/ramshared-winsvc probe-cuda --config /tmp/.../winsvc.toml`  
3. **After:** free restored 5351931904 B; zero pattern mismatches; evidence: `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/probe-cuda-wsl-20260715.log`

## Gaps

| Kind | Detail |
| --- | --- |
| **env-bound** | MSVC `link.exe` / WDK build / Driver Verifier / SCM / broker release log |
| **env-bound** | Physical Windows StorPort CREATE/REGISTER + 3-round NTFS SHA-256 |
| **closed** | Pure policy + Gate A/B + queue snapshots + broker release once |
| **closed** | `WindowsDriverLink` VirtualAlloc/IOCTL + `WindowsHostState` elevation/lock/SHA-256 (source; link env-bound) |
| **closed** | Live CUDA three-offset probe on host GPU (WSL libcuda) |
| **open** | Pagefile product path remains out of this slice |

## Rollback trigger

- Any BugCheck / new dump during Verifier IOCTL campaign.
- Checksum mismatch or identity mismatch on storage-only LUN.
- Request latency > 5,000 ms or teardown > 30,000 ms on physical campaign.
- CUDA free not restored within 64 MiB after probe/teardown.
- Pagefile appears on the RamShared volume during stop → refuse code 7; do not force UNREGISTER/DESTROY.

## Traceability

| RF | ITEM | Evidence in this turn |
| --- | --- | --- |
| RF-1 | ITEM-1,2,4 | config + probe plan + runtime |
| RF-2 | ITEM-3,4 | driver_link + miniport owner/rundown (compile-time C; live env-bound) |
| RF-3 | ITEM-1,2,4 | broker grant equality + unwind |
| RF-4 | ITEM-1,5,6 | evidence schema + format script + measure SHA-256 |
| RF-5 | ITEM-4,5 | Gate A/B + stop refusal code 7 |
| RF-6 | ITEM-4,5 | product CLI; lab installer renamed |
| NFR-1..6 | mixed | unit/cover green; live NFR gates env-bound |
