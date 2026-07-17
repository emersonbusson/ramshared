const body = `## Resumo

Adiciona um teste unitario para a funcao heartbeat_psi.

## Commits

| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| \`hash\` | Add heartbeat_psi test | Improve coverage | <details><summary>detalhes</summary>**Arquivos:** broker_tenant.rs<br>**Validacao:** cargo test<br>**Risco/rollback:** nulo</details> |

## Issue

Closes #000

## Responsavel

@jules

## Labels

type:test, area:winsvc

## Validacao

- [x] Gates de build/test do escopo tocado
- [ ] \`./scripts/docs-check.sh\` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger

Falha nos testes.`;

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

console.log("Missing:", missing);
