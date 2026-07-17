const body = `
## Resumo
Cache /proc/swaps read.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| \`perf-swapoff\` | Cache /proc/swaps in swapoff_all | Prevent I/O blocking | <details><summary>detalhes</summary>Updated swapoff_all</details> |

## Issue
N/A

## Responsavel
@agent

## Labels
type:perf

## Validacao
Ran cargo test

## Rollback trigger
Stall > 1ms
`;

const requiredSections = [
  { name: 'Resumo', pattern: /^##\s+Resumo\s*$/im },
  { name: 'Commits', pattern: /^##\s+Commits\s*$/im },
  { name: 'Issue', pattern: /^##\s+Issue\s*$/im },
  { name: 'Responsavel', pattern: /^##\s+Responsavel\s*$/im },
  { name: 'Labels', pattern: /^##\s+Labels\s*$/im },
  { name: 'Validacao', pattern: /^##\s+Validacao\s*$/im },
  { name: 'Rollback trigger', pattern: /^##\s+Rollback trigger\s*$/im }
];

const missing = requiredSections
  .filter(section => !section.pattern.test(body))
  .map(section => section.name);

console.log(missing);
