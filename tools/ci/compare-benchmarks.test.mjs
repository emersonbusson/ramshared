import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

test('compare-benchmarks refuses legacy stress JSON as release evidence', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  try {
    const base = path.join(tmpDir, 'base.json');
    const cand = path.join(tmpDir, 'cand.json');
    const legacy = { status: 'PASS_ZERO_PANIC', reclaim_speed_gbs: 14.4,
      reclaim_duration_ms: 1127, peak_swap_mb: 9216, p99_cycle_latency_ms: 0.0023,
      peak_pressure_index: 10, tier3_ssd_mb: 4096 };
    fs.writeFileSync(base, JSON.stringify(legacy));
    fs.writeFileSync(cand, JSON.stringify(legacy));
    assert.throws(() => execFileSync('node',
      ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'],
      { encoding: 'utf8' }), error => {
      const report = JSON.parse(error.stdout);
      return report.passed === false && report.alarms.some(x => x.includes('unqualified'));
    });
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('compare-benchmarks refuses mismatched workload size', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  try {
    const base = path.join(tmpDir, 'base.json');
    const cand = path.join(tmpDir, 'cand.json');
    const report = { metric_version: 2, status: 'PASS_ZERO_PANIC',
      battery_mode: true, cascade_mode: true, total_allocated_mb: 14768,
      reclaim_speed_gbs: 10, reclaim_duration_ms: 1000, peak_swap_mb: 1000,
      p99_cycle_latency_ms: 0.001, peak_pressure_index: 10, tier3_ssd_mb: 100 };
    fs.writeFileSync(base, JSON.stringify(report));
    fs.writeFileSync(cand, JSON.stringify({ ...report, total_allocated_mb: 16640 }));
    assert.throws(() => execFileSync('node',
      ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'],
      { encoding: 'utf8' }), error => {
      const output = JSON.parse(error.stdout);
      return output.passed === false && output.alarms.some(x => x.includes('workload'));
    });
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('compare-benchmarks refuses unverified integrity and kernel claims', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  try {
    const base = path.join(tmpDir, 'base.json');
    const cand = path.join(tmpDir, 'cand.json');
    const report = { metric_version: 2, status: 'PASS_ZERO_PANIC',
      battery_mode: true, cascade_mode: true, total_allocated_mb: 1000,
      reclaim_speed_gbs: 2, reclaim_duration_ms: 500 };
    fs.writeFileSync(base, JSON.stringify(report));
    fs.writeFileSync(cand, JSON.stringify(report));
    assert.throws(() => execFileSync('node',
      ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'],
      { encoding: 'utf8' }), error => {
      const result = JSON.parse(error.stdout);
      return result.alarms.some(x => x.includes('integrity'))
        && result.alarms.some(x => x.includes('kernel log'));
    });
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('compare-benchmarks flags throughput regression (>3%)', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  const base = path.join(tmpDir, 'base.json');
  const cand = path.join(tmpDir, 'cand.json');

  const baseData = {
    metric_version: 2,
    battery_mode: true,
    cascade_mode: false,
    max_safe_pct: 5,
    total_allocated_mb: 795,
    peak_swap_mb: 1182,
    tier1_zram_mb: 887,
    tier1_zram_pct: 86,
    tier2_vram_mb: 295,
    tier2_vram_pct: 7,
    tier3_ssd_mb: 0,
    tier3_ssd_pct: 0,
    peak_pressure_index: 1.91,
    telemetry_readings_count: 7,
    active_io_cycles_completed: 2,
    reclaim_duration_ms: 300.0,
    reclaim_speed_gbs: 3.00,
    post_reclaim_free_ram_mb: 7000,
    status: 'PASS_ZERO_PANIC'
  };

  // Degraded throughput (2.50 GB/s is -16.6% drop)
  const candData = { ...baseData, reclaim_speed_gbs: 2.50 };

  fs.writeFileSync(base, JSON.stringify(baseData));
  fs.writeFileSync(cand, JSON.stringify(candData));

  let threw = false;
  try {
    execFileSync('node', ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'], { encoding: 'utf8' });
  } catch (err) {
    threw = true;
    const output = JSON.parse(err.stdout);
    assert.equal(output.passed, false, 'Degraded run must not pass');
    assert.ok(output.alarms.some(a => a.includes('Throughput dropped')), 'Must flag throughput drop');
  }
  assert.equal(threw, true, 'Degraded run must exit with non-zero code');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

test('compare-benchmarks passes on throughput gain', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  const base = path.join(tmpDir, 'base.json');
  const cand = path.join(tmpDir, 'cand.json');

  const baseData = {
    metric_version: 2,
    battery_mode: true,
    cascade_mode: false,
    max_safe_pct: 5,
    total_allocated_mb: 795,
    peak_swap_mb: 1182,
    tier1_zram_mb: 887,
    tier1_zram_pct: 86,
    tier2_vram_mb: 295,
    tier2_vram_pct: 7,
    tier3_ssd_mb: 0,
    tier3_ssd_pct: 0,
    peak_pressure_index: 1.91,
    telemetry_readings_count: 7,
    active_io_cycles_completed: 2,
    reclaim_duration_ms: 300.0,
    reclaim_speed_gbs: 2.50,
    post_reclaim_free_ram_mb: 7000,
    status: 'PASS_ZERO_PANIC',
    integrity_status: 'PASS',
    kernel_log_status: 'PASS_ZERO_PANIC'
  };

  // Faster throughput (3.10 GB/s is gain)
  const candData = { ...baseData, reclaim_speed_gbs: 3.10 };

  fs.writeFileSync(base, JSON.stringify(baseData));
  fs.writeFileSync(cand, JSON.stringify(candData));

  const out = execFileSync('node', ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'], { encoding: 'utf8' });
  const parsed = JSON.parse(out);
  assert.equal(parsed.passed, true);
  assert.equal(parsed.alarms.length, 0);

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

test('compare-benchmarks flags tail latency regression (>10%)', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  const base = path.join(tmpDir, 'base.json');
  const cand = path.join(tmpDir, 'cand.json');

  const baseData = {
    metric_version: 2,
    battery_mode: true,
    cascade_mode: false,
    max_safe_pct: 5,
    total_allocated_mb: 795,
    peak_swap_mb: 1182,
    tier1_zram_mb: 887,
    tier1_zram_pct: 86,
    tier2_vram_mb: 295,
    tier2_vram_pct: 7,
    tier3_ssd_mb: 0,
    tier3_ssd_pct: 0,
    peak_pressure_index: 1.91,
    telemetry_readings_count: 7,
    active_io_cycles_completed: 2,
    reclaim_duration_ms: 300.0,
    reclaim_speed_gbs: 3.00,
    post_reclaim_free_ram_mb: 7000,
    status: 'PASS_ZERO_PANIC',
    integrity_status: 'PASS',
    kernel_log_status: 'PASS_ZERO_PANIC',
    p99_cycle_latency_ms: 1.0,
  };

  // Degraded P99 tail latency (1.20 ms is +20% increase)
  const candData = { ...baseData, p99_cycle_latency_ms: 1.20 };

  fs.writeFileSync(base, JSON.stringify(baseData));
  fs.writeFileSync(cand, JSON.stringify(candData));

  let threw = false;
  try {
    execFileSync('node', ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'], { encoding: 'utf8' });
  } catch (err) {
    threw = true;
    const output = JSON.parse(err.stdout);
    assert.equal(output.passed, false, 'Degraded tail latency must not pass');
    assert.ok(output.alarms.some(a => a.includes('P99 tail latency')), 'Must flag P99 tail latency');
  }
  assert.equal(threw, true, 'Degraded run must exit with non-zero code');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

test('compare-benchmarks passes on tail latency reduction', () => {
  const tmpDir = fs.mkdtempSync('/tmp/bench-test-');
  const base = path.join(tmpDir, 'base.json');
  const cand = path.join(tmpDir, 'cand.json');

  const baseData = {
    metric_version: 2,
    battery_mode: true,
    cascade_mode: false,
    max_safe_pct: 5,
    total_allocated_mb: 795,
    peak_swap_mb: 1182,
    tier1_zram_mb: 887,
    tier1_zram_pct: 86,
    tier2_vram_mb: 295,
    tier2_vram_pct: 7,
    tier3_ssd_mb: 0,
    tier3_ssd_pct: 0,
    peak_pressure_index: 1.91,
    telemetry_readings_count: 7,
    active_io_cycles_completed: 2,
    reclaim_duration_ms: 300.0,
    reclaim_speed_gbs: 3.00,
    post_reclaim_free_ram_mb: 7000,
    status: 'PASS_ZERO_PANIC',
    integrity_status: 'PASS',
    kernel_log_status: 'PASS_ZERO_PANIC',
    p99_cycle_latency_ms: 1.0,
  };

  // Improved P99 tail latency (0.80 ms is -20% decrease)
  const candData = { ...baseData, p99_cycle_latency_ms: 0.80 };

  fs.writeFileSync(base, JSON.stringify(baseData));
  fs.writeFileSync(cand, JSON.stringify(candData));

  const out = execFileSync('node', ['tools/ci/compare-benchmarks.mjs', base, cand, '--json'], { encoding: 'utf8' });
  const parsed = JSON.parse(out);
  assert.equal(parsed.passed, true);
  assert.equal(parsed.alarms.length, 0);

  fs.rmSync(tmpDir, { recursive: true, force: true });
});
