# Architecture Decision Records (ADRs)

Numbered, append-only decision records for RamShared architecture, memory,
driver, protocol, and governance choices. The canonical registry below is the
only source for a current numeric ADR reference.

## Record format

`ADR-NNNN-lowercase-slug.md` is required. New records use the
`governed-v1` profile and include: Status, Context, Decision, Consequences,
Alternatives considered, Kahneman, and Rollback trigger. A rollback trigger is
numeric or observable for a non-trivial decision.

Legacy records retain their original content and are not retrofitted solely to
match the newer profile. Their registry state is preserved, not inferred.

## Canonical index

| ADR | Filename | Title | Status | Governance profile |
| --- | --- | --- | --- | --- |
| 0001 | [ADR-0001-vram-cascade-tiering.md](ADR-0001-vram-cascade-tiering.md) | zram→VRAM→VHDX swap cascade (VRAM as a cold tier) | Accepted | legacy |
| 0002 | [ADR-0002-rust-userspace-port.md](ADR-0002-rust-userspace-port.md) | Rust userspace daemon/CLI | Accepted | legacy |
| 0003 | [ADR-0003-page-state-swap-safety.md](ADR-0003-page-state-swap-safety.md) | Page states and swap safety | Accepted | legacy |
| 0004 | [ADR-0004-ublk-io-uring-crate.md](ADR-0004-ublk-io-uring-crate.md) | ublk/io_uring crate | Accepted | legacy |
| 0005 | [ADR-0005-broker-protocol-jsonl.md](ADR-0005-broker-protocol-jsonl.md) | Broker JSONL protocol | Accepted | legacy |
| 0006 | [ADR-0006-storport-virtual-miniport.md](ADR-0006-storport-virtual-miniport.md) | StorPort virtual miniport | Accepted | legacy |
| 0007 | [ADR-0007-kernel-native-language-c.md](ADR-0007-kernel-native-language-c.md) | Kernel-native work in C | Accepted | legacy |
| 0008 | [ADR-0008-evidence-and-document-lifecycle.md](ADR-0008-evidence-and-document-lifecycle.md) | Evidence and document lifecycle | Accepted | governed-v1 |

## Historical filename collision

| Filename | Collides with canonical ADR | Registry disposition | Preservation note |
| --- | --- | --- | --- |
| [ADR-0007-windows-local-broker-ipc.md](ADR-0007-windows-local-broker-ipc.md) | 0007 | historical-noncanonical | Preserved filename collision; this record is historical and no new numeric ID may use this exception. |

The historical file remains available at its original path because existing
references may depend on it. Its filename is not canonical and must not be
used to create, rename, or justify another duplicate numeric ADR. Future ADRs
continue at 0009 or the next free canonical number.
