# SPEC - External GPU workload WDDM pressure correlation

## Closed Scope

In now:

- Plan-only/default audit harness.
- Audit of existing `gate.json` plus daemon JSONL events or pre-rendered
  `ramshared diagnose --events --json` output.
- Optional delegated GPU gate launch.

Out now:

- Process attribution.
- Forcing WSL2 pressure on the daily host.
- Treating aggregate VRAM pressure as DEMOTE evidence.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-1, ITEM-2 |
| RF-2 | ITEM-1 |
| RF-3 | ITEM-2 |
| NFR-1 | ITEM-1 |

## Technical Decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | PASS requires both GPU gate OK and `demotes > 0` from `ramshared diagnose`. | Prevents false closure from pressure-only evidence. |
| DT-2 | Missing events or zero demotes is `PARTIAL`, not failure of the pressure gate. | Environment may lack live daemon/cascade; claim remains open. |
| DT-3 | The audit never names external applications. | Product behavior is aggregate WDDM/CUDA pressure. |

## Atomicity And Rollback

- Atomicity frontier: artifact files only.
- Kernel/driver: unchanged.
- Host/persistent: no disk or swap mutation by this audit.
- Forward-only: none.

## Kahneman Map

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 pass gate | #13 | Can pressure-only evidence pass? | Static test requires `DEMOTES` and `demotes -gt 0`. | Any path that sets PASS without diagnose. |
| ITEM-2 missing telemetry | #9 | Is the partial state numeric and explicit? | Synthetic no-demote fixture exits 2 with `STATUS=PARTIAL`. | Missing event stream exits 0. |

## Files To Create / Modify

**CREATE — `scripts/p0/Invoke-ExternalGpuWddmPressureAudit.ps1`**

- Purpose: combine app-agnostic GPU workload pressure evidence with daemon
  telemetry diagnosis.
- Required tests: `scripts/p0/Test-ExternalGpuWddmPressureAuditStatic.ps1`.
- Cover target: N/A — PowerShell audit harness.

**CREATE — `scripts/p0/Test-ExternalGpuWddmPressureAuditStatic.ps1`**

- Purpose: verify safety, app-agnostic naming, and pass/partial guards.
- Required tests: itself.
- Cover target: N/A — static harness.

**MODIFY — `docs/reliability/GAP-REGISTER.md`**

- Record the harness as the required close path.

## Implementation Order

1. ITEM-1: create audit harness.
2. ITEM-2: create static test.
3. ITEM-3: run synthetic PASS/PARTIAL fixtures.
4. ITEM-4: update docs without claiming live closure.
