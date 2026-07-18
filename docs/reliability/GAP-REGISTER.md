# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| External GPU workload WDDM pressure | PARTIAL | No real external GPU workload file/window was available in the current session. Aggregate samplers and gates exist, but parser/static success is not live pressure proof. | `scripts/p0/Invoke-GpuWorkloadGate.ps1` PASS with idle/load/recovery output showing `pressure_observed=true` and `recovered_near_idle=true`, plus matching `ramshared diagnose --events` or daemon telemetry showing DEMOTE/recovery without corruption. |
| WSL2 freeze-elimination campaign | PARTIAL | The daily WSL2 desktop must not be thrashed. QEMU/ublk and broker drills are green, but they do not prove repeated live WSL2 desktop freeze elimination under host pressure. | Two isolated-lab `before -> action -> after` runs with watchdog timeout, binary match, swapoff-first proof, no ghost swap, no hung task/D-state evidence, and clean terminal state. |
| Windows physical product Online | PARTIAL | Physical daily host Online remains policy-bound unless an approved lab/host binary match and signing state are present. Guest and static gates are useful but do not prove physical Online. | Approved physical/lab run with `BINARY_MATCH`, three fresh SHA rounds, no bugcheck/dump, correlated `LeaseRelease`, CUDA free restoration, and terminal cleanup. |
| GPU-PV CUDA inside `win11-drill` | PARTIAL | Previous records show guest GPU-PV/CUDA was environment-bound. Host CUDA tests pass; guest CUDA execution is a separate claim. | Guest-side CUDA execution proof under `win11-drill`, with driver/device healthy and matching package evidence. |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are green, but full `ramshared up` ublk wire-up on non-WSL2 is intentionally not the current product surface. | Dedicated custom-kernel lab SPEC with full up/down wire-up, swapoff-first teardown, crash/drain drills, and terminal no-ghost proof. |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| App-specific GPU workload naming | Public tracked scan for specific example application names and old render-specific terms is clean. Generic names are `gpu_workload`, `dcc`, `host_agent`, `vram_reclaim`, and `gpu_budget`. |
| Historical signing password literal | `validation.md` now references `RAMSHARED_TESTSIGN_PFX_PASSWORD`; `tools/ci/check-public-hygiene.mjs` blocks the old literal and inline signing-password patterns. |
| P1 broker isolated drill freshness | `scripts/kernel/qemu-broker-drill.sh` PASS with `KTEST-DAEMON-BINARY-MATCH=ok`, `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`, `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`, and `KTEST-DAEMON-TERMINATED=ok`. |
| Linux/WSL2 installable bundle | `scripts/package/build-linux-bundle.sh` builds release binaries, stages safety scripts/docs, and emits a validating `SHA256SUMS` manifest. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
