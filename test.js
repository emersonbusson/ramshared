const body = `## Resumo
Adicionado o comando build
## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| feat | Adicionou a função cmd_build | ... | ... |
## Issue
OpsInfra-100
## Responsavel
OpsInfra100/2026-09-01/packaging/089
## Labels
type:scripts, type:packaging
## Validacao
Executado bash
## Rollback trigger
Falhas sintáticas`;

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

console.log('Missing:', missing);
