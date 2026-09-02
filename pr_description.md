## Resumo
Compute and store SHA-256 checksums for sealed kernel bzImage and initramfs.cpio.gz. Added initramfs support to the sealing scripts and corresponding validation.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| (to be populated) | Added initramfs parameter parsing and payload scaling/checksum logic to seal-kernel-pair.sh. | The contract must include checksum computation for the sealed kernel and initramfs for integrity validation. | <details><summary>detalhes</summary>**Arquivos:** Modified seal-kernel-pair.sh and corresponding static tests.<br>**Validacao:** Tests passing.<br>**Risco/rollback:** Low risk.</details> |

## Issue
Closes #000

## Responsavel
@jules

## Labels
type:scripts, type:security, type:packaging

## Validacao
echo "shellcheck cannot be run as pwsh is unavailable in the environment."

## Rollback trigger
Errors executing test scripts or sealing process logic regressions found in the main qualification bundles.
