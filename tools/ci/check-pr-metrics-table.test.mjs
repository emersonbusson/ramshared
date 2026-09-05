import test from 'node:test';
import assert from 'node:assert/strict';
import { validatePrMetricsTable } from './check-pr-metrics-table.mjs';

const VALID_TABLE = `
## Resumo
Test release consolidation.

### 📊 Evolução Comparativa de Hardware
| Métrica / Parâmetro | Baseline (#883) | Atual (#1049) | Delta & Dir | Veredito Hardware |
| :--- | :---: | :---: | :---: | :--- |
| **Memória RAM Alocada** | 795 MB | 795 MB | 🟡 Baseline | Volume sob teste |
| **Swap Multinível Ativo** | 0 MB (Heap) | **1.182 MB** | 🟢 GAIN (+1.182 MB) [🔺] | Ativação física de ublk + GPU DMA |
| ├─ *Tier 1: ZRAM LZ4* | 0 MB | **887 MB** | 🟢 GAIN (+887 MB) [🔺] | Compressão kernel ultrarrápida |
| ├─ *Tier 2: GPU VRAM* | 0 MB | **295 MB** | 🟢 GAIN (+295 MB) [🔺] | Páginas alocadas na RTX 2060 |
| └─ *Tier 3: Host SSD* | 0 MB | **0 MB** | 🟢 GAIN (0 MB) [🔻] | Proteção: 0% de spill em disco |
| **Pressão Suportada (PSI)** | 1.175 | **1.913** | 🟢 GAIN (+62.8%) [🔺] | Pico absorvido sem travamento WSL2 |
| **Throughput / Latência** | 9.09 GB/s (85ms) | **2.47 GB/s (313ms)** | 🟢 GAIN (PCIe DMA) [🔺] | Barramento físico real PCIe Gen3 x16 |
| **RAM Restaurada pós-run** | 10.384 MB | **7.369 MB** | 🟢 GAIN (0 MB leak) [🔺] | Zero vazamentos pós-teste |
| **Estabilidade Operacional** | \`PASS_ZERO_PANIC\` | \`PASS_ZERO_PANIC\` | 🟢 PASS | Zero panics / stalls / OOM-kills |

## Commits
| Commit | Tipo / Escopo | Descrição |
| :---: | :--- | :--- |
| \`cc916bc\` | \`feat(ci)\` | Standardize hardware metrics comparison |
| \`0024464\` | \`docs(gov)\` | Codify empirical hardware metrics table |

## Labels
type:feat, area:core, area:mm
`;

test('valid hardware metrics table passes', () => {
  const res = validatePrMetricsTable(VALID_TABLE);
  assert.equal(res.ok, true, 'Valid table must pass');
  assert.equal(res.errors.length, 0);
});

test('detects forbidden buzzwords', () => {
  const badBody = VALID_TABLE + '\nAudited under Kahneman disciplines and cognitive hygiene.';
  const res = validatePrMetricsTable(badBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('forbidden internal methodology buzzword')));
});

test('detects raw JSON links', () => {
  const badBody = VALID_TABLE + '\nTelemetry verified in [`benchmark-2026-09-05_01-32-17.json`](docs/benchmarks/history/benchmark-2026-09-05_01-32-17.json)';
  const res = validatePrMetricsTable(badBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('raw JSON benchmark link')));
});

test('fails on missing PASS_ZERO_PANIC', () => {
  const badBody = VALID_TABLE.replace(/PASS_ZERO_PANIC/g, 'FAIL_PANIC');
  const res = validatePrMetricsTable(badBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('PASS_ZERO_PANIC')));
});

test('fails on missing direction indicators', () => {
  const badBody = VALID_TABLE.replace(/\[🔺\]/g, '').replace(/\[🔻\]/g, '');
  const res = validatePrMetricsTable(badBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('missing explicit optimization directions')));
});

test('fails on unresolved 🔴 ALARM without waiver', () => {
  const badBody = VALID_TABLE + '\n| Throughput | 9.09 GB/s | 1.50 GB/s | -83.5% | 🔴 ALARM | Stalling |';
  const res = validatePrMetricsTable(badBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('🔴 ALARM regressions')));
});

test('allows 🔴 ALARM with explicit documented waiver', () => {
  const waivedBody = VALID_TABLE + '\n| Throughput | 9.09 GB/s | 1.50 GB/s | -83.5% | 🔴 ALARM | Stalling |\n[alarm-waiver-justified: Degraded due to PCIe link width drop on testbed VM]';
  const res = validatePrMetricsTable(waivedBody);
  assert.equal(res.ok, true, 'Waived alarm must pass');
});

test('non-perf PR passes without table but with valid labels', () => {
  const nonPerfBody = '## Resumo\nDocumentation typo fix.\n## Commits\n- docs: fix typo\n## Labels\ntype:docs, area:governance';
  const res = validatePrMetricsTable(nonPerfBody);
  assert.equal(res.ok, true);
  assert.equal(res.errors.length, 0);
});

test('fails when labels are missing or invalid', () => {
  const missingLabelsBody = VALID_TABLE.replace(/## Labels[\s\S]*$/i, '');
  const res = validatePrMetricsTable(missingLabelsBody);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('missing required ## Labels section')));

  const invalidLabelsBody = VALID_TABLE.replace(/type:feat, area:core, area:mm/, 'random-label');
  const res2 = validatePrMetricsTable(invalidLabelsBody);
  assert.equal(res2.ok, false);
  assert.ok(res2.errors.some(e => e.includes('must specify at least one type:* and one area:* label')));
});

test('perf PR fails when table is missing', () => {
  const perfBodyMissingTable = '## Resumo\nImproved memory reclaim throughput.\n## Labels\narea:mm\ntype:perf\n## Validacao\nRan stress battery';
  const res = validatePrMetricsTable(perfBodyMissingTable);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('Missing required Hardware Benchmark Comparison table')));
});

test('commit list verification succeeds when all commits are listed', () => {
  const res = validatePrMetricsTable(VALID_TABLE, { expectedCommits: ['cc916bc', '0024464'] });
  assert.equal(res.ok, true);
});

test('commit list verification fails when a commit is missing', () => {
  const res = validatePrMetricsTable(VALID_TABLE, { expectedCommits: ['cc916bc', '0024464', 'ba2a5e9'] });
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('Missing commits in ## Commits table: ba2a5e9')));
});

test('language validation rejects PT-BR when English is strictly required for merge', () => {
  const res = validatePrMetricsTable(VALID_TABLE, { requireEnglish: true });
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('contains Portuguese markers')));
});

