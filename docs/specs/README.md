# RamShared System Specifications Index

This catalog organizes all 38 system architecture and design specifications in RamShared. Each specification follows the **SSDV3** methodology (PRD ➔ SPEC ➔ IMPL ➔ Validation) and defines formal contracts, operational invariants, and testable gates.

---

## 1. Active Core Production Specifications

These specifications govern the active runtime architecture of RamShared across Linux, WSL2, and host services.

| Specification | Primary Scope & Architectural Responsibility |
| :--- | :--- |
| [`cascade-vram-ondemand`](no-milestone/cascade-vram-ondemand/) | Dynamic VRAM allocation, on-demand memory chunking, and PCIe DMA transport. |
| [`wsl2-revocable-vram-origin`](no-milestone/wsl2-revocable-vram-origin/) | SSD-authoritative origin store with clean, revocable VRAM cache layer. |
| [`wsl2-cascade-swap`](no-milestone/wsl2-cascade-swap/) | Dual-tier virtual memory cascade orchestration across ZRAM, VRAM, and disk. |
| [`wsl2-cascade-boot`](no-milestone/wsl2-cascade-boot/) | Systemd service integration, boot orchestration, and slice containment. |
| [`wsl2-cascade-orphan-recover`](no-milestone/wsl2-cascade-orphan-recover/) | Automated detection and cleanup of ghost `(deleted)` swap devices and unsealed partitions. |
| [`cascade-lifecycle-observability`](no-milestone/cascade-lifecycle-observability/) | Schema v4 lifecycle states, telemetry aggregation, and health probes. |
| [`cascade-transport-policy`](no-milestone/cascade-transport-policy/) | Transport engine selection (`io_uring`, `ublk`, page-locked DMA, direct I/O). |
| [`custom-kernel-ublk-product-transport`](no-milestone/custom-kernel-ublk-product-transport/) | Linux `ublk` userspace block device driver implementation and performance tuning. |
| [`external-gpu-workload-wddm-pressure`](no-milestone/external-gpu-workload-wddm-pressure/) | Dynamic WDDM GPU headroom monitoring and automatic 3D graphics reservation. |
| [`vram-reclaim-pressure-matrix`](no-milestone/vram-reclaim-pressure-matrix/) | Host memory pressure hysteresis, proactive chunk eviction, and demotion state machine. |
| [`broker-telemetry-reconciliation`](no-milestone/broker-telemetry-reconciliation/) | IPC broker synchronization, lease state machine, and IPC integrity verification. |
| [`benchmark-evidence-integrity`](no-milestone/benchmark-evidence-integrity/) | Deterministic benchmark execution, SHA-256 evidence hashing, and metric qualification. |
| [`campaign-evidence-lifecycle`](no-milestone/campaign-evidence-lifecycle/) | Retention policy and gating rules for empirical validation records. |
| [`documentation-governance-integrity`](no-milestone/documentation-governance-integrity/) | Automated documentation lifecycle, freshness tracking, and link integrity verification. |
| [`documentation-localization-integrity`](no-milestone/documentation-localization-integrity/) | Multi-lingual synchronization between English primary docs and Portuguese translations. |
| [`ci-trust-and-release-integrity`](no-milestone/ci-trust-and-release-integrity/) | Continuous integration trust gates, reproducible packaging, and release promotion. |

---

## 2. Hardware & Platform Acceleration Research

Specifications in this track explore low-level driver development, kernel upstreaming, and Windows StorPort integration.

| Specification | Primary Scope & Architectural Responsibility |
| :--- | :--- |
| [`kernel-vram-as-memory`](no-milestone/kernel-vram-as-memory/) | Linux kernel NUMA node abstraction exposing GPU memory directly to the kernel allocator. |
| [`microsoft-native-vram-memory-tier`](no-milestone/microsoft-native-vram-memory-tier/) | Hyper-V virtual NUMA enlightenments and Windows host-guest memory sharing. |
| [`windows-storport-cuda-vram`](no-milestone/windows-storport-cuda-vram/) | Native Windows SCSI StorPort miniport virtual disk driver backed by CUDA DMA. |
| [`windows-autonomous-broker-service`](no-milestone/windows-autonomous-broker-service/) | Least-privilege Windows Service Control Manager (SCM) broker service. |
| [`windows-swap-driver`](no-milestone/windows-swap-driver/) | Native Windows virtual disk pagefile backing and crash-dump safety verification. |
| [`windows-task-manager-disk-counters`](no-milestone/windows-task-manager-disk-counters/) | Windows storage class driver compatibility and Task Manager I/O counter precision. |
| [`kernel-native-language`](no-milestone/kernel-native-language/) | Rust for Linux kernel module implementations for block devices. |
| [`wsl2-custom-kernel-p1`](no-milestone/wsl2-custom-kernel-p1/) | Custom WSL2 Linux kernel builds with `CONFIG_BLK_DEV_UBLK` and `io_uring` support. |
| [`wsl2-upstream-native-contribution`](no-milestone/wsl2-upstream-native-contribution/) | Preparation and qualification for Linux upstream kernel submission. |
| [`mainline-vram-tiering`](no-milestone/mainline-vram-tiering/) | Mainline Linux memory management (MM) subsystem integration and tiered tiering hooks. |
| [`wsl2-native-vram-autotier`](no-milestone/wsl2-native-vram-autotier/) | Autonomous physical memory migration between system DDR and GPU VRAM. |
| [`memory-broker`](no-milestone/memory-broker/) | Cross-process physical memory leasing and arbitration engine. |

---

## 3. Archived Historical Explorations & Postmortems

These specifications represent foundational research, early prototypes, and resolved incident postmortems. They are preserved for architectural context and regression prevention.

| Specification | Focus & Historical Outcome |
| :--- | :--- |
| [`wsl2-freeze`](no-milestone/wsl2-freeze/) | Root-cause analysis of early WSL2 synchronous pagefile deadlocks; established the swapoff-first invariant. |
| [`wsl2-freeze-elimination-campaign`](no-milestone/wsl2-freeze-elimination-campaign/) | Systematic campaign that replaced blocking I/O with asynchronous ublk and write-through origin persistence. |
| [`wsl2-control-plane-pressure-incident`](no-milestone/wsl2-control-plane-pressure-incident/) | Postmortem establishing cgroup v2 slice containment (`ramshared-control.slice`) to isolate supervisor memory. |
| [`wsl2-nbd-product-readiness`](no-milestone/wsl2-nbd-product-readiness/) | Evaluation of network block device (NBD) backend; superseded by modern `ublk`. |
| [`wsl2-native-vram-tier`](no-milestone/wsl2-native-vram-tier/) | Initial prototype exploring monolithic static VRAM allocation; superseded by dynamic chunk leasing. |
| [`wsl2-relay-lifecycle-reliability`](no-milestone/wsl2-relay-lifecycle-reliability/) | Early UNIX socket relay daemon investigation; replaced by direct kernel block protocols. |
| [`cascade-desktop-app`](no-milestone/cascade-desktop-app/) | Exploratory GUI application prototype; superseded by lightweight TUI (`ramshared top`) and CLI. |
| [`public-repository-hygiene`](no-milestone/public-repository-hygiene/) | Repository standardization and hygiene audit eliminating internal robot jargon from public docs. |
| [`release-promotion-publication`](no-milestone/release-promotion-publication/) | Early CI/CD release workflow experiments. |
| [`comment-language-integrity`](no-milestone/comment-language-integrity/) | Migration from Portuguese source code comments to standardized English. |
