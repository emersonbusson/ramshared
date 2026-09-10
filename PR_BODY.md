## Resumo
Modified `drivers/block/ramshared/main.c` to enforce strict boundary clamping on `module_param` limits (`default_capacity_mb`, `max_devices`, `queue_depth`) at module initialization. Introduced an atomic counter (`ramshared_dev_count`) to track and enforce `max_devices` during `ramshared_pci_probe()`, correctly decrementing on error paths to prevent state leaks. Also updated RustSec advisory-db in CI test files to match the updated contract.

## Issue
N/A

## Responsavel
UpstreamPkg100

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| 5847784 | clamp module_param limits | Enforce fail-fast physical constraints and fix state leaks | Clamps `max_devices` and `default_capacity_mb` securely. Updated RustSec CI test files. |

## Labels
type:kernel
type:upstream
type:security
area:ramshared

## Validacao
```bash
cargo test -p ramshared-vram
node tools/ci/check-ci-contract.test.mjs
node tools/ci/check-ci-aggregate.test.mjs
```

## Rollback trigger
Revert if `ramshared` module initialization panics, probe fails on legal devices, or if CI governance validation unexpectedly fails.
