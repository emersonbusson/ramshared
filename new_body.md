## Resumo
Refactored VdTranslateSrb and VdTranslateSrbNoDisk to use early guard clauses instead of deep nested switch statements. Validates CDB length to prevent out-of-bounds accesses and safely rejects unsupported operation codes upfront, keeping the main I/O happy path flat at root indentation level.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| refactor(kernel-win): guard clauses for SCSI SRB command block validation in virtdisk | Replaced nested switch logic with linear early returns in virtdisk.c | Ensure safe CDB processing and maintain clean C architecture with root-level flat happy path | <details><summary>detalhes</summary>**Arquivos:** drivers/windows/ramshared/virtdisk.c<br>**Validacao:** script checks<br>**Risco/rollback:** regression in valid cdb parsing.</details> |

## Issue
Fixes #0

## Responsavel
@jules

## Labels
type:refactor, type:kernel-win

## Validacao
Executed CI validation scripts: docs-check, kernel style, build matrix, sparse, checkpatch, abi guard, qemu smoke, and adversarial invariants.

## Rollback trigger
Revert if the kernel driver exhibits unexpected parsing errors for valid SCSI CDBs or Windows HLK tests fail.
