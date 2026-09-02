const fs = require('fs');
const body = fs.readFileSync('/tmp/plan_pr.md', 'utf8');
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

if (missing.length > 0) {
  console.log(`PR description is missing required sections: ${missing.join(', ')}. Please check .github/pull_request_template.md.`);
} else {
  console.log('All required PR description sections are present.');
}
