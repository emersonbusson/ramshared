const body = "## Resumo\n\nRefactor `print_text_report` to improve maintainability. The excessive length of the `print_text_report` function in `crates/ramshared-cli/src/main.rs` has been addressed by extracting formatting and printing logic into three smaller, well-named helper functions: `print_overview`, `print_details`, and `print_issues`. Breaking them into smaller pieces significantly improves readability and maintainability, isolating each logical group.\n\n## Commits\n\n| Commit | O que fez | Por que fez | Detalhes |\n|---|---|---|---|\n| `HEAD` | Refactor `print_text_report` | Improve maintainability and readability | <details><summary>detalhes</summary>**Arquivos:** `crates/ramshared-cli/src/main.rs`<br>**Validacao:** cargo test, cargo fmt, cargo clippy<br>**Risco/rollback:** Baixo</details> |\n\n## Issue\n\nN/A\n\n## Responsavel\n\n@jules\n\n## Labels\n\ntype:refactor\narea:cli\n\n## Validacao\n\n- [x] Gates de build/test do escopo tocado\n- [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)\n- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)\n\n## Rollback trigger\n\nSe cargo test falhar no master ou não compilar";

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
