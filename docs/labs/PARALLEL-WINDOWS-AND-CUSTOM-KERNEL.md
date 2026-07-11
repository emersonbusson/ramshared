# Parallel work: Windows lab + custom kernel (VRAM as swap)

> Snapshot **2026-07-10**. What can run at the same time, and what is running now.

## What you asked

1. **Fix Windows lab** (`win11-drill`)  
2. **Custom kernel from latest official Microsoft WSL tree**, with configs that support **memory/swap path for VRAM-as-swap** (NBD/ublk, zram, writeback, io_uring)

## Can both run at once?

| Work | On | Parallel? |
| --- | --- | --- |
| Win11 ISO download + VM install (vmconnect clicks) | Host Hyper-V / R: | **Yes** with kernel **build** |
| Kernel `make -j2` | `linux-kernel-lab` VM | **Yes** with Win11 install |
| Kernel **boot as WSL custom** + RamShared up | Host WSL | **After** build finishes; prefer **not** while Win11 install is heavy |
| Stress both VMs + WSL cascade thrash | Host RAM | **No** — risk freeze |

**Rule:** install Windows in the background (you click installer); let kernel compile in Linux lab; only when `bzImage` is ready, wire WSL custom kernel + `ramshared up` on the host.

## Current status (started for you)

### Windows (`win11-drill`)

| Item | Status |
| --- | --- |
| Location | **R:** only (not C:) |
| VHD | `R:\Hyper-V\win11-drill\Virtual Hard Disks\win11-drill.vhdx` (80 GB dynamic, empty) |
| ISO | `R:\Hyper-V\iso\Win11_25H2_English_x64_v2.iso` (~7.9 GB, **latest Fido 25H2**) |
| VM | **Running**, DVD = ISO, for **install** |
| You | Open **vmconnect** → finish Windows setup (language, disk = the empty VHD, local account) |
| After install | Eject ISO / boot from hard disk; optional `Disable-Win11LabUac.ps1` **inside** guest |

### Custom kernel (official MS tree)

| Item | Status |
| --- | --- |
| Tree | `microsoft/WSL2-Linux-Kernel` branch **`linux-msft-wsl-6.18.y`** (tag **linux-msft-wsl-6.18.35.2**) |
| Where | Hyper-V lab: `~/src/WSL2-Linux-Kernel` on `linux-kernel-lab` |
| Config base | `Microsoft/config-wsl` |
| Extras for VRAM-as-swap path | `CONFIG_BLK_DEV_UBLK=m`, `CONFIG_ZRAM_WRITEBACK=y`, `CONFIG_IO_URING=y`, `CONFIG_BLK_DEV_NBD=m`, `CONFIG_ZRAM=m`, `CONFIG_SWAP=y` |
| Build | `make -j2` background → log `~/kernel-build.log` |
| VRAM “borrow” itself | Still **userspace** RamShared (`ramsharedd` + swapon) on this kernel — **not** HMM in-kernel yet (ADR-0007 / P0) |

When build finishes:

```bash
# on lab
ls -la ~/src/WSL2-Linux-Kernel/arch/x86/boot/bzImage
# copy to Windows path for WSL, e.g.
# scp ... /mnt/c/wsl/kernel-ramshared
# then scripts/kernel/boot-kernel-safe.ps1 + qemu-validate first
```

Host script (WSL) when you prefer build on host instead of lab:

```bash
KTAG=linux-msft-wsl-6.18.y bash scripts/kernel/build-wsl-kernel.sh
```

## VRAM as swap (what “including memory management” means here)

| Layer | Language | Role |
| --- | --- | --- |
| Custom kernel | **C** (official MS tree) | swap, zram, nbd/ublk, io_uring, optional writeback |
| RamShared daemon/CLI | **Rust** | allocate VRAM (CUDA), serve block device, demote |
| Product on WSL | both | kernel provides swap stack; Rust provides GPU backend |

“Native C in kernel for full page↔VRAM mm” = later research (P2), not this build.

## Host WSL lab distro (throwaway)

| Item | Status |
| --- | --- |
| Name | `RamShared-Kernel` (Ubuntu 24.04) |
| Live | `R:\WSL\RamShared-Kernel\ext4.vhdx` (not C:) |
| Backup | `E:\WSL-backup\RamShared-Kernel\RamShared-Kernel-base.tar` |
| Auth | passwordless `emedev` + sudo NOPASSWD |
| Default product | still `Ubuntu-24.04` |
| Doc | [`WSL-KERNEL-LAB.md`](WSL-KERNEL-LAB.md) |

## Host safety

- Lab disks/ISOs on **R:**; WSL lab VHDX on **R:**; export on **E:**  
- C: free remains large  
- `gha-ubuntu` may be Off to free RAM for builds  
- Do not thrash swap on live WSL while both VMs are heavy  

## Checklist

- [x] Win11 ISO on R:  
- [x] win11-drill VHD + boot from ISO  
- [ ] **You:** complete Win11 setup in vmconnect  
- [x] Kernel tree 6.18.y + configs for swap/VRAM path  
- [ ] Kernel build finished (`bzImage`)  
- [ ] Validate (`qemu-validate` / boot-kernel-safe)  
- [ ] Optional: WSL `.wslconfig` → custom kernel + `ramshared up`  
- [x] WSL lab distro `RamShared-Kernel` on R: + base export on E:  

