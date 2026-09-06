# RamShared Operator Guide

This guide is the authoritative operations manual for installing, running, monitoring, and troubleshooting RamShared in production and development environments across Linux, WSL2, and Windows.

---

## 1. System Requirements & Hardware Support

| Component | Minimum Requirement | Recommended |
| :--- | :--- | :--- |
| **Operating System** | Linux Kernel ≥ 5.15 or WSL2 (Windows 10 Build 19044+ / Windows 11) | WSL2 on Windows 11 23H2+ or native Linux 6.x |
| **GPU / Acceleration** | Any NVIDIA GPU (Pascal+) or AMD/Intel with Vulkan 1.2+ support | NVIDIA RTX 30/40/50 series with CUDA 12+ |
| **Host System RAM** | 8 GiB physical DDR4/DDR5 | 16 GiB+ DDR5 |
| **Host Storage** | NVMe PCIe Gen3 SSD with at least 16 GiB free space | NVMe PCIe Gen4/Gen5 SSD |
| **Kernel Subsystems** | `ublk` (`CONFIG_BLK_DEV_UBLK`), `io_uring`, or standard `nbd` | `ublk` with ZRAM enabled |

> [!NOTE]
> RamShared also operates in **GPU-less / headless mode**. If no compatible GPU is detected or if GPU headroom is fully consumed by external 3D workloads, RamShared safely cascades between compressed host RAM (ZRAM) and the SSD origin store without downtime or errors.

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
# Start dual-tier cascade with automatic hardware detection
$ ramshared up

# Start with a specific maximum cache limit (e.g., 4 GiB)
$ ramshared up --max-cache 4G
```

What happens on `ramshared up`:
1. Validates host GPU headroom, reserving `max(2 GiB, 20% total VRAM)` for host graphics.
2. Formats or maps the authoritative SSD origin backing store.
3. Initializes the userspace block device daemon (`ublk` or NBD) with SHA-256 block integrity checks.
4. Mounts the RamShared block device as intermediate priority swap in `/proc/swaps`.
5. Establishes the 3-tier cascade: Hot (ZRAM, pri 100) ➔ Accelerated (RamShared VRAM/SSD, pri 50) ➔ Fallback (Disk, pri -2).

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

`demote` frees all clean cached chunks across PCIe back to the GPU driver without dropping swapped pages; data remains safely persisted on the authoritative SSD origin.

### Stopping the Cascade (`down`)

```bash
$ ramshared down
```

> [!IMPORTANT]
> **Swapoff-First Ordering:** `ramshared down` executes a strict, ordered teardown:
> 1. Removes `/dev/ramshared0` from the active kernel swap table (`swapoff`).
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

To automatically configure the multi-tier cascade whenever WSL2 or your Linux workstation boots:

### Enabling Boot Integration

```bash
sudo scripts/safety/install-cascade-boot.sh --enable
```

This installs:
- `/etc/systemd/system/ramshared-cascade.service`: Manages the lifecycle of the block daemon and swap priority tables.
- Resource slice controls (`ramshared-control.slice` and `ramshared-workloads.slice`) to protect supervisor memory.

### Disabling Boot Integration

```bash
sudo scripts/safety/install-cascade-boot.sh --disable
```

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
2. If `/dev/ramshared0` or an orphaned NBD device is listed with `(deleted)` status, deactivate it immediately:
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
- RamShared automatically reserves `max(2 GiB, 20% VRAM)`. Close heavy GPU tasks or run with a smaller cache:
  ```bash
  $ ramshared up --max-cache 1G
  ```

### Issue: Generating Support Diagnostics

To generate an anonymized diagnostic bundle for GitHub issues or internal reviews:

```bash
$ ramshared diagnose --bundle --output ./ramshared-diag.tar.gz
```
