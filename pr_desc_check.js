const body = `## Resumo

Added a linear guard clause to \`CtlDispatchDeviceControl\` in \`drivers/windows/ramshared/control.c\` that caps the maximum administrative IOCTL payload length at 4 MiB (4 * 1024 * 1024 bytes). This prevents potential kernel pool exhaustion (and subsequent system Bug Checks) caused by maliciously large \`METHOD_BUFFERED\` IRP payloads. Exceeding requests are early-returned with \`STATUS_INVALID_PARAMETER\`.

## Commits

| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| feat(security): cap admin IOCTL payload at 4 MiB | Added 4 MiB payload size check in \`CtlDispatchDeviceControl\` | Prevent pool exhaustion from massive buffered IRP payloads | <details><summary>detalhes</summary>**Arquivos:** drivers/windows/ramshared/control.c<br>**Validacao:** CI scripts<br>**Risco/rollback:** Revert if blocked</details> |

## Issue
Closes #097

## Responsavel
@jules

## Labels
type:security, type:kernel

## Validacao
- [x] Gates de build/test do escopo tocado
- [x] \`./scripts/docs-check.sh\`
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A)

## Rollback trigger
Revert this change if valid, authorized administrative IOCTLs legitimately require payloads exceeding 4 MiB and are being incorrectly rejected.`;

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
