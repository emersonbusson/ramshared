---
slug: external-gpu-workload-wddm-pressure
title: External GPU workload WDDM pressure correlation
milestone: —
issues: []
---

# PRD - External GPU workload WDDM pressure correlation

## Summary

RamShared must prove external GPU pressure with aggregate VRAM measurements and
daemon telemetry correlation. A GPU workload gate alone proves pressure/recovery;
it does not prove that the Linux/WSL2 cascade observed the pressure and performed
DEMOTE safely.

## Technical Context

- Confirmed in codebase: `scripts/p0/Invoke-GpuWorkloadGate.ps1` records idle,
  loaded, and recovery aggregate VRAM windows and emits `gate.json`.
- Confirmed in codebase: `ramshared diagnose --events PATH --json` summarizes
  daemon JSONL telemetry and reports `demotes`.
- Confirmed in docs: `docs/reliability/GAP-REGISTER.md` keeps this gate PARTIAL
  until external pressure and daemon DEMOTE/recovery are correlated.
- Inference: Windows WDDM pressure cannot be attributed to a process by name unless
  telemetry explicitly records that attribution; the product should remain
  process-agnostic.

## Recommended Option

Add an audit harness that combines an existing or newly generated GPU workload
gate with a daemon event stream. The audit is plan-only by default. Live mode may
launch `Invoke-GpuWorkloadGate.ps1`, but the audit only passes when `gate.json`
is OK and `ramshared diagnose --events --json` output reports at least one DEMOTE.

Discarded alternatives:

- Accept aggregate VRAM pressure alone. Rejected: it does not prove RamShared
  lifecycle response.
- Encode example application names. Rejected: the contract is app-agnostic
  external GPU pressure.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Correlate aggregate GPU pressure with daemon telemetry. | `audit-summary.json` includes `GPU_GATE_OK=true`, `DIAGNOSE_OK=true`, and `DEMOTES>0`; diagnosis may be produced in-line from `EventsPath` or supplied as `DiagnoseJsonPath`. |
| RF-2 | Stay app-agnostic. | No example application names in script, docs, labels, or claims. |
| RF-3 | Avoid false PASS. | Missing `gate.json`, failed gate, missing events, diagnose failure, or zero demotes produces PARTIAL/exit 2, not PASS. |
| NFR-1 | Safe by default. | Default mode is plan-only; workload launch requires `-RunGpuGate`. |

## Validation Plan

- Static: `scripts/p0/Test-ExternalGpuWddmPressureAuditStatic.ps1`.
- Synthetic parser: local sample gate/events fixture through the audit script.
- Live close evidence: physical host or isolated WSL2 lab run with real
  `Invoke-GpuWorkloadGate.ps1` output and matching daemon telemetry JSONL.
