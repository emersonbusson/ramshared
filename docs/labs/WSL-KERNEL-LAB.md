# WSL kernel lab distro (`RamShared-Kernel`)

> Snapshot **2026-07-10**. Throwaway Ubuntu 24.04 for **break/rebuild kernel work** on the host WSL2 stack — **not** a minimal stripped kernel, and **not** the product default distro.

## Intent

| Question | Answer |
| --- | --- |
| “Keep only what we need?” | **Yes for repository diffs/configuration** (Kconfig extras, scripts, docs). **No** to a “minimal kernel without the WSL stack” — keep the full Microsoft WSL2 stack plus our deltas. |
| Product distro | **`Ubuntu-24.04`** (default) — day-1 cascade / daily use |
| Lab distro | **`RamShared-Kernel`** — passwordless, rebuild, break, re-import from backup |
| Fill C:? | **No** — live VHDX on **`<lab-drive>:`**, backup on **`<backup-drive>:`** |

## Layout (disk policy)

| What | Path | Disk |
| --- | --- | --- |
| Live rootfs (dynamic VHDX, cap 40 GB) | `<lab-drive>:\WSL\<lab-distro>\ext4.vhdx` | **`<lab-drive>:`** |
| Base backup after bootstrap | `<backup-drive>:\WSL-backup\<lab-distro>\base.tar` | **`<backup-drive>:`** |
| Product default WSL | `Ubuntu-24.04` | (existing install) |
| Host OS | `C:` | **Never** put this lab VHDX/backup here |

Sparse live size today ~2 GB used; grows only as packages/sources land **inside** the distro. Prefer kernel trees on secondary storage mounts when possible so the VHDX stays lean.

## Lab profile (inside distro)

- User default: `labuser` (`/etc/wsl.conf`)
- Local authentication: password login disabled with `passwd -d`; local lab only
- `sudo`: **NOPASSWD** (`/etc/sudoers.d/90-labuser-lab`)
- Marker: `/etc/ramshared/lab-profile`
- Build deps: `build-essential`, `libssl-dev`, `libelf-dev`, `bc`, `bison`, `flex`, `dwarves`, etc.

**Security:** this is a **local throwaway lab**, not a networked multi-user machine. Do not expose it.

## Daily commands

```powershell
# Enter lab (from Windows or fixed interop)
wsl -d RamShared-Kernel

# Product distro must stay default
wsl --set-default Ubuntu-24.04
wsl -l -v
```

```bash
# From product Ubuntu-24.04 (after WSLInterop is registered)
/mnt/c/Windows/System32/wsl.exe -d RamShared-Kernel -- whoami
/mnt/c/Windows/System32/wsl.exe -d RamShared-Kernel -- sudo -n id
```

## Backup / restore

```powershell
# Fresh base export (distro stopped recommended)
wsl -t RamShared-Kernel
wsl --export RamShared-Kernel <backup-drive>:\WSL-backup\<lab-distro>\base.tar

# Nuclear reset from backup (DESTROYS live lab rootfs — human only)
wsl --unregister RamShared-Kernel
mkdir <lab-drive>:\WSL\<lab-distro>
wsl --import RamShared-Kernel <lab-drive>:\WSL\<lab-distro> <backup-drive>:\WSL-backup\<lab-distro>\base.tar --version 2 --vhd
wsl --set-default Ubuntu-24.04
```

Helper (optional): `scripts/windows/Manage-WslKernelLab.ps1`.

## Interop note (custom host kernel)

On some custom WSL kernels, `binfmt_misc` **WSLInterop** fails to register at boot (`Permission denied` → `exec format error` for `wsl.exe`). Fix in the **running** product distro:

```bash
sudo sh -c 'echo ":WSLInterop:M::MZ::/init:PF" > /proc/sys/fs/binfmt_misc/register'
```

## Relation to Hyper-V isolated lab

| Surface | Use |
| --- | --- |
| `RamShared-Kernel` (this doc) | Host WSL2 same GPU-PV / same kernel binary path as product |
| Hyper-V isolated lab | Isolated VM build/break when you need full VM isolation |

Both keep artifacts **off C:**. See `LAB-DISK-GUARD.md`.
