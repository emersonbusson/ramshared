const body = `## Resumo
Added explicit guard clauses to scripts/safety/cascade-pressure-probe.sh to validate the cgroup v2 mount, memory controller delegation, and the PSI memory interface. These will fail-fast with semantic exit code 69 (EX_UNAVAILABLE) if prerequisites are not met. The old, less specific directory check was removed.

## Commits: table
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| feat: add explicit cgroup v2 and PSI guard clauses | Added checks for /sys/fs/cgroup/cgroup.controllers, memory inside controllers, and /proc/pressure/memory returning EX_UNAVAILABLE (69) | Prevent test failure or unpredictable behavior on environments lacking strict cgroup v2 features and PSI support | Replaced the old -d /sys/fs/cgroup directory check with specific validations before calling read_prios |

## Labels
type:scripts, type:security, type:observability

## Validacao
shellcheck cannot be run...
sudo bash scripts/safety/cascade-pressure-probe.sh || [ $? -eq 69 ]

## Rollback trigger
Revert if tests fail to run on legitimate environments that incorrectly appear to lack PSI or cgroup memory controller support.

Issue: 099
Responsavel: jules`;

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
