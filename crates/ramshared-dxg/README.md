# ramshared-dxg

Direct `/dev/dxg` WDDM VidMm video-memory budget and adapter telemetry interface for WSL2.

## Scope & Responsibility

`ramshared-dxg` allows user-space daemons running in WSL2 to query host-authoritative GPU memory budgets directly from the Windows DirectX graphics kernel (`dxgkrnl`):
- **Adapter Enumeration:** Enumerates physical GPU adapters via `ENUM_ADAPTERS2_IOCTL`.
- **VidMm Budget Query:** Queries current GPU memory commitments, available budget, and WDDM memory pressure via `QUERY_VIDEO_MEMORY_INFO_IOCTL`.
- **Dynamic Headroom Verification:** Ensures that graphics applications retain `max(2 GiB, 20% VRAM)` headroom before RamShared scales up swap allocations.

## Workspace Dependencies

- Pure low-level driver interface; zero workspace crate dependencies.

## Safety Invariants

- **Isolated Unsafe Ioctls:** Mirrors Microsoft WSL2 `d3dkmthk.h` UAPI definitions; all raw ioctl calls are isolated with safe wrapper interfaces.
- **Fail-Safe Fallbacks:** When `/dev/dxg` is absent or unreadable, safely yields zero VRAM budget without kernel panic.

## Testing

```bash
cargo test -p ramshared-dxg
```

## WSL2 adapter identity: two LUID namespaces

Stock WSL2 `dxgkrnl` keeps **two distinct LUID namespaces** for the same physical
adapter and never exposes the host one to userspace:

| API | LUID returned | Example (one RTX 2060) |
| --- | --- | --- |
| `/dev/dxg` `ENUM_ADAPTERS2` | VM-bus **channel** LUID (`struct winluid luid`) | `00000000:455c7025` |
| CUDA `cuDeviceGetLuid` | host DXGI adapter LUID (`struct winluid host_adapter_luid`) | `00000000:00012055` |
| Windows DXGI `IDXGIAdapter1::GetDesc1` | host DXGI adapter LUID | `00000000:00012055` |

Kernel reference (`drivers/hv/dxgkrnl/ioctl.c`): `dxgkio_enum_adapters` fills
`inf->adapter_luid = entry->luid;` (the VM-bus channel LUID). The host LUID is
kept in `adapter->host_adapter_luid`, rewritten in only for `dxgkio_query_statistics`
and then restored before `copy_to_user`. **There is no ioctl that returns
`host_adapter_luid`.**

Consequence: string equality between an allocator LUID (CUDA/Vulkan) and a WDDM
LUID from this crate is **not a total test** for "same physical adapter" on WSL2.
`ramshared-wsl2d` therefore proves correspondence explicitly
(`AdapterCorrespondence` in `ramshared-wsl2d::gpu_budget`):

- `SharedLuid` — both APIs report the same Windows LUID string (normal case when
  a driver surface shares one namespace).
- `SoleAdapter` — the exact LUID request returned `AdapterNotFound` **and**
  `open(None)` opened exactly one adapter. With a single physical GPU there is
  only one possible pair, so the pair denotes it despite the namespace split.

Any other outcome (`NoAdapters`, `AmbiguousAdapters`) is **not** correspondence:
the provider is not opened and the budget falls back to allocator-only. The
strict `SharedLuid` check is still enforced inside `constrained_budget` when
correspondence is `SharedLuid`, so two loose snapshots can never be combined by
accident.
