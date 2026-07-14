# Windows drivers tree

Day-0 StorPort path for **native Windows** VRAM / lab-backed pagefile (P4 / Track 2).

### Task Manager shows 100% / 0 KB/s on RAMSHARE VRAMDISK

Three layered causes (fix in order):

1. **RAW LUN** — no NTFS → Task Manager "Formatado: 0 MB". Format with `Format-RamSharedLun.ps1` only while **WinDriveBackend is alive** (else StorageWMI 40004).
2. **SRB_STATUS_BUSY on TUR** (old builds) — StorPort requeues forever → stuck 100%. Current `virtdisk.c` returns SCSI **NOT READY** autosense; rebuild/reload `ramshared.sys`.
3. **Wrong volume** — letter `V: RAMSHARED` may be a physical SSD, not the 64 MiB virtual LUN.

For real MB/s use locale-safe `scripts/windows/Measure-RamSharedDiskIo.ps1` (CIM PerfDisk + optional file probe), not Task Manager alone.

## Status (2026-07-09)

| Item | State |
| --- | --- |
| Build (host WDK) | `ramshared.sys` + `poolstress.sys` via `scripts/windows/Build-Drivers.ps1` |
| Load environment | Hyper-V **`win11-drill` only** (test-signing) |
| Product disk | LUN **RAMSHARE VRAMDISK** (e.g. 64 MiB lab) via INF + `devcon install` Root\RamShared |
| Pagefile / ITEM-8 | Lab **PASS** residency + KPD; see IMPL |
| **Physical host load** | **FORBIDDEN** until host-real gates in IMPL |

## Pointers

| Doc | Role |
| --- | --- |
| [`docs/specs/no-milestone/windows-swap-driver/`](../../docs/specs/no-milestone/windows-swap-driver/) | PRD · SPEC · IMPL · PREFLIGHT |
| [`crates/ramshared-winsvc/`](../../crates/ramshared-winsvc/) | Userspace protocol + product service scaffold |
| [`scripts/windows/`](../../scripts/windows/) | Build, install, Start/Stop lab, B1/B2/DT-9 drills |
| [`docs/reliability/DEGRADATION-MATRIX.md`](../../docs/reliability/DEGRADATION-MATRIX.md) | B1/B2/0x7A/DT-9 |
| Root [`README.md`](../../README.md) | Day-1 product remains **Linux/WSL2** |

## Hard rules

1. **RNF-6:** crash and pagefile drills only on disposable VMs with checkpoints.
2. **DT-9:** never destroy the disk or kill the backend while secondary pagefile is hot (BugCheck **0x7A** / `c0000185` proven).
3. **#13:** lab green does not mean host-real ready.
4. Linux / WSL2 cascade does **not** require this tree.
