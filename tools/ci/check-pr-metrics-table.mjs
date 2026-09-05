#!/usr/bin/env node
import fs from 'node:fs';
import { execSync } from 'node:child_process';

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

export function validatePrMetricsTable(body, { expectedCommits = [], requireEnglish = false } = {}) {
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

  // 3. Language requirement check
  if (requireEnglish) {
    const ptHeaders = [
      /##\s+Resumo/i,
      /##\s+Valida[çc][ãa]o/i,
      /##\s+Respons[áa]vel/i,
      /Evolu[çc][ãa]o\s+Comparativa/i,
      /Mem[óo]ria\s+RAM/i,
      /Mais\s+[ée]\s+melhor/i,
      /Menos\s+[ée]\s+melhor/i,
    ];
    for (const pattern of ptHeaders) {
      if (pattern.test(body)) {
        errors.push(`PR description contains Portuguese markers matching ${pattern} when English is strictly required for merge.`);
      }
    }
  }

  // 4. Check for table presence
  const hasHardwareTable = 
    body.includes('Evolução Comparativa de Hardware') ||
    body.includes('Hardware Benchmark Evolution') ||
    body.includes('Category / Metric') ||
    body.includes('Hardware Benchmark Comparison') ||
    body.includes('Dimensão / Parâmetro') ||
    body.includes('Métrica / Parâmetro');

  const isPerfRelated = 
    /\barea:mm\b/i.test(body) ||
    /\btype:perf\b/i.test(body) ||
    /\b(benchmark|stress|reclaim throughput|page-fault latency)\b/i.test(body);

  if (!hasHardwareTable) {
    if (isPerfRelated) {
      errors.push('Missing required Hardware Benchmark Comparison table (PR touches performance, memory, or benchmarks).');
    }
  } else {
    // Verify multi-tier memory breakdown
    const hasSwapMetrics = /zram/i.test(body) && /vram/i.test(body) && /ssd/i.test(body);
    if (!hasSwapMetrics) {
      errors.push('Hardware table missing multi-tier memory breakdown (Tier 1 ZRAM, Tier 2 VRAM, Tier 3 SSD).');
    }

    // Mandatory Tier 3 qualification data check (fail-closed merge blocker)
    const hasTier3Data = /(?:Tier\s*3|Host\s*SSD|SSD\s*Disk|SSD\s*origin).*?(?:\d+\s*(?:MB|GB|%))/i.test(body);
    if (!hasTier3Data) {
      errors.push('Hardware table missing mandatory Tier 3 (SSD) qualification metrics (must document Tier 3 swap capacity, usage, and status). Merge blocked.');
    }

    // Verify reclaim throughput / latency
    const hasReclaim = /reclaim/i.test(body) || /libera[çc][ãa]o/i.test(body) || /throughput/i.test(body);
    if (!hasReclaim) {
      errors.push('Hardware table missing reclaim throughput / latency metric.');
    }

    // Verify PSI memory pressure
    const hasPsi = /psi/i.test(body) || /press[ãa]o/i.test(body);
    if (!hasPsi) {
      errors.push('Hardware table missing PSI memory pressure metric.');
    }

    // Verify RAM restoration
    const hasRestored = /restaurada/i.test(body) || /restored/i.test(body) || /leak/i.test(body) || /vazamento/i.test(body);
    if (!hasRestored) {
      errors.push('Hardware table missing restored RAM / zero-leak metric.');
    }

    // Verify optimization direction indicators
    const hasDirections = 
      (body.includes('🔺') || body.includes('Higher is better') || body.includes('Mais é melhor')) &&
      (body.includes('🔻') || body.includes('Lower is better') || body.includes('Menos é melhor'));
    if (!hasDirections) {
      errors.push('Hardware table missing explicit optimization directions [🔺 Higher is better] / [🔻 Lower is better].');
    }

    // Verify stability verdict
    const hasVerdict = /PASS_ZERO_PANIC/i.test(body);
    if (!hasVerdict) {
      errors.push('Hardware table missing mandatory stability verdict: PASS_ZERO_PANIC.');
    }

    // Check for unresolved 🔴 ALARM entries
    const hasAlarm = body.includes('🔴 ALARM');
    const hasWaiver = /\[alarm-waiver-justified:[^\]]+\]/i.test(body);
    if (hasAlarm && !hasWaiver) {
      errors.push('Hardware table contains 🔴 ALARM regressions. Fix root-cause per docs/reliability/HARDWARE-METRICS-TRIAGE.md or add [alarm-waiver-justified: <reason>].');
    }
  }

  // 5. Check exhaustive commit list
  if (expectedCommits && expectedCommits.length > 0) {
    const commitsSectionMatch = body.match(/##\s+Commits[\s\S]*?(?=##\s+|$)/i);
    const commitsSection = commitsSectionMatch ? commitsSectionMatch[0] : '';

    if (!commitsSection) {
      errors.push('PR description is missing required ## Commits section.');
    } else {
      const missingCommits = [];
      for (const commitSha of expectedCommits) {
        const shortSha = commitSha.slice(0, 7).toLowerCase();
        if (!commitsSection.toLowerCase().includes(shortSha)) {
          missingCommits.push(shortSha);
        }
      }
      if (missingCommits.length > 0) {
        errors.push(`Missing commits in ## Commits table: ${missingCommits.join(', ')}. All ${expectedCommits.length} branch commits must be listed.`);
      }
    }
  }

  // 6. Check for mandatory labels in PR body
  const labelsSectionMatch = body.match(/##\s+Labels[\s\S]*?(?=##\s+|$)/i);
  const labelsSection = labelsSectionMatch ? labelsSectionMatch[0] : '';
  if (!labelsSection) {
    errors.push('PR description is missing required ## Labels section.');
  } else {
    const hasTypeLabel = /\btype:[a-z0-9_-]+/i.test(labelsSection);
    const hasAreaLabel = /\barea:[a-z0-9_-]+/i.test(labelsSection);
    if (!hasTypeLabel || !hasAreaLabel) {
      errors.push('PR description ## Labels section must specify at least one type:* and one area:* label.');
    }
  }

  return {
    ok: errors.length === 0,
    errors
  };
}

function getBranchCommits(baseRef = 'origin/main') {
  try {
    const output = execSync(`git log ${baseRef}..HEAD --format="%h"`, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'ignore'] });
    return output.trim().split('\n').filter(Boolean);
  } catch {
    return [];
  }
}

function main() {
  const args = process.argv.slice(2);
  let body = '';
  let expectedCommits = [];
  let requireEnglish = args.includes('--require-english');

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

  if (args.includes('--check-commits')) {
    const baseIdx = args.indexOf('--base');
    const base = baseIdx !== -1 && args[baseIdx + 1] ? args[baseIdx + 1] : 'origin/main';
    expectedCommits = getBranchCommits(base);
  } else {
    const commitsArg = args.find(a => a.startsWith('--commits='));
    if (commitsArg) {
      expectedCommits = commitsArg.replace('--commits=', '').split(',').map(s => s.trim()).filter(Boolean);
    }
  }

  const result = validatePrMetricsTable(body, { expectedCommits, requireEnglish });
  if (result.ok) {
    console.log('✓ Hardware metrics comparison table and PR completeness passed validation.');
    process.exit(0);
  } else {
    console.error('✗ Hardware metrics / PR validation failed:');
    result.errors.forEach(e => console.error(`  - ${e}`));
    process.exit(1);
  }
}

if (process.argv[1] && process.argv[1].endsWith('check-pr-metrics-table.mjs')) {
  main();
}
