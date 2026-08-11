# Runbook — Custom WSL2 kernel for Phase B (zram writeback + ublk)

> **Canonical SSDV3 (P1):** [`docs/specs/no-milestone/wsl2-custom-kernel-p1/`](../specs/no-milestone/wsl2-custom-kernel-p1/)  
> (PRD · SPEC · IMPL · AUDIT-2.5). Day-to-day CLI: `bash scripts/kernel/wsl-kernel.sh status|enable|arm|apply`.

Unblocks **Step 3 (IMPL) for items 4–5** (the zram-writeback topic + ublk;
see ADR-0004 and `docs/specs/no-milestone/wsl2-cascade-swap/`). The Microsoft
prebuilt kernel **does not** have these configurations (verified:
`# CONFIG_ZRAM_WRITEBACK is not set`, `# CONFIG_BLK_DEV_UBLK is not set`).
`CONFIG_IO_URING=y` **already exists**.

> **Warning:** the **boot** step requires `wsl --shutdown` (on Windows) — it
> ends **all** WSL sessions, including the agent's. Therefore build/install is
> a runbook: the owner controls the restart.

## Safe, reusable workflow (`scripts/kernel/` toolkit)

Use the toolkit — it is safe (it validates in isolation before arming) and
**self-healing** (it reverts itself if it does not boot). The manual sections
below are the reference for what the scripts do.

```sh
# 1. BUILD (official Microsoft base + configurations; verifies they applied; modules_install)
bash scripts/kernel/build-wsl-kernel.sh CONFIG_BLK_DEV_UBLK=m CONFIG_ZRAM_WRITEBACK=y CONFIG_IO_URING=y
#    → prints the bzImage and <release> (use it below).

# 2. ISOLATED VALIDATION (QEMU, DOES NOT touch WSL) — proves the kernel boots
sudo bash scripts/kernel/qemu-validate.sh ~/WSL2-Linux-Kernel/arch/x86/boot/bzImage "<release>" \
  ~/WSL2-Linux-Kernel/drivers/block/ublk_drv.ko ~/WSL2-Linux-Kernel/mm/zsmalloc.ko \
  ~/WSL2-Linux-Kernel/drivers/block/zram/zram.ko
#    → "QEMU-VALIDATE: PASS" = booted to userspace. Proceed only if it passes.

# 3. Copy the bzImage to Windows
mkdir -p /mnt/c/wsl && cp ~/WSL2-Linux-Kernel/arch/x86/boot/bzImage /mnt/c/wsl/kernel-ramshared
cp scripts/kernel/boot-kernel-safe.ps1 /mnt/c/wsl/   # self-healing launcher
cp scripts/kernel/boot-kernel-logged.ps1 /mnt/c/wsl/ # wrapper with persistent log
```

```powershell
# 4a. SAFE PREFLIGHT (in Windows PowerShell; DOES NOT end WSL):
powershell -ExecutionPolicy Bypass -File C:\wsl\boot-kernel-logged.ps1 -PreflightOnly
#    → validates the kernel, clean backup, .wslconfig, and arm/disarm in a temporary file.
#    The log is at C:\wsl\boot-ramshared.log.

# 4b. SAFE SWITCH + AUTO-REVERT (ends WSL):
powershell -ExecutionPolicy Bypass -File C:\wsl\boot-kernel-logged.ps1
#    → backs up .wslconfig, arms it, runs wsl --shutdown, and checks boot (timeout).
#    If it DOES NOT boot: RESTORES .wslconfig and restarts → returns to the Microsoft kernel.
#    The log is at C:\wsl\boot-ramshared.log.
#    Test the arming logic without touching WSL: ... -DryRunConfig C:\wsl\test.txt
```

Auto-revert prerequisite: a **clean** `.wslconfig` (without `kernel=`) at
`C:\wsl\wslconfig-original.txt` (the launcher creates it the first time when
the current file does not yet contain `kernel=`).

## 0. Prerequisites (in WSL2)

