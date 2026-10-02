# RamShared Operator Guide

This guide is the authoritative operations manual for installing, running, monitoring, and troubleshooting RamShared in production and development environments across Linux, WSL2, and Windows.

---

## 1. System Requirements & Hardware Support

| Component | Minimum Requirement | Recommended |
| :--- | :--- | :--- |
| **Operating System** | Linux Kernel ≥ 5.15 or WSL2 (Windows 10 Build 19044+ / Windows 11) | WSL2 on Windows 11 23H2+ or native Linux 6.x |
| **GPU / Acceleration** | A working CUDA provider or a Vulkan 1.1+ adapter with a transfer queue, stable identity, and fresh `VK_EXT_memory_budget` data | Use only an adapter whose exact driver and workload have passed the documented physical qualification |
| **Host System RAM** | 8 GiB physical DDR4/DDR5 | 16 GiB+ DDR5 |
| **Host Storage** | NVMe PCIe Gen3 SSD with at least 16 GiB free space | NVMe PCIe Gen4/Gen5 SSD |
| **Kernel Subsystems** | Standard WSL2: `nbd`; native Linux or compatible WSL2 custom kernel: `ublk`/`io_uring` | Use the transport qualified for the exact kernel surface |

> [!NOTE]
> In **GPU-less / headless mode**, the GPU cache target is zero. Whether the
> remaining ZRAM and SSD-origin topology can start depends on the preflight and
> configured transport; no uninterrupted-service guarantee is implied.
>
> Having VRAM alone does not make an adapter compatible. CUDA/Vulkan discovery
> is implemented, but physical multi-vendor cache qualification is still open;
> an unqualified or unmeasurable adapter must remain origin-only.

Standard WSL2 uses NBD as its baseline transport. `ublk`/`io_uring` is
qualified on native Linux or WSL2 with a compatible custom kernel.

---

## 2. Installation & Quick Start

### Building from Source

```bash
# Clone the repository
git clone https://github.com/emersonbusson/ramshared.git
cd ramshared

# Build release binaries
cargo build --release --workspace

# Install the operator CLI to your cargo bin directory (~/.cargo/bin)
cargo install --path crates/ramshared-cli
```

### Initial Pre-Flight Check

Before launching the cascade, run `ramshared diagnose` to verify host prerequisites, GPU detection, kernel drivers, and storage origin paths:

```bash
ramshared diagnose
```

A healthy pre-flight verification reports:
- GPU vendor, driver version, and available VRAM budget
- ZRAM device status (`/dev/zram0`)
- Authoritative origin swap partition or backing file
- Kernel block device backend (`ublk` or `nbd`)

---

## 3. Core Lifecycle Operations

RamShared provides declarative commands to control the tiered memory cascade:

### Starting the Cascade (`up`)

```bash
# Start the 3-tier cascade with automatic hardware detection
$ ramshared up

# Start with explicit tier sizes (MiB). This host's working example:
$ ramshared up --vram 4096 --zram 2048

# Flags: --vram <MiB>  VRAM/NBD logical capacity (prio 100)
#        --zram <MiB>  zram tier (prio 200); `--zram 0` skips zram
#        --daemon PATH daemon binary (default `ramsharedd`)
# Defaults are 1024 MiB each, or RAMSHARED_VRAM_MIB / RAMSHARED_ZRAM_MIB.
```

What happens on `ramshared up`:
1. Validates the surface-specific GPU headroom policy described below.
2. Formats or maps the authoritative SSD origin backing store.
3. Initializes NBD on standard WSL2, or `ublk` only on a qualified compatible-kernel surface, with block integrity checks.
4. Mounts the RamShared block device as intermediate priority swap in `/proc/swaps`.
5. Establishes the 3-tier cascade: **zram (prio 200) ➔ RamShared VRAM/SSD (prio 100) ➔ WSL fallback disk (prio −2)**. Higher priority is used first; the SSD origin remains the correctness boundary.

> **Windows Administrator token:** attaching the origin VHDX (`wsl.exe --mount --vhd`)
> requires a Windows Administrator token. Linux `sudo` does **not** elevate a
> Windows process. From WSL2, elevate with `Start-Process -Verb RunAs`, and take
> the origin approval token from the script's `PLAN` output (it is size-derived).
> Full recipe: [`runbooks/windows-elevation.md`](runbooks/windows-elevation.md).
> Normal `ramshared up` never runs `mkswap`; the sealed swap header is provisioned
> once by `scripts/safety/provision-origin-swap.sh`.

### Checking Operational Status (`status`)

```bash
$ ramshared status
```

