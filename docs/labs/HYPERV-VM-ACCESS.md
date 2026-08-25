# Hyper-V lab evidence register

## Current boundary — disabled staging only

This is a sanitized historical evidence register, not an access runbook. The
current candidate authorizes no VM lifecycle, guest login, guest repair,
storage/VHDX action, GPU/device action, formatting, pressure campaign, WSL
operation, or host action. Lab names, guest principals, host paths, addresses,
device nodes, VM identifiers, and artifact locations are intentionally replaced
with context-preserving `SANITIZED_*` placeholders.

Historical measurements and verdicts below remain evidence for their exact
recorded builds only. They do not qualify the current candidate or authorize a
repeat. A future attended campaign needs a separately approved scope and its
own sealed identity record outside the public repository.

## Historical lab roles

| Sanitized surface | Historical role | Evidence boundary |
| --- | --- | --- |
| `SANITIZED_VM_DRIVER_LAB` | Isolated Windows StorPort/CUDA lab | PowerShell Direct results only |
| `SANITIZED_VM_WSL2_LAB` | Disposable Windows WSL2/freeze lab | Bounded readiness and campaign receipts |
| `SANITIZED_VM_KERNEL_LAB` | Generic Linux/kernel build lab | SSH/capability observations |

The inventory was deliberately closed: one driver lab, one Windows WSL2 lab,
and one kernel lab. No historical record permits a substitute daily host,
additional VM, clone, host disk, daily WSL storage, or foreign VHDX. The
Windows WSL2 lab's historical VM-owned disk is represented here as
`SANITIZED_VM_OWNED_VHDX`; it never identifies a public host path.

## Retained Windows driver/WSL evidence

> **Historical non-current / no execution:** The dated ledger below preserves
> command categories, outcomes, and measurements from closed lab records. It is
> not an access runbook and authorizes no VM, guest, storage, WSL, or pressure
> operation.

### 2026-07-18 — access and runtime diagnosis

- The driver lab's bounded PowerShell Direct probe initially returned
  `STATUS=PARTIAL` because its local credential was rejected. The local secret
  remained outside version control.
- Subsequent guest-only WSL runtime repair observations retained the official
  WSL `2.7.10` package/version category, `REGDB_E_CLASSNOTREG`, and a
  runtime-unavailable result. An earlier no-output highest-privilege probe
  recorded `last_task_result=267009`; the repaired bounded probe later recorded
  `last_task_result=0`, but neither result made the WSL runtime usable. A
  packaged-update observation also recorded guest DNS resolution failure.
- The single disposable Windows WSL2 lab began with no heartbeat or PowerShell
  Direct during its unattended boot window and ended in an inert state. No
  second VM, daily-host storage action, or pressure campaign was used.
- Historical media inspection found the required no-prompt EFI image and an
  answer file. A later installer-media result remained non-authoritative; the
  retained conclusion is that media preparation was insufficient by itself to
  prove guest readiness.

### 2026-07-18 and 2026-07-21 — disposable lab recovery attempts

- A same-lab storage relocation and vTPM setup were recorded. The guest runtime
  package could report a version while its status/list probes still exceeded a
  30-second deadline; the result remained `PARTIAL`.
- Follow-up repair observations found an absent runtime service registration.
  A temporary service experiment did not restore bounded runtime status/list
  success. The lab was returned to an inert state; the record expressly rejects
  treating that repair path as untried.
- The historical Linux kernel lab lacked the GPU and ublk device surfaces at
  first. After a guest-only module capability repair, SSH, non-interactive
  privilege, and the ublk capability were recorded as `PASS`. This was
  capability evidence only, never proof of WSL2 GPU reclaim or product
  lifecycle safety.

### 2026-08-20 — bounded recovery and readiness

- A plan-first readiness probe first exposed a cleanup defect after a failed
  PowerShell Direct attempt. The bounded fallback restored the exact disposable
  lab to its inert state without a forced power path; a later receipt retained
  `powershell_direct_auth_failed` and `restored_off_host_fallback`.
- An approved offline credential diagnosis was limited to the exact
  `SANITIZED_VM_OWNED_VHDX`. Its original account database was restored with an
  exact hash check. The blank-credential path was still rejected, so it did not
  produce guest WSL evidence.
- A reimage followed one byte-for-byte, SHA-256-verified backup of the
  VM-owned disk. It did not involve a host disk, daily WSL storage, a new VM,
  or a checkpoint. The lab-only persistent console-login setting is historical
  and must never be applied to a daily host.
- The resulting bounded readiness record observed terminal setup completion,
  interactive guest access, and WSL status/list completion with exit `0`.
  Verdict: `PASS / wsl_runtime_ready` for access/readiness only.

The readiness result closes neither guardian, origin, GPU/WDDM, freeze,
pressure, nor attended-rollout qualification. Those matrices remain
`PARTIAL`; the daily host is not a substitute surface.

## Detailed historical temporal ledger

### 2026-07-18 — driver-lab runtime diagnosis

**Category:** bounded PowerShell Direct, guest WSL runtime, and artifact review.

- A read-only guest-access probe returned `STATUS=PARTIAL` when the local
  credential was rejected. The credential source remained local-only and no
  secret was recorded.