```sh
# Kernel build dependencies (missing: flex, bison, libelf-dev).
sudo apt-get update
sudo apt-get install -y build-essential flex bison libelf-dev libssl-dev bc dwarves \
  python3 pahole cpio
```

## 1. Kernel source (tag = version in use)

```sh
uname -r   # for example: 6.6.114.1-microsoft-standard-WSL2 → use tag linux-msft-wsl-6.6.y
cd ~
git clone --depth 1 --branch linux-msft-wsl-6.6.y \
  https://github.com/microsoft/WSL2-Linux-Kernel.git
cd WSL2-Linux-Kernel
```

## 2. Configuration: Microsoft base + the two Phase B CONFIGs

```sh
# Base = the official WSL2 configuration (already in the repository at Microsoft/config-wsl).
cp Microsoft/config-wsl .config
# Enable the Phase B gatekeepers:
./scripts/config --file .config --enable  CONFIG_ZRAM_WRITEBACK   # BOOL (depends on CONFIG_ZRAM=m), item 4
./scripts/config --file .config --module  CONFIG_BLK_DEV_UBLK      # ublk_drv (item 5)
./scripts/config --file .config --enable  CONFIG_IO_URING          # already =y; ensures it
make olddefconfig
# Confirm:
grep -E "CONFIG_ZRAM_WRITEBACK|CONFIG_BLK_DEV_UBLK|CONFIG_IO_URING" .config
```

## 3. Build (heavy — use caution in WSL2)

```sh
# A limited -j avoids freezing WSL2 (MEMORY rule: heavy builds can freeze it).
make -j"$(($(nproc)/2))" 2>&1 | tee /tmp/kbuild.log
# Output: ./arch/x86/boot/bzImage
ls -la arch/x86/boot/bzImage
# Installs modules (ublk_drv.ko, zram.ko with writeback) in /lib/modules/<release>/
# — the booted kernel looks for the .ko files there. REQUIRED for ublk/zram to load.
sudo make modules_install
```

## 4. Install (Windows side)

```sh
# Copy the bzImage to a Windows path (for example, C:\wsl\kernel-ramshared).
mkdir -p /mnt/c/wsl
cp arch/x86/boot/bzImage /mnt/c/wsl/kernel-ramshared
```

On **Windows**, `%UserProfile%\.wslconfig`:

```ini
[wsl2]
kernel=C:\\wsl\\kernel-ramshared
```

## 5. Boot (ends the agent session)

```powershell
# In Windows PowerShell/CMD:
wsl --shutdown
# Reopen WSL.
```

## 6. Post-boot verification (new session)

```sh
uname -r                                   # must reflect the new kernel
zcat /proc/config.gz | grep -E "ZRAM_WRITEBACK|BLK_DEV_UBLK"   # both m/y
sudo modprobe ublk_drv && ls /dev/ublk-control   # item 5 available
# zram writeback: backing_dev starts to exist after zram is allocated.
```

## 7. Then: Phase B Step 3 (SSDV3)

- **Item 4** (zram-writeback-VRAM topic; cascade SPEC:
  [`wsl2-cascade-swap`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md)): the
  current recommendation is **NOT** to implement userspace backing
  (reentrancy under reclaim + DEMOTE without draining) — prefer a kernel-side
  VRAM block device **or** keep the two-tier cascade. Reopen the SPEC if the
  kernel-side path is pursued. It is **not** enough for the kernel to have the
  configuration; the safe design requires the kernel-side driver.
- **Item 5** ([`ADR-0004`](../decisions/ADR-0004-ublk-io-uring-crate.md)):
  implement the ublk server by reusing worker H1; use the `io-uring` crate
  (ADR-0004); use generic `--swap-dev`; **benchmark ublk latency versus NBD**
  (adoption gate — without a gain, keep NBD).

## Rollback

- Remove the `kernel=` line from `.wslconfig` + run `wsl --shutdown` → return
  to the Microsoft prebuilt kernel. Application-only; no data is touched (the
  Day-0 two-tier cascade continues to work on the standard kernel).
