# PREFLIGHT — windows-swap-driver

> Checklist and gap closure for ITEM-1…ITEM-11. **Source of truth for design remains `SPEC.md`.**  
> **Live gates:** `IMPL.md` · empirical log: root [`validation.md`](../../../../validation.md).

## Status snapshot (2026-07-09 closeout)

| Area | State | Notes |
| --- | --- | --- |
| PRD | GO | `PRD.md` — see **PRD errata** (SPEC wins on conflicts) |
| SPEC | GO (re-audit) | `SPEC.md` unique in-place; C1–H4 closed |
| IMPL | **lab-complete / host-real blocked** | See `IMPL.md` gate table |
| ADR | Done | [ADR-0006](../../../decisions/ADR-0006-storport-virtual-miniport.md) |
| ABI | Frozen + exercised | `protocol.h` + `proto.rs`; CREATE/REGISTER on VM |
| Service crate | Lab + scaffold product | Pure tests green on Linux; C# `RamSharedWinSvc` lab SCM; Rust bin needs MSVC |
| CUDA Windows loader | Code present | `loader_win` + `nvcuda.dll` candidates; **no** host-real `Cuda::load` evidence yet |
| `VramBackend` promote | **Done** (Linux) | in `ramshared-block`; lab Windows uses file backend |
| `TransportKind::WinDrive` | **Done** | broker + wsl2d filter |
| Driver C tree | **Built + loaded in VM** | `ramshared.sys`, INF+devcon, LUN 64 MiB |
| Scripts | **Operational** | Start/Stop lab, B1/B2, DT-9, ITEM-8, disciplined campaign |
| DEGRADATION-MATRIX | Updated | B1/B2/DT-9 rows with empirical status |
| Host-real driver | **Blocked** | lab ITEM-8 green ≠ host-real |

## Lab evidence closed (VM only)

| Gate | Result |
| --- | --- |
| Format + smoke NTFS | PASS |
| DT-21 pagefile residency | PASS (Usage 25%, KPD 3/3) |
| DT-9 refuse hot / reboot kill | PASS |
| B1 safe arm (no PF) | PASS |
| B2 pagefile-hot | FAIL 0x7A → DT-9 required |
| Lab SCM delayed-auto | PASS_LAB_SCM |

## Still required before host-real

1. Disposable Hyper-V (or equivalent) + snapshot — RNF-6 remains the default for crash drills.
2. Product path: `nvcuda.dll` + `VramBackend` (or proven free-floor policy) on a **GPU-capable** Windows machine.
3. MSVC + cargo for product `ramshared-winsvc` (or documented lab SCM sunset with proof — Kahneman #18).
4. Test-signing only until attestation (ITEM-11 / R9).
5. ITEM-9 measured K; ITEM-10 soak — no invented numbers.
6. Explicit go for host-real in IMPL + `validation.md` — never by chat assertion.

```powershell
# On Windows host or VM (safe preflight):
.\scripts\windows\Get-WinDrivePreflight.ps1
```

## Implementation order (historical → current)

| # | ITEM | Status |
| --- | --- | --- |
| 1 | RF-4 CUDA loaders | Code done; Windows mem_info evidence open |
| 2 | VramBackend → block | Done (Linux green) |
| 3 | winsvc + broker WinDrive | Done (lab); product bin env-bound |
| 4 | ABI | Frozen + used on VM |
| 5 | Driver MVP | Done in VM (test-sign) |
| 6–7 | I/O + pagefile | Done in VM lab path |
| 8 | Kernel-page / residency | **Lab PASS**; host-real still hard-gated |
| 9–11 | Bench / soak / sign | Open |

## PRD errata (SPEC is authoritative)

| PRD text | SPEC resolution |
| --- | --- |
| IOCTL names `REGISTER_RING` / `START_DEVICE` / `STOP_DEVICE` | Use `REGISTER_QUEUE`, `UNREGISTER_QUEUE`, `COMMIT_AND_FETCH`, `CREATE_DISK`, `DESTROY_DISK` |
| Service name `ramsharedwsvcd` | Binary/crate **`ramshared-winsvc`** (SCM display name free at IMPL) |
| RNF-1 “48h” soak | **72h = 3×24h** (DT-12 / ITEM-10) |
| RNF-5 “broker triggers eviction” / 5s | **Holder-cooperative** + disconnect (DT-19); no `LeaseRevoke` Msg |
| RF-2 “>80% raw memcpy” | Not the promotion gate; RNF-2 is **capacity + bounded p99** (DT-13) |
| Error path “watchdog thread” | Crash containment via **IRP_MJ_CLEANUP → QTeardownOnCrash** (DT-10) |

## Explicitly NOT done (correct for Day-0)

- No **host-real** `.sys` load on a physical daily driver machine.
- No Partner Center submission (R9 organizational).
- No invented broker force-revoke API.
- No dual-path “ImDisk forever” product path (drill-only historical).
- No claim that B2 pagefile-hot is “contained” without BSOD — evidence is **0x7A** + DT-9 refuse.

## Ready when

- [x] SPEC GO + PREFLIGHT + ADR + ABI + driver load in VM
- [x] ITEM-8 **lab** evidence (residency + DT-9 + B1 safe + lab SCM)
- [ ] Product CUDA Windows path evidence
- [ ] ITEM-9 / 10 / 11 as applicable
- [ ] Explicit host-real go in IMPL + validation log
