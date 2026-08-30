## Resumo
Add physical limits sanity check to `create_secondary` pagefile allocation. It validates that the requested pagefile allocation size does not exceed 90% of the available physical volume free space using the `GetDiskFreeSpaceExW` API.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| feat: bounds check pagefile allocation | Added volume free space check | To enforce "Physical Limits Sanity Checks" architectural principle | Rejects sizes >90% of free space using `GetDiskFreeSpaceExW`. |

## Issue
Closes ITEM-037

## Responsavel
CleanArch100/2026-08-30

## Labels
type:refactor, type:security, type:test

## Validacao
cargo test -p ramshared-winsvc && cargo clippy -p ramshared-winsvc -- -D warnings

## Rollback trigger
Revert if valid pagefile creation requests are incorrectly blocked by the bounds logic.
