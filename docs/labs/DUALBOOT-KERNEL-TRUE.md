# Dual-boot + kernel-true — why it was blocked, what we fixed

## Short answer

**It was not a project policy ban.** The **environment** could not create free space for Linux on **R: (RUSSIA)** the way Windows shrink works. That is fixed by carving space on **E: (ESPANHA)** instead.

| Disk | Label | Role | Dual-boot space |
| --- | --- | --- | --- |
| C: | CANADA | Windows system SSD | **Never** put Linux here |
| R: | RUSSIA | Media + Hyper-V lab | **Blocked** for shrink (~2.7 GB only) |
| E: | ESPANHA | Large data HDD (Samsung HD154UI) | **READY** — **32 GB unallocated** (2026-07-10) |

---

## Why R: “did not allow” dual-boot

Windows `Resize-Partition` only shrinks from the **end** of the NTFS volume.  
It reports `Get-PartitionSupportedSize` → **SizeMin**.

On R: (Hitachi 466 GB):

- Free **inside** NTFS: ~**170 GB**
- Shrinkable: only ~**2.68 GB**

So free space exists, but **immovable / high-LBA data** keeps SizeMin near full size. Typical causes:

1. Large media files in `R:\BAIXADOS RUSSIA\` (20–28 GB each) scattered so free space is not at the **end**
2. NTFS metadata / previous Hyper-V files / fragmentation
3. Defrag helped fragmentation but **did not** move everything that blocks SizeMin

**Free space ≠ shrinkable space.** That is a Windows NTFS rule, not RamShared refusing dual-boot.

---

## What we did (resolved)

1. Measured all disks and shrink capacity.  
2. **Shrunk E: (ESPANHA)** by ~**32 GB**.  
3. Disk 0 now has **LargestFreeExtent ≈ 32 GB** unallocated — installer can create `ext4` root there.  
4. E: data kept; only free NTFS capacity reduced (FreeGB dropped from ~329 to ~297 as expected).

Ubuntu live ISO already on lab store:

`R:\Hyper-V\iso\ubuntu-24.04.2-live-server-amd64.iso` (~3 GB)

---

## Finish dual-boot (one human boot — unavoidable)

PowerShell **cannot** complete a bare-metal Ubuntu install without a reboot into the installer (or USB). Remaining steps:

1. **USB boot** (recommended): Rufus/Ventoy + the ISO above.  
2. Firmware: boot USB once.  
3. Installer → **Something else** / manual:  
   - Use **only the 32 GB unallocated** on **SAMSUNG HD154UI** (the disk that has E:).  
   - **Do not** format C: or wipe entire disks.  
   - EFI: use existing ESP on the Windows SSD (disk with C:), or create ESP only if you know what you are doing.  
4. After install: boot menu (F12) → Ubuntu.  
5. On bare metal Linux:

```bash
lspci -nn | grep -i nvidia
ls -la /dev/dri
# then proprietary or open NVIDIA stack for kernel-true experiments
```

With **real** `/dev/dri` + `10de:` PCI id, **kernel-true Gate A** (not WSL GPU-PV) can proceed to measurement / SPEC.

---

## Kernel-true vs dual-boot

| Goal | Needs |
| --- | --- |
| Dual-boot Ubuntu for kernel builds | **Unallocated 32 GB on E:** — **done** |
| Kernel-true VRAM as process memory | Dual-boot **into** that Linux **or** DDA (host loses GPU) |
| WSL GPU-PV | Still **not** bare-metal BARs (vendor `0x1414`) |

Dual-boot space is the **blocker we could remove in Windows**.  
Actually installing Ubuntu still needs **one** boot from USB (BIOS/UEFI interaction).

---

## If you need more than 32 GB root later

- Move/delete media on **E:** and re-run shrink, or  
- Free immovable blockers on **R:** (move `BAIXADOS` off the volume end), defrag, re-check `Get-PartitionSupportedSize -DriveLetter R`.

---

## Related

- Inventory (WSL-only GPU): `docs/specs/no-milestone/kernel-vram-as-memory/PASSO0-INVENTORY.md`  
- Mainline strategy: `docs/specs/no-milestone/mainline-vram-tiering/PRD.md`  
- Hyper-V lab (not dual-boot): `docs/labs/HYPERV-RUSSIA-LAB.md`