- The dated guest-runtime repair category covered the official runtime package,
  Appx registration observation, status/list probes, and a bounded
  highest-privilege task probe. The task later recorded `last_task_result=0`,
  but the runtime still returned `REGDB_E_CLASSNOTREG`; that is a failed
  runtime-registration result, not a repair instruction.
- The current result category is `guest_wsl_runtime_unavailable`: WSL/VMP
  feature presence and a package version were insufficient when bounded status
  and list probes did not complete. This driver-lab result remains `PARTIAL`.

### 2026-07-18 — disposable WSL2-lab creation and media evidence

**Category:** disposable-VM creation, first-boot readiness, media inspection,
and VM-owned-disk evidence.

- The historical creation receipt recorded one disposable Windows VM, a dynamic
  VM-owned VHD, Windows and answer media, nested virtualization, and disabled
  checkpoints. Its first unattended boot had neither heartbeat nor PowerShell
  Direct, so it was returned to the inert state; no second VM was created.
- Media inspection recorded a no-prompt EFI image plus an answer file. A later
  remastered-media path reached a storage-driver prompt, so media preparation
  alone was explicitly classified as non-authoritative for readiness.
- The same-lab relocation receipt recorded an SSD-backed **VM-owned** disk,
  vTPM/local-key-protector evidence, and a four-GiB fixed-memory configuration.
  The VM configuration and host paths are intentionally not public. No host
  disk, daily WSL storage, foreign VHDX, clone, or checkpoint entered the
  historical scope.
- Runtime measurements remained bounded and partial: package status `Ok`,
  then status/list probes exceeding 30 seconds with empty stdout/stderr. A
  follow-up service-registration observation found the service absent; a
  temporary experiment did not restore the official registration. The VM was
  returned to `Off` after each closed attempt.

### 2026-07-18 — kernel-lab access and capability sequence

**Category:** Hyper-V network observation, SSH access, kernel capability, and
guest-only module observation.

- The primary adapter address field was empty; the dated neighbor-table fallback
  resolved `SANITIZED_IP_ADDRESS`. SSH, non-interactive privilege, and
  `cloud-init=done` were observed. The guest reported kernel
  `SANITIZED_KERNEL_RELEASE`.
- When the historical kernel-lab start reported insufficient host memory, its
  disabled-staging record used startup 2 GiB, minimum 1 GiB, and maximum 8 GiB
  as VM-only bounds. This is historical/no-execution evidence, not a setting to
  apply on any current host or VM.
- The first capability receipt was `STATUS=PARTIAL`: GPU device nodes,
  `ublk-control`, and the required module surface were absent. This explicitly
  excluded WSL2 GPU reclaim, GPU-PV reclaim, and product-transport claims.
- After a guest-only module-package repair category, the refreshed receipt was
  `STATUS=PASS` for SSH, passwordless privilege, `ublk_drv`, and the ublk
  control capability. It still did not establish lifecycle, ordered detach,
  crash/drain, or no-ghost product evidence.

### 2026-08-20 — bounded readiness, rollback, and reimage results

**Category:** plan-first VM readiness, fallback cleanup, offline credential
diagnosis, backup/reimage, and readiness readback.

- The first plan-first readiness receipt exposed a cleanup defect after a failed
  guest connection. Its bounded fallback produced
  `restored_off_host_fallback`; no forced power path, checkpoint, host-disk
  action, or pressure action was recorded.
- The bounded offline diagnosis retained three private hash checks: backup,
  repair copy-back, and original-account-database restoration. The blank
  credential remained rejected; it did not establish guest WSL readiness.
- A byte-for-byte SHA-256-verified VM-owned-disk backup preceded the reimage.
  The dated recovery receipt retained four GiB assigned memory, zero
  checkpoints, one VM-owned VHD, and a successful terminal setup/readiness
  result. The later WSL status/list probes exited `0` within their deadlines.
- This is an **access/readiness-only** `PASS / wsl_runtime_ready`. It does not
  promote driver, guardian, origin, GPU, freeze, pressure, or attended-rollout
  matrices, all of which remain `PARTIAL` or blocked.

## Retained kernel-lab capability evidence

On 2026-07-18, a neighbor-table fallback resolved a sanitized guest address
(`SANITIZED_IP_ADDRESS`) after the primary adapter observation was empty. The
guest reported a sanitized kernel release, successful SSH and non-interactive
privilege, and no GPU or ublk device surfaces. The first capability receipt was
`STATUS=PARTIAL`.

After the historical guest-only module repair, the ublk capability node became
available and the refreshed capability receipt was `STATUS=PASS`. That result
does not establish a product transport: lifecycle, swapoff-first detach,
crash/drain, and no-ghost evidence were still required.

## Evidence handling

- Secrets remain in local approved secret storage only; none is reproduced in
  source, logs, documentation, or chat.
- Public records retain semantic verdicts, deadlines, dates, hashes, and
  measured outcomes, but refer to `SANITIZED_ARTIFACT_REF` rather than a
  host-local artifact path.
- An absent or inaccessible lab is a `PARTIAL`/environment-bound result, never
  an invitation to repair, recreate, boot, stop, reimage, or pressure a host.
- A future approval must bind exact identities in a protected, private receipt
  and retain before/action/after evidence. This document supplies no command,
  approval token, identity, or activation route.
