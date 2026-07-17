const body = `## Resumo
The testing gap in \`pagefile_may_target_volume\` edge cases was addressed. Added coverage for invalid volume letters, ambiguous/short paths, invalid drive characters, and correct case-insensitivity/whitespace handling. Improved test suite reliability by ensuring these parsing constraints fail closed properly and safely handle edge cases.

## Commits

| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| \`HEAD\` | Adicionado edge case tests para host safety pagefile targeting | A função pagefile_may_target_volume não tinha coverage adequada nas condições de erro e parsing | <details><summary>detalhes</summary>**Arquivos:** crates/ramshared-winsvc/src/host_safety.rs<br>**Validacao:** cargo test executado e validado com sucesso.<br>**Risco/rollback:** Risco zero, afeta apenas modulo de testes.</details> |

## Issue
N/A

## Responsavel
@emersonbusson

## Labels
type:test, area:winsvc

## Validacao
- [x] Gates de build/test do escopo tocado
- [ ] \`./scripts/docs-check.sh\` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger
N/A (test-only change)`;

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
