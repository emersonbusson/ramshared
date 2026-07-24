# SPEC - VRAM reclaim pressure matrix

> Passo 2 SSDV3. Implements [`PRD.md`](PRD.md).

## Scope

In now:

- App-agnostic matrix runner.
- Plan-only default.
- Windows smoke and Windows 3 GiB live cases through the existing host exhaustive
  harness.
- Supervised WSL2 1 GiB and 4 GiB reclaim cases through the Windows watchdog
  harness.
- Calibrated split-owner case with staged external pressure.
- Machine-readable `matrix-summary.json` for PASS/PARTIAL/FAIL per case.

Out now:

- Direct or unsupervised WSL2 pressure on the daily desktop.
- Process attribution to any named external application.

## Decisions

| ID | Decision | Why |
| --- | --- | --- |
| DT-1 | `-Run` requires `-ApprovePhysicalHost`. | Avoid accidental physical host mutation. |
| DT-2 | Daily-host WSL2 pressure requires the approved Windows watchdog harness; missing approval becomes `PARTIAL`. | A WSL-side hang must have an external termination and telemetry owner. |
| DT-3 | Insufficient VRAM headroom becomes `PARTIAL` before creating a LUN. | A safe refusal is valid evidence, not corruption. |
| DT-4 | Split uses 1 GiB Windows + 3 GiB WSL2, then a staged 1 GiB external workload. | Both owners fit with a 256 MiB setup margin on the 6 GiB GPU; reclaim, rather than impossible simultaneous reservation, is tested. |
| DT-5 | WSL2 artifact closure requires `integrity-result.json` per round. | A killed pressure worker is not proof that swapped data survived reclaim. |
| DT-6 | Preflight uses owner allocations plus a 256 MiB setup margin; reserve is evaluated after staged pressure. | Reserve is an invariant to restore, not a third resident owner. |

## Validation

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/p0/Test-VramReclaimPressureMatrixStatic.ps1
```

Live runs must be explicitly approved:

```powershell
pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/p0/Invoke-VramReclaimPressureMatrix.ps1 -Run -ApprovePhysicalHost -Case windows-3gib
```

Rollback trigger: revert if WSL2 or split-owner gaps can close without
before/action/after evidence, per-round checksum integrity, DEMOTE/teardown proof, and a
clean terminal state.
