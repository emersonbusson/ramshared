# FAQ — short answers

## What is this?

When RAM is full, Linux starts using the **disk**. That feels bad.  
RamShared adds a **middle cushion**: idle **GPU** memory. If the GPU gets busy, that cushion is **given back** and data goes to disk again. Apps keep running.

## Will it break / freeze my PC?

**Designed so normal use shouldn’t freeze WSL2.**

Past freezes came from turning things off the wrong way (killing the GPU swap daemon while pages were still on that device). Today `ramshared down` always turns swap off **first**.

What you *might* notice:

- A **short slowdown** when a game on Windows reclaims VRAM (we measured up to ~1.2 s for a tiny read under hard reclaim; a full demote of hundreds of MB can take on the order of **tens of seconds**).
- That is **not** the same as “WSL dead forever.”

Heavy thrash tests: we don’t run those on the WSL you work in every day.

## Is it free RAM for games?

**No.** If a game already fills the GPU, there’s little idle memory to borrow. This is for **workstation pressure** (builds, containers, browsers) while the GPU is often idle.

## What do I need?

- Linux or **WSL2**
- NVIDIA GPU + working `nvidia-smi`
- Rust to build
- sudo

```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared doctor   # if check fails
```

## Why does Task Manager show RamShared disk at 100% with 0 KB/s and 0 ms?

That is a **Windows UI limitation + common virtual miniport footgun**, not “infinite speed”.

What you usually see on **Disco N — RAMSHARE VRAMDISK**:

| Field | Meaning |
| --- | --- |
| **Tempo de atividade 100%** | Class driver is polling / queue looks “busy” |
| **0 KB/s read/write** | No (or few) completed transfers counted as user data |
| **0 ms** | Latency counters not meaningful for that path |
| **Formatado: 0 MB** | LUN is still **RAW** (no NTFS partition) — Task Manager has no volume |

Important:

1. **Letter `V: RAMSHARED` may be a physical SSD** that was labeled earlier — **not** the 64 MiB virtual LUN. Always check `Get-Disk` name + size (lab LUN is usually 64 MiB / Fibre Channel / `RAMSHARE VRAMDISK`).
2. **Backend must be alive** (`WinDriveBackend` / winsvc). A ghost RAW disk after backend exit makes `Initialize-Disk` fail with StorageWMI **40004** (writes never complete).
3. Prefer real metrics (do **not** trust Task Manager alone):

```powershell
# Elevated from WSL: ./scripts/windows/wsl-elevated-ps.sh -File C:\ramshared\bin\...
# Start backend if needed, then format only the RamShared LUN (free letter, not V: if V: is physical):
.\scripts\windows\Start-RamSharedLab.ps1 -SizeBytes 67108864 -HoldSeconds 3600
.\scripts\windows\Format-RamSharedLun.ps1 -ExpectedSizeBytes 67108864 -DriveLetter S -Force
# Locale-safe PerfDisk (CIM) + optional sequential probe:
.\scripts\windows\Measure-RamSharedDiskIo.ps1 -Seconds 10 -DriveLetter S
```

4. Day-0 driver fix: **TEST UNIT READY no longer returns `SRB_STATUS_BUSY`** when the LUN is not ready (that made StorPort requeue forever → stuck 100%). It returns CHECK CONDITION **NOT READY** with autosense instead. Rebuild/reload `ramshared.sys` to pick that up.

**Live lab (host 2026-07-14):** Disk5 `RAMSHARE VRAMDISK` 64 MiB RAW → GPT+NTFS `S:` label `RAMSHARED` with backend alive; direct probe **8 MiB write ≈ 1224 MB/s, read ≈ 146 MB/s, match=True**. Task Manager can still show odd % busy on StorPort; use the measure script.

## Is RamShared using my VRAM right now?

Run:

```bash
./target/release/ramshared status
# or machine-readable:
./target/release/ramshared status --json
```

| Phase | Meaning |
| --- | --- |
| **Armed** | VRAM tier is on as swap (`nbd`), daemon is up, but **almost no pages** there yet (idle cushion). |
| **UsingZram** | Guest pressure is mostly on compressed RAM. |
| **UsingVram** | Real use of the GPU-backed tier (used ≥ ~1 MiB). |
| **UsingDisk** | Spilled past VRAM to disk/VHDX. |
| **Demoting** | Giving VRAM back (host GPU pressure / canary) — only when the daemon reports it. |
| **Degraded** | Ghost swap, bad priority order, or VRAM swap without daemon — fix before relying on the cushion. |
| **Off** | Product cascade not present. |

`cascade-health.sh` includes the same `phase` when the CLI binary is available.

## How do I know it worked?

```bash
swapon --show
```

Aim for three lines: **zram** (first), **GPU-backed** device (second), **disk** (last). Names vary; **order** matters.

## Is there a simple app / menu?

Yes — a small control panel (not a fancy store app):

```bash
bash scripts/safety/install-cascade-app.sh
./scripts/safety/cascade-app.sh --gui
```

Same actions as the CLI: start, stop, status, check, boot on/off.  
Under the hood it still calls the safe `ramshared up` / `down` paths.

## Turn on at WSL boot?

Yes, if you want — from the control app (**Enable boot**) or:

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

Needs systemd in the distro. Config: `/etc/ramshared/cascade.conf`.  
Undo: **Disable boot** in the app, or `sudo bash scripts/safety/uninstall-cascade-boot.sh`.

## What happens when I open a game?

The daemon watches free GPU memory and latency. If the card is under pressure, it **stops using GPU as swap** (DEMOTE). Pages move to disk. Processes in WSL are **not** killed on purpose.

You may feel WSL get sluggish for a while. If it’s stuck for a long time, check `swapon --show` and logs (`journalctl -u ramshared-cascade -b` if you used boot). As a last resort on Windows: `wsl --shutdown` (only after you’ve tried `ramshared down` when you can).

## Can I install the Windows kernel driver on my physical host?

**Yes, for development and testing (MVP/Beta).** You can compile and load the driver on a physical machine, but it requires two prerequisites:
1. **Disable Secure Boot** in your motherboard UEFI/BIOS settings.
2. Enable Windows Test Mode by running:
   ```powershell
   bcdedit.exe /set "{current}" testsigning yes
   bcdedit.exe /set "{current}" nointegritychecks yes
   ```
3. Reboot your PC and compile/sign the drivers using the provided scripts.

*Note:* Force-killing the backend while the virtual disk contains active pagefile pages will cause a bluescreen (**0x7A**). Ensure you stop the pagefile usage before stopping the backend.

## Does mixing GDDR6 and DDR4 memory cause compatibility or latency issues?

No. GDDR6 (on the GPU) and DDR4 (on the motherboard) never communicate directly; each is managed by its own physical memory controller. All data transfers between them go through the PCI-Express (PCIe) bus, which standardizes the communication.

While the GPU's internal GDDR6 bandwidth is massive (e.g., 336 GB/s), the transfer speed is bounded by the PCIe bus bandwidth (e.g., PCIe Gen3 x16 is limited to ~15.8 GB/s). However, even at ~15.8 GB/s, this is **several times faster** than high-end NVMe SSD write speeds, and the access latency is in the microsecond range (µs) compared to milliseconds (ms) for SSDs and HDDs. Data integrity is fully protected by hardware-level PCIe CRC and parity checks.

## Where are the real numbers?

[validation.md](../validation.md) and [docs/reliability/](reliability/). If a number isn’t written there, treat marketing claims as suspect.
