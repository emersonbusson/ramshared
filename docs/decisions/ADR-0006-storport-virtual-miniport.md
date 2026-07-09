# ADR-0006 — StorPort virtual miniport + SPSC ring (Windows VRAM pagefile)

- **Status:** Accepted
- **Date:** 2026-07-09
- **SPEC:** [`docs/specs/no-milestone/windows-swap-driver/SPEC.md`](../specs/no-milestone/windows-swap-driver/SPEC.md)
- **PRD:** [`…/windows-swap-driver/PRD.md`](../specs/no-milestone/windows-swap-driver/PRD.md)

## Context

Linux/WSL2 already ships a **userspace VRAM block backend** (NBD/ublk path) used as a cold swap tier. Native Windows needs an equivalent **secondary pagefile** backing without application changes. Prior research (Passo 0 drill) showed secondary pagefiles on virtual disks are viable and that surprise-removal of user-paged data can be contained without host BSOD; **kernel-page** residency remains the residual hard risk (ITEM-8).

## Decision

1. **Write a StorPort virtual miniport from scratch (Day-0)** — no long-term ImDisk/WinSpd dependency in the product path.
2. **Userspace owns CUDA/VRAM** (`ramshared-winsvc`); the driver only translates SCSI/SRB ↔ shared-memory rings + bounce buffers.
3. **ABI = frozen C header** `drivers/windows/ramshared/protocol.h` mirrored in Rust (`proto.rs`) with size/golden tests before driver body lands.
4. **Wake path Day-0:** single `IOCTL_RAMSHARED_COMMIT_AND_FETCH` loop (DT-22); events are auxiliary only.
5. **Broker lease is logical budget only** (DT-20); Windows process does local `cuMemAlloc` after `cuMemGetInfo` fail-closed.
6. **Revocation is holder-cooperative** (DT-19); no new broker force-revoke message.

## Alternatives considered

| Option | Why rejected |
| --- | --- |
| Ship ImDisk forever | GPL/WDM legacy; violates Day-0; used only as Passo 0 instrument |
| Fork/revive WinSpd | Abandoned beta; third-party shim risk |
| Kernel-mode CUDA | Wrong layer; signing and stability nightmare |
| Pagefile as primary/boot | Structurally unavailable post-boot model |

## Consequences

- New tree `drivers/windows/` and crate `ramshared-winsvc`.
- WDK/SDV/InfVerif/Driver Verifier replace Linux `checkpatch` for the driver (DT-14).
- Host-real loads gated on ITEM-8 + degradation matrix update + attestation policy (R9).
- Linux workspace must stay green (RNF-8) when touching shared crates.

## Rollback

Remove Windows driver package and stop the service; pagefile config returns to C:-only. Revert shared-crate ITEMs (1–2) only with Linux drill re-validation.