Displays a compact summary:
- Overall status: `Armed`, `UsingZram`, `UsingVram`, `UsingDisk`, `Demoting`, `Degraded`, or `Off`
- Logical capacity vs. physical allocated VRAM cache
- Dynamic WDDM GPU headroom budget
- Throughput and IOPS metrics (Tier 1 vs. Tier 2 vs. Tier 3)

### Releasing VRAM Cache (`demote`)

When you plan to launch a heavy GPU application (e.g., local LLM inference, 3D render, gaming) and want to immediately yield all GPU cache memory back to the driver:

```bash
$ ramshared demote
```

`demote` requests release of clean cached chunks while the authoritative SSD origin remains the correctness boundary. Completion time and available headroom are reported rather than guaranteed.

### Reserve policies

- Broker/NBD capacity reserve: `max(1536 MiB, 20% of physical VRAM)`.
- Broker/NBD runtime free buffer: a separate `768 MiB` held back from reported
  free VRAM before admitting new allocations.
- Origin-cache reserve: `max(configured floor, 20% of measured capacity)`;
  production currently defaults the configured floor to `512 MiB` (clamped to
  `128–4096 MiB`) and keeps a separate `640 MiB` runtime buffer. The active
  qualification gate tracks the mismatch with the `1536 MiB` PRD/SPEC default.
- Windows StorPort reserve: `max(configured reserve, 512 MiB, 10%)`.

The capacity reserve limits the cache target. The runtime buffer protects a
new allocation against changing external GPU use and is not a fourth reserve
formula.

### Stopping the Cascade (`down`)

```bash
$ ramshared down
```

> [!IMPORTANT]
> **Swapoff-First Ordering:** `ramshared down` executes a strict, ordered teardown:
> 1. Removes the managed tier devices (`/dev/nbd0` on WSL2, `/dev/ublkN` on a
>    qualified kernel, `/dev/zram0`) from the active kernel swap table (`swapoff`).
> 2. Synchronizes pending disk blocks to the authoritative origin SSD.
> 3. Detaches the userspace block device and stops daemon threads.
> 4. Frees allocated GPU VRAM buffers cleanly.
>
> This order prevents kernel deadlocks, page fault panics, and BugCheck 0x7A crashes.

---

## 4. Real-Time Observability & Monitoring

### Interactive TUI Dashboard (`top`)

```bash
ramshared top
```

The interactive terminal dashboard displays:
- **Tier 1 (ZRAM):** Compressed memory usage, compression ratio (LZ4), CPU compression cost.
- **Tier 2 (VRAM Cache):** Active allocated chunks, hit rate, PCIe DMA transfer rate.
- **Tier 3 (SSD Origin):** Authoritative write-through IOPS, page faults served from NVMe.
- **GPU Telemetry:** Active WDDM budget, external 3D consumption, reserve headroom.

### Streaming JSON Telemetry (`monitor`)

For Prometheus exporters, external log shippers, or background daemon monitoring:

```bash
ramshared monitor --format json --interval 1s
```

---

## 5. WSL2 & Systemd Boot Autostart

To bring the multi-tier cascade up automatically whenever WSL2 or your Linux
workstation boots, enable the boot-path units. This is an opt-in lifecycle
decision (RF-1): a sealed install deliberately leaves them **disabled**, so an
installed product that is not yet set to autostart is expected behaviour and
not a fault.

### Prerequisites

Before enabling, confirm all of the following. If any is missing, the boot
gate will fail closed and the cascade will not start.

1. The installed release selector points at the release you intend to boot
   (the `current` symlink under the product root).
2. The sealed origin is attached and its config is present at
   `/etc/ramshared/origin.conf`.
3. The Windows guardian is publishing a fresh health proof (age under
   `stale_after_seconds`) and `safe-mode/` is empty. The boot gate consumes
   that proof and mints `/run/ramshared/host-resume-lease.json` from it.
4. `scripts/safety/install-cascade-boot.sh` has completed an attended install
   for this release version.

### Enabling Boot Integration

Enable the three boot-path units with `systemctl enable` for
`ramshared-host-gate.service`, `ramshared-cascade.service` and
`ramshared-supervisor.service`, run under `sudo`.

`systemctl enable` only links the units into `multi-user.target`. Nothing runs
until the next boot. Confirm that by checking `systemctl is-active` on the
gate and the cascade reports `inactive`, and that `systemctl show
ramshared-cascade.service -p ExecMainStartTimestamp` is empty.

What each unit does at boot:

| Unit | Role |
| --- | --- |
| `ramshared-host-gate.service` | Verifies the Windows guardian proof and mints the boot-bound host-resume lease. `Before=ramshared-cascade.service`. |
| `ramshared-cascade.service` | Runs `ramshared boot`, which walks the fail-closed gate chain (identity → approval → lease → dirty) and then activates the cascade. `Requires=ramshared-host-gate.service`. |
| `ramshared-supervisor.service` | Control-plane supervisor. |
| `ramshared-control.slice`, `ramshared-workloads.slice` | Resource slices that protect supervisor memory. Already `static`; nothing to enable. |

