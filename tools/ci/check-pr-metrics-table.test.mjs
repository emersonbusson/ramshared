import test from 'node:test';
import assert from 'node:assert/strict';
import { validatePrMetricsTable } from './check-pr-metrics-table.mjs';

const VALID_TABLE = `
## Resumo
Test release consolidation.

### 📊 Evolução Comparativa de Hardware
| Dimensão / Parâmetro | Baseline de Soak (PR #555) | Consolidação 383 PRs (PR #883) | Consolidação Atual 162 PRs (PR #1049) | O que isso demonstra no hardware real |
| :--- | :---: | :---: | :---: | :--- |
| **Memória RAM Alocada** | 17.364 MB | 795 MB | 795 MB | Volume de pressão gerado pelo teste |
| **Swap Multinível Engajado** | 9.160 MB | 0 MB | 1.182 MB | Capacidade do kernel de empurrar swap |
| **Tier 1 — ZRAM LZ4 (RAM)** | 1.024 MB | 0 MB | 887 MB | Compressão ultrarrápida transparente |
| **Tier 2 — GPU VRAM (RTX 2060)** | 4.096 MB | 0 MB | 295 MB | Páginas alocadas na GPU via PCIe DMA |
| **Tier 3 — Host SSD (Disco)** | 4.040 MB | 0 MB | 0 MB | Proteção de SSD: 0% de derramamento |
| **Pressão Suportada (PSI)** | Saturação | 1.175 | 1.913 | Absorção de pico sem travar |
| **Tempo de Liberação (Reclaim)** | Descarte | 85.46 ms | 313.76 ms | Tempo real para descarregar |
| **Veredito de Estabilidade** | PASS_ZERO_PANIC | PASS_ZERO_PANIC | PASS_ZERO_PANIC | Zero panics, zero stalls |
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

test('non-perf PR passes without table', () => {
  const nonPerfBody = '## Resumo\nDocumentation typo fix.\n## Commits\n- docs: fix typo';
  const res = validatePrMetricsTable(nonPerfBody);
  assert.equal(res.ok, true);
  assert.equal(res.errors.length, 0);
});

test('perf PR fails when table is missing', () => {
  const perfBodyMissingTable = '## Resumo\nImproved memory reclaim throughput.\n## Labels\narea:mm\ntype:perf\n## Validacao\nRan stress battery';
  const res = validatePrMetricsTable(perfBodyMissingTable);
  assert.equal(res.ok, false);
  assert.ok(res.errors.some(e => e.includes('Missing required Hardware Benchmark Comparison table')));
});
