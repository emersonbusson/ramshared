# SPEC - VRAM reclaim pressure matrix

> Passo 2 SSDV3. Implements [`PRD.md`](PRD.md).

## Scope

In now:

- App-agnostic matrix runner.
- Plan-only default.
- Windows smoke and Windows 3 GiB live cases through the existing host exhaustive
  harness.
- Machine-readable `matrix-summary.json` for PASS/PARTIAL/FAIL per case.

Out now:

- WSL2 pressure on the daily desktop without explicit approval.
- Split-owner orchestration until Windows large-LUN and WSL2 cleanup evidence are
  both green.
- Process attribution to any named external application.

## Decisions

| ID | Decision | Why |
| --- | --- | --- |
| DT-1 | `-Run` requires `-ApprovePhysicalHost`. | Avoid accidental physical host mutation. |
| DT-2 | WSL2 cases without `-ApproveSharedDesktopWsl` become `PARTIAL`, not raw errors. | Preserve artifact evidence while protecting the daily WSL2 host. |
| DT-3 | Insufficient VRAM headroom becomes `PARTIAL` before creating a LUN. | A safe refusal is valid evidence, not corruption. |
| DT-4 | Split-owner remains `PARTIAL` until a dedicated orchestrator exists. | Avoid half-running two owners without teardown proof. |

## Validation

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/p0/Test-VramReclaimPressureMatrixStatic.ps1
```

Live runs must be explicitly approved:

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/p0/Invoke-VramReclaimPressureMatrix.ps1 -Run -ApprovePhysicalHost -Case windows-3gib
```

Rollback trigger: revert if WSL2 or split-owner gaps can close without
before/action/after evidence, checksum integrity, DEMOTE/teardown proof, and a
clean terminal state.
