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
