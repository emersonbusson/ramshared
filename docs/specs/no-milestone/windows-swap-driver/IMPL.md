# IMPL — windows-swap-driver (StorPort VRAM pagefile)

> SSDV3 PASSO 3 tracking file. Design SoT: [`SPEC.md`](SPEC.md).  
> Preflight: [`PREFLIGHT.md`](PREFLIGHT.md). Branch naming suggestion: `feat/windows-swap-driver`.

## Status: scaffold / pre-IMPL

Workspace is green with a **Linux stub** of `ramshared-winsvc` and a **frozen ABI** mirror.  
No StorPort `.sys` yet. **Do not load drivers on the daily host.**

## ITEM tracker

| ITEM | RF/RNF | Status | Evidence |
| --- | --- | --- | --- |
| ITEM-1 | RF-4 CUDA loader | **Partial** | `loader_unix.rs` / `loader_win.rs` + Windows `CANDIDATES` exist; residual: native Windows `Cuda::load` + `mem_info` log on RTX host |
| ITEM-2 | VramBackend → block | **Pending** | Still in `wsl2d/backend.rs` |
| ITEM-3 | winsvc + WinDrive transport | **Scaffold** | Crate + stub main; no `TransportKind::WinDrive` yet |
| ITEM-4 | ABI protocol.h + proto.rs | **Scaffold done** | Sizes + golden SQE test; keep in lockstep |
| ITEM-5 | StorPort driver MVP | **Pending** | Tree README only |
| ITEM-6 | driver_link I/O loop | **Pending** | — |
| ITEM-7 | NtCreatePagingFile | **Pending** | Allow-list DT-24 = 26200.* |
| ITEM-8 | Kernel-page drill | **Pending** | Hard gate; script stub |
| ITEM-9 | Measure-PagefileVram | **Pending** | Script stub |
| ITEM-10 | Driver soak 72h | **Pending** | Script stub |
| ITEM-11 | Attestation install | **Pending** | Script stub; R9 org |

## Files landed in preflight (not feature-complete)

| Path | Role |
| --- | --- |
| `drivers/windows/ramshared/protocol.h` | Frozen C ABI |
| `crates/ramshared-winsvc/` | Stub binary + `proto.rs` tests |
| `scripts/windows/*.ps1` | Harness stubs + preflight |
| `docs/decisions/ADR-0006-storport-virtual-miniport.md` | Architecture decision |
| `docs/reliability/DEGRADATION-MATRIX.md` | Windows modes reserved |

## Validation (scaffold)

```bash
cargo test -p ramshared-winsvc
cargo test --workspace   # no regressions
cargo fmt --all -- --check
```

## Next commit suggestion

`feat(mm): promote VramBackend into ramshared-block (ITEM-2)`  
then `feat(mm): add TransportKind::WinDrive and winsvc lease client (ITEM-3)`.

## Rollback trigger (kernel / host)

- Any BugCheck on a non-VM host → stop host loads; re-run ITEM-8 only in VM.
- Linux drill regression after ITEM-1/2 → revert that ITEM immediately (RNF-8).
