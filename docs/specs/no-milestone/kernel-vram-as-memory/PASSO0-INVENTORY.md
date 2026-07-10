# Passo 0 — Lab inventory (kernel-vram-as-memory)

> Gate A from `PRD.md` §14. Date: 2026-07-10. Host: `emedev` WSL2.

## Environment

| Check | Result |
| --- | --- |
| Kernel | `6.6.123.2-microsoft-standard-WSL2+` |
| WSL guest | **YES** (`WSLInterop` present) |
| GPU | NVIDIA GeForce RTX 2060, 6144 MiB total, ~4258 MiB free (sample) |
| Driver (guest view) | 610.74 via `nvidia-smi` |
| `/dev/dri` | **absent** |
| PCI “GPU” class | `0x030200` vendor **`0x1414`** device `0x008e` (Microsoft DXG / GPU-PV, not bare NVIDIA BARs) |
| `lsmod` nvidia/dxg | empty in this sample (userspace CUDA path still works via GPU-PV) |
| ReBAR / real BAR map | **Not available** as bare-metal ReBAR — PV virtual device only |
| systemd | runtime **yes** (`is-system-running=degraded`) |
| Desktop (WSLg) | `DISPLAY=:0`, `zenity` + `notify-send` present |
| Cascade binaries | `target/release/ramshared` + `ramsharedd` present |
| Swap now | disk `/dev/sdb` 8G only (cascade **not** armed at sample time) |

## Gate A verdict

| Gate | Pass? |
| --- | --- |
| A1 bare-metal or real passthrough | **FAIL** — WSL2 GPU-PV only |
| A2 inventory recorded | **PASS** (this file) |
| A3 pressure not on fragile host | N/A for inventory-only |

## Gate B

**Not run** — requires bare-metal / passthrough lab. Blocked by A1.

## Decision (reaffirmed)

| Track | Status |
| --- | --- |
| Kernel-true LKM / HMM / NUMA | **BLOCKED on hardware** for this lab |
| Cascade product + desktop control | **GO** (this is the usable path here) |

Next for trilha K: only when a non-WSL bare-metal machine is available. Do not open SPEC for K1–K4 on this host.
