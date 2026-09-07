## Resumo
Identified an architectural scope trap regarding the hung thread watchdog detection task. The `watchdog.rs` module strictly requires "Pure time arithmetic, no threads", rendering thread monitoring an invalid change. Generated a `FINDING_ONLY` report and updated documentation registries accordingly.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| e4f31b778e4fd9c3067b918d49e2d19f4417d3cd | (remote commit) | CI/CD | Previous commit on remote |
| 156355c4427b6e00321ebc68c7de9288dbf72e09 | (remote commit) | CI/CD | Base branch commit |
| b6b8e6dc5789f3744777a34d2347f6cff326e4e1 | Added FINDING_ONLY report and updated doc registries | To document the architectural scope trap | No code changes to `watchdog.rs` due to pure timer invariant. |

## Issue
N/A

## Responsavel
@jules

## Labels
type:resilience, area:core

## Validacao
`umask 0022 && cargo test && ./scripts/docs-check.sh`

## Rollback trigger
Never, purely documentary.
