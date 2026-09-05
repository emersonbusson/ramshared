#!/usr/bin/env node
import fs from 'node:fs';

const FORBIDDEN_BUZZWORDS = [
  /\bcognitive\s+hygiene\b/i,
  /\bhigiene\s+cognitiva\b/i,
  /\bkahneman\b/i,
];

const FORBIDDEN_JSON_LINKS = [
  /\[[^\]]*benchmark-[^\]]*\.json[^\]]*\]/i,
  /\([^)]*benchmark-[^)]*\.json[^)]*\)/i,
  /\[[^\]]*latest\.json[^\]]*\]/i,
  /\([^)]*latest\.json[^)]*\)/i,
];

export function validatePrMetricsTable(body) {
  const errors = [];

  if (!body || typeof body !== 'string') {
    return { ok: false, errors: ['PR description is empty or invalid string.'] };
  }

  // 1. Check for forbidden buzzwords
  for (const pattern of FORBIDDEN_BUZZWORDS) {
    if (pattern.test(body)) {
      errors.push(`PR description contains forbidden internal methodology buzzword matching ${pattern}.`);
    }
  }

  // 2. Check for forbidden raw JSON links
  for (const pattern of FORBIDDEN_JSON_LINKS) {
    if (pattern.test(body)) {
      errors.push(`PR description contains forbidden raw JSON benchmark link matching ${pattern}. Keep hardware evidence self-contained.`);
    }
  }

  // 3. Check for table presence
  const hasHardwareTable = 
    body.includes('Evolução Comparativa de Hardware') ||
    body.includes('Category / Metric') ||
    body.includes('Hardware Benchmark Comparison') ||
    body.includes('Dimensão / Parâmetro');

  const isPerfRelated = 
    /\barea:mm\b/i.test(body) ||
    /\btype:perf\b/i.test(body) ||
    /\b(benchmark|stress|reclaim throughput|page-fault latency)\b/i.test(body);

  if (!hasHardwareTable) {
    if (isPerfRelated) {
      errors.push('Missing required Hardware Benchmark Comparison table (PR touches performance, memory, or benchmarks).');
    }
    return {
      ok: errors.length === 0,
      errors
    };
  }

  // 4. Verify essential domains
  const hasSwapMetrics = /zram/i.test(body) && /vram/i.test(body) && /ssd/i.test(body);
  if (!hasSwapMetrics) {
    errors.push('Hardware table missing multi-tier memory breakdown (ZRAM, VRAM, SSD).');
  }

  const hasReclaim = /reclaim/i.test(body) || /libera[çc][ãa]o/i.test(body);
  if (!hasReclaim) {
    errors.push('Hardware table missing reclaim bandwidth/latency metric.');
  }

  const hasPsi = /psi/i.test(body) || /press[ãa]o/i.test(body);
  if (!hasPsi) {
    errors.push('Hardware table missing PSI memory pressure metric.');
  }

  const hasVerdict = /PASS_ZERO_PANIC/i.test(body);
  if (!hasVerdict) {
    errors.push('Hardware table missing mandatory stability verdict: PASS_ZERO_PANIC.');
  }

  // 5. Check for unresolved 🔴 ALARM entries
  const hasAlarm = body.includes('🔴 ALARM');
  const hasWaiver = /\[alarm-waiver-justified:[^\]]+\]/i.test(body);
  if (hasAlarm && !hasWaiver) {
    errors.push('Hardware table contains 🔴 ALARM regressions. Fix root-cause per docs/reliability/HARDWARE-METRICS-TRIAGE.md or add [alarm-waiver-justified: <reason>].');
  }

  return {
    ok: errors.length === 0,
    errors
  };
}

function main() {
  const args = process.argv.slice(2);
  let body = '';

  const fileIdx = args.indexOf('--file');
  if (fileIdx !== -1 && args[fileIdx + 1]) {
    body = fs.readFileSync(args[fileIdx + 1], 'utf8');
  } else if (args[0] && !args[0].startsWith('--')) {
    body = args[0];
  } else if (process.env.PR_BODY) {
    body = process.env.PR_BODY;
  } else {
    try {
      body = fs.readFileSync(0, 'utf8');
    } catch {
      body = '';
    }
  }

  const result = validatePrMetricsTable(body);
  if (result.ok) {
    console.log('✓ Hardware metrics comparison table passed validation.');
    process.exit(0);
  } else {
    console.error('✗ Hardware metrics validation failed:');
    result.errors.forEach(e => console.error(`  - ${e}`));
    process.exit(1);
  }
}

if (process.argv[1] && process.argv[1].endsWith('check-pr-metrics-table.mjs')) {
  main();
}
