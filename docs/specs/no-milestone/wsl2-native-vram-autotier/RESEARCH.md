# Research appendix — official mechanism mapping

| Proposal | Official source | Conclusion |
| --- | --- | --- |
| GPU-PV ownership | [DirectX on the Linux kernel](https://devblogs.microsoft.com/directx/directx-heart-linux/) | `/dev/dxg` transports WDDM calls; Windows owns physical GPU memory management. |
| Budget fields | [DXGI query video memory info](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/ns-dxgi1_4-dxgi_query_video_memory_info) | Budget, usage, reservation, and available reservation are host-authoritative signals. |
| Dynamic trim | [Process residency budgets](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/process-residency-budgets) | Applications must respond as budgets change. |
| WSL uAPI | [WSL2 Linux kernel `d3dkmthk.h`](https://github.com/microsoft/WSL2-Linux-Kernel/blob/linux-msft-wsl-6.18.y/include/uapi/misc/d3dkmthk.h) | Enum/open/query/close layouts and ioctl numbers are already shipped. |
| HMM requirements | [Linux HMM documentation](https://docs.kernel.org/6.15/mm/hmm.html) | HMM needs a real device-memory owner and migration callbacks; enabling a config does not create a tier. |
| Kernel cold-page selection | [zram writeback](https://www.kernel.org/doc/html/next/admin-guide/blockdev/zram.html) | Idle/age tracking can select pages for backing-device writeback in Phase 2. |
| Automatic WSL policy pattern | [WSL autoMemoryReclaim](https://devblogs.microsoft.com/commandline/windows-subsystem-for-linux-september-2023-update/) | Observe, reclaim gradually, bound behavior, and expose a small configuration surface. |
| Contribution route | [WSL2-Linux-Kernel guidance](https://github.com/microsoft/WSL2-Linux-Kernel) | WSL feature request goes to `microsoft/WSL`; reusable kernel work should go upstream. |

## Explicit non-mappings

GPU-PV exposes no guest-owned PFN/dev_pagemap contract for RamShared, so `MEMORY_DEVICE_PRIVATE`, fake NUMA, and HMM migration are not valid WSL implementations. `dxgkrnl` is a graphics/compute control plane; the proposed Phase 3 storage frontend belongs in a dedicated VMBus `blk-mq` driver.
