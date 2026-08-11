# Operations runbooks

This directory routes supported operator procedures. A runbook explains a
bounded procedure; it is not authority to change an unsafe host, lab, driver,
GPU, service, disk, swap, VM, or evidence record without the preconditions it
names.

| Objective | Canonical runbook | Boundary |
| --- | --- | --- |
| Recover workstation disk space | [WORKSTATION-SPACE-RECOVERY.md](WORKSTATION-SPACE-RECOVERY.md) | Inventory and explicit human approval precede any removal; historical receipts never authorize a new cleanup. |
| Operate the Windows autonomous broker | [windows-autonomous-broker.md](windows-autonomous-broker.md) | Physical-driver and storage controls remain supervised. |
| Run the Windows VRAM drive drill | [windows-vram-drive-drill.md](windows-vram-drive-drill.md) | Use only its declared lab and identity gates. |
| Review an architecture decision | [REVIEW-ADR.md](REVIEW-ADR.md) | The canonical decision registry is [`../decisions/README.md`](../decisions/README.md). |
| Follow the kernel phase-B procedure | [FASE-B-KERNEL.md](FASE-B-KERNEL.md) | Kernel work remains an explicit SSDV3/lab boundary. |

For retention and host-lab storage limits, also read
[`../labs/EVIDENCE-RETENTION.md`](../labs/EVIDENCE-RETENTION.md) and
[`../labs/LAB-DISK-GUARD.md`](../labs/LAB-DISK-GUARD.md).
