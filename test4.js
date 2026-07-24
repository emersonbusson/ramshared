const body = "## Resumo\n\nTranslate Portuguese comments in `scripts/safety/postmortem.sh` to English to resolve a false positive where TODO scanners flagged the Portuguese word \"todo\" (every). This also aligns the script with the project's strict English-only policy for new code/comments.\n\n## Commits\n\n| Commit | O que fez | Por que fez | Detalhes |\n|---|---|---|---|\n| `HEAD` | Traduziu bloco de comentários em `scripts/safety/postmortem.sh` | Falso positivo no scanner de TODO devido à palavra 'todo' (em português: 'todo/qualquer'). | <details><summary>detalhes</summary>**Arquivos:** `scripts/safety/postmortem.sh`<br>**Validacao:** `bash -n scripts/safety/postmortem.sh`<br>**Risco/rollback:** Nenhum, apenas alteração de comentário.</details> |\n\n## Issue\n\nN/A (Automated Task)\n\n## Responsavel\n\n@jules\n\n## Labels\n\ntype:fix, area:scripts\n\n## Validacao\n\n- [x] Gates de build/test do escopo tocado (`bash -n scripts/safety/postmortem.sh`)\n- [ ] `./scripts/docs-check.sh` (N/A)\n- [ ] SSDV3 (N/A)\n\n## Rollback trigger\n\nReverter se a tradução for imprecisa e confundir a equipe.";

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
