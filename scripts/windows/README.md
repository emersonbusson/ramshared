# Windows VM harness scripts (SPEC windows-swap-driver)

**SPEC:** [`docs/specs/no-milestone/windows-swap-driver/SPEC.md`](../../docs/specs/no-milestone/windows-swap-driver/SPEC.md)  
**Safety:** run only inside a disposable Hyper-V VM (RNF-6). Never thrash pagefile/swap on the live host.

| Script | ITEM | Role |
| --- | --- | --- |
| `Invoke-KernelPageDrill.ps1` | 8 | Kernel paged-pool → pagefile-VRAM; kill service; B1 vs B2; ≥3 runs with residency gate (DT-21) |
| `Invoke-RevokeDrill.ps1` | RNF-5 | Holder-cooperative teardown + `LeaseRelease` (DT-19) — **no** invented broker Msg |
| `Measure-PagefileVram.ps1` | 9 | Side-by-side page-in latency vs disk; ≥3 runs → `docs/benchmarks/results.jsonl` + `BENCHMARKS.md` |
| `Invoke-DriverSoak.ps1` | 10 | Driver Verifier soak 3×24h (DT-12) |
| `Build-Sign-Install.ps1` | 11 | Build / attestation package / install (Partner Center flow) |
| `Get-WinDrivePreflight.ps1` | preflight | Host/VM readiness checklist (no driver load) |

Stubs print the planned surface and exit non-zero until ITEM implementation fills them in.
