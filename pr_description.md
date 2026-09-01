## Resumo
Prevent integer overflow in IOCTL memory allocation size calculation by using checked math (`RtlULongLongAdd`, `RtlULongLongMult` from `ntintsafe.h`) for `queue_depth` scaling and offset sizing. This avoids potential memory corruption due to undersized allocations or out-of-bounds copies.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
| --- | --- | --- | --- |
| 1 | Prevent integer overflow in IOCTL memory allocation size calculation | Replace direct arithmetic with checked RtlULongLongAdd and RtlULongLongMult from ntintsafe.h to avoid potential overflows when processing untrusted IOCTL inputs for queue_depth and buffer offset calculations. | <details><summary>detalhes</summary>Added `#include <ntintsafe.h>` in `control.c` and `queue.c`. Used `RtlULongLongMult` and `RtlULongLongAdd` to compute `sq_bytes`, `cq_bytes` and validate `data_area_len` in `QRegister`. Computed buffer copy offsets dynamically using `RtlULongLongMult` and verified success before running `RtlCopyMemory`.</details> |

## Labels
type:security, type:kernel

## Validacao
- Compiled cleanly (0 warnings) and checked with `check-kernel-sparse.sh` and `check-kernel-checkpatch.sh`.
- Passed CI verification: `check-adversarial-invariants.sh` (0 trailing whitespaces).
- Ran cargo tests successfully.

## Rollback trigger
Revert if kernel drivers experience compilation issues on the target CI architecture or regressions in tests.
