const body = `## Resumo\n\nRefactored ...\n\n## Commits\n\n| Commit |\n\n## Issue\n\nCloses #1\n\n## Responsavel\n\n@jules\n\n## Labels\n\ntype:refactor area:core\n\n## Validacao\n\n- [x] Gates\n\n## Rollback trigger\n\nDeadlock`;

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
