# PREFLIGHT — windows-swap-driver (ready for IMPL)

> Checklist and gap closure so **ITEM-1…ITEM-11** can start without inventing paths,
> ABI, or safety gates. **Source of truth for design remains `SPEC.md`.**

## Status snapshot (2026-07-09)

| Area | State | Notes |
| --- | --- | --- |
| PRD | GO | `PRD.md` — see **PRD errata** below (SPEC wins on conflicts) |
| SPEC | GO (re-audit) | `SPEC.md` unique in-place; C1–H4 closed |
| IMPL | Scaffold | `IMPL.md` tracks ITEMs; no full feature yet |
| ADR | Done | [ADR-0006](../../../decisions/ADR-0006-storport-virtual-miniport.md) |
| ABI | Frozen scaffold | `drivers/windows/ramshared/protocol.h` + `crates/ramshared-winsvc/src/proto.rs` |
| Service crate | Stub | `ramshared-winsvc` in workspace; Linux exit 2; proto tests green |
| CUDA Windows loader | **Mostly done** | `loader_unix` / `loader_win` + `CANDIDATES=nvcuda.dll` already in tree (ITEM-1 residual = Windows host `mem_info` evidence) |
| `VramBackend` promote | **Pending ITEM-2** | still lives in `wsl2d/backend.rs` |
| `TransportKind::WinDrive` | **Pending ITEM-3** | only `NbdUnix`/`NbdTcp` today |
| Driver C tree | Scaffold only | README + protocol; no `.sys` yet |
| Scripts | Stubs | `scripts/windows/*.ps1` + `Get-WinDrivePreflight.ps1` |
| DEGRADATION-MATRIX | Updated | Windows B1/B2/pagefile rows reserved |
| Host-real driver | **Blocked** | until ITEM-8 + residency (DT-21) + signing policy (R9/ITEM-11) |

## Open before coding ITEM-5+ on a machine

1. **Disposable Hyper-V VM** (Win11 ideally `26200.*`) with snapshot — RNF-6.
2. **EWDK/WDK** for StorPort build (DT-14).
3. **Test-signing** only inside that VM until attestation path exists.
4. **GPU** needed from ITEM-1/6 on the Windows side that runs CUDA; pure disk/pagefile drills can use non-GPU backends first (Passo 0 precedent).
5. **Broker** reachable over TCP if exercising RF-5 (WSL2/Linux broker + Windows tenant).

```powershell
# On Windows host or VM (safe):
.\scripts\windows\Get-WinDrivePreflight.ps1
```

## Implementation order (do not skip)

See SPEC § "Ordem de implementação". Short form:

| # | ITEM | First concrete step |
| --- | --- | --- |
| 1 | RF-4 CUDA | Confirm `Cuda::load` + `mem_info` on native Windows with `nvcuda.dll` (code largely present) |
| 2 | VramBackend | Move `VramBackend` → `ramshared-block`; re-export from wsl2d; Linux drills green |
| 3 | winsvc + broker | `TransportKind::WinDrive`, lease e2e, **no driver** |
| 4 | ABI | Already scaffolded — keep golden-bytes green when changing fields |
| 5 | Driver MVP | StorPort + control device + rings (VM, test-signing) |
| 6–7 | I/O + pagefile | `driver_link`, `NtCreatePagingFile` allow-list 26200 |
| 8 | Kernel-page drill | **Hard gate** before host-real |
| 9–11 | Bench / soak / sign | Capacity gate + attestation |

## PRD errata (SPEC is authoritative)

| PRD text | SPEC resolution |
| --- | --- |
| IOCTL names `REGISTER_RING` / `START_DEVICE` / `STOP_DEVICE` | Use `REGISTER_QUEUE`, `UNREGISTER_QUEUE`, `COMMIT_AND_FETCH`, `CREATE_DISK`, `DESTROY_DISK` |
| Service name `ramsharedwsvcd` | Binary/crate **`ramshared-winsvc`** (SCM display name free at IMPL) |
| RNF-1 “48h” soak | **72h = 3×24h** (DT-12 / ITEM-10) |
| RNF-5 “broker triggers eviction” / 5s | **Holder-cooperative** + disconnect (DT-19); no `LeaseRevoke` Msg |
| RF-2 “>80% raw memcpy” | Not the promotion gate; RNF-2 is **capacity + bounded p99** (DT-13) |
| Error path “watchdog thread” | Crash containment via **IRP_MJ_CLEANUP → QTeardownOnCrash** (DT-10), not a free-running watchdog inventing timeouts |

## Gap closure log (this preflight pass)

| Gap | Action |
| --- | --- |
| No tree under `drivers/windows/` | Created README + `protocol.h` + driver README |
| No `ramshared-winsvc` member | Workspace member + stub + proto tests |
| No harness scripts | `scripts/windows/*` stubs + preflight |
| No ADR for StorPort Day-0 | ADR-0006 |
| No IMPL tracking | `IMPL.md` scaffold |
| DEGRADATION-MATRIX missing Windows modes | Rows added (designed / pending evidence) |
| SPEC path `docs/windows-vram-drive/IMPL.md` | Corrected to this folder’s `IMPL.md` |
| LIBRARIES / ROADMAP silent on WinDrive | Updated pointers |

## Explicitly NOT done (correct for Day-0)

- No `.sys` binary, no INF package, no pagefile on the live host.
- No Partner Center submission (R9 organizational).
- No invented broker force-revoke API.
- No dual-path “ImDisk forever” product path (drill-only historical).

## Ready when

- [x] SPEC GO + PREFLIGHT + ADR + ABI freeze + stub crate + scripts
- [ ] ITEM-2 land (`VramBackend` promote) — first code slice after this scaffold
- [ ] ITEM-8 evidence before host-real load
