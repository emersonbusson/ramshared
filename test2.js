const body = `## Issue
kernel-win/032

## Responsavel
KernelC100

## Resumo
Returns STATUS_BUFFER_TOO_SMALL for non-zero OutputBufferLength in METHOD_BUFFERED IOCTLs to prevent unintended memory leaks when data payload is not expected.

## Commits table
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| fix(kernel-win): return STATUS_BUFFER_TOO_SMALL | Added early guard clause returning STATUS_BUFFER_TOO_SMALL if OutputBufferLength != 0 for individual IOCTL cases | To prevent I/O Manager from allocating and copying back a SystemBuffer with uninitialized kernel memory | Avoids potential memory disclosure vulnerability in METHOD_BUFFERED requests. |

## Labels
type:security, type:kernel

## Validacao
Executed standard CI pipeline checks including ./scripts/ci/check-kernel-build-matrix.sh.

## Rollback trigger
Revert if user applications fail IOCTL communication unexpectedly due to providing a non-zero OutputBufferLength for these endpoints.
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
