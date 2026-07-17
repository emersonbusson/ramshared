const body = `## Resumo
Replaced inefficient manual substring searches for JSON fields in extract_json_u64 and extract_json_string with standard serde_json::from_str using a locally defined DiskInfo struct. The unused manual parsing functions were subsequently removed. The original approach used format! internally for every key lookup, causing repeated memory allocation and slow substring search across strings, acting as a CPU and memory performance bottleneck for WMI queries. The serde_json struct approach is nearly 3.5x faster than the original manual unoptimized parsing. Benchmarking extraction of 100,000 payload iterations completed in ~20-22ms down from ~72-75ms.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| \`performance-optimize-json-parsing\` | Refactored JSON parsing in \`find_lun\` | Manual substring search allocations were inefficient | <details><summary>detalhes</summary>**Arquivos:** \`crates/ramshared-winsvc/src/windows_host.rs\`<br>**Validacao:** \`cargo test\`<br>**Risco/rollback:** Minimal; behavior preserved via default fallbacks</details> |

## Issue
N/A

## Responsavel
@Jules

## Labels
type:perf
area:core

## Validacao
- [x] Gates de build/test do escopo tocado
- [ ] \`./scripts/docs-check.sh\` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger
Latency regressions in WMI/CIM queries > 2x baseline over 5 samples, or failure to parse exact matching keys resulting in false-negative matching of LUN identities.`;

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