### Disabling Boot Integration

The revert is `systemctl disable` for those same three units. It takes effect
at the next boot; to stop a running cascade in the current boot use
`ramshared down`.

### Why `install-cascade-boot.sh --enable` is not the command

`scripts/safety/install-cascade-boot.sh` installs the units **disabled** on
purpose and refuses `--enable` with
`BOOT_ENABLE_REQUIRES_LIFECYCLE_APPROVAL`. It has no `--disable` flag at all.
The installer never touches unit enablement; that is a separate operator
decision, made with `systemctl` as described above. See
`docs/specs/no-milestone/wsl2-cascade-boot/SPEC.md` (RF-1) and
`validation.md` EVD-0167/EVD-0168.

### Fail-closed behaviour to expect at boot

`ramshared-host-gate.sh` deletes `/etc/ramshared/origin.conf` and the lease
**before** parsing any host-controlled data, then re-mints from a fresh proof.
If the Windows guardian proof is stale, the Windows mount is not ready, or the
identity does not match, it exits without minting and the cascade stays down.
That is deliberate: a failed or foreign proof must never retain authority
minted by an earlier invocation. Recover by making the guardian proof fresh,
then starting `ramshared-host-gate.service` followed by
`ramshared-cascade.service`.

---

## 6. Windows Native StorPort Miniport & Services

In native Windows environments (Track 2), RamShared operates as a virtual SCSI StorPort miniport driver:

- **Service Architecture:**
  - `RamSharedWinSvc`: Runs as a system service completing SCSI Request Blocks (SRBs) via page-locked host memory.
  - `RamSharedBroker`: Least-privilege SCM service arbitrating logical leases over local authenticated named pipes (`\\.\pipe\RamSharedBroker`).
- **Service Management via PowerShell (Administrator):**
  ```powershell
  # Check service health
  Get-Service -Name RamSharedBroker, RamSharedWinSvc

  # Start services
  Start-Service -Name RamSharedBroker
  Start-Service -Name RamSharedWinSvc

  # Stop services (enforces pagefile de-registration first)
  Stop-Service -Name RamSharedWinSvc
  Stop-Service -Name RamSharedBroker
  ```
- **Pagefile Safety Rule:** Never terminate or uninstall services while Windows has an active pagefile configured on the RamShared virtual disk (`X:\pagefile.sys`). Clear the pagefile in `SystemPropertiesPerformance.exe` before stopping the driver.

---

## 7. Storage Compaction & Space Recovery

Over extended development cycles, WSL2 virtual disk files (`ext4.vhdx`) and build caches may grow large. Use the safe, non-destructive space recovery workflow:

```bash
# Check reclaimable disk space across caches, targets, and logs
bash scripts/dev/reclaim-space.sh --dry-run

# Run safe cleanup (cleans cargo build target, old test artifacts, and truncates logs)
bash scripts/dev/reclaim-space.sh
```

For WSL2 VHDX virtual hard drive compaction on the Windows host:
```powershell
# Shutdown WSL instances cleanly
$ wsl.exe --shutdown

# Compact the virtual disk using diskpart
diskpart
# select vdisk file="<wsl-vhdx-path>\ext4.vhdx"
# compact vdisk
```

---

## 8. Troubleshooting & Emergency Recovery

### Issue: Abnormal Host Shutdown Left Device in Stale State

If the workstation experienced a power failure or sudden reboot while the cascade was active:

1. Verify swap status:
   ```bash
   $ cat /proc/swaps
   ```
2. If a managed tier device (`/dev/nbd0`, `/dev/ublkN`, `/dev/zram0`) is listed
   with `(deleted)` status, deactivate it immediately:
   ```bash
   $ sudo swapoff -a
   ```
3. Restart the cascade cleanly:
   ```bash
   $ ramshared up
   ```

### Issue: GPU Memory Allocation Rejected

If the cascade start reports `INSUFFICIENT_HEADROOM`:
- An external 3D game, AI model, or compute task is consuming the GPU budget.
- For broker/NBD, RamShared applies the capacity reserve plus runtime buffer described above. Close heavy GPU tasks or run with smaller tiers:
  ```bash
  $ ramshared up --vram 1024 --zram 1024
  ```

### Issue: Generating Support Diagnostics

To generate an anonymized diagnostic bundle for GitHub issues or internal reviews:

```bash
$ ramshared diagnose --bundle --output ./ramshared-diag.tar.gz
```
