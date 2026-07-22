# IMPL - External GPU workload WDDM pressure correlation

## Status

**PARTIAL.** The audit harness and static/synthetic gates exist. Live
closure still requires a real workload gate and matching daemon telemetry with
`demotes > 0`.

## Implemented

- `scripts/p0/Invoke-ExternalGpuWddmPressureAudit.ps1`
- `scripts/p0/Test-ExternalGpuWddmPressureAuditStatic.ps1`

## Validation

- Static: `pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/p0/Test-ExternalGpuWddmPressureAuditStatic.ps1`
- Synthetic PASS: fixture with `gate.ok=true` and daemon event `canario_demotes=1`.
- Synthetic PARTIAL: fixture with `gate.ok=true` and daemon event `canario_demotes=0`.
- Windows-host synthetic artifacts:
  - `C:\ramshared\artifacts\external-gpu-wddm-synthetic\pass\out`: `STATUS=PASS`,
    `GPU_GATE_OK=true`, `DIAGNOSE_OK=true`, `DEMOTES=1`.
  - `C:\ramshared\artifacts\external-gpu-wddm-synthetic\partial\out`: `STATUS=PARTIAL`,
    `GPU_GATE_OK=true`, `DIAGNOSE_OK=true`, `DEMOTES=0`, exit code 2.
