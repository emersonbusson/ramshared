#!/usr/bin/env node
import fs from 'node:fs';

function parseJson(filepath) {
  try {
    const raw = fs.readFileSync(filepath, 'utf8');
    return JSON.parse(raw);
  } catch (err) {
    console.error(`Error reading ${filepath}: ${err.message}`);
    process.exit(1);
  }
}

function calcDeltaPct(current, prev) {
  if (prev === 0) {
    return current === 0 ? 0 : 100;
  }
  return ((current - prev) / prev) * 100;
}

function formatDelta(deltaPct) {
  const sign = deltaPct > 0 ? '+' : '';
  return `${sign}${deltaPct.toFixed(1)}%`;
}

function evaluateThroughput(candidate, baseline) {
  const deltaPct = calcDeltaPct(candidate.reclaim_speed_gbs, baseline.reclaim_speed_gbs);
  
  // Regime shift: Heap-only (0 MB swap) -> Physical Hardware Swap (>0 MB swap)
  if (baseline.peak_swap_mb === 0 && candidate.peak_swap_mb > 0) {
    return {
      status: '🟢 GAIN',
      isAlarm: false,
      deltaPct,
      note: 'Physical DMA Tier Activated'
    };
  }

  // Homogeneous comparison
  if (deltaPct > 0.5) {
    return { status: '🟢 GAIN', isAlarm: false, deltaPct, note: 'Throughput increased' };
  } else if (deltaPct < -3.0) {
    return { status: '🔴 ALARM', isAlarm: true, deltaPct, note: 'Throughput dropped > 3%' };
  }
  return { status: '🟡 NEUTRAL', isAlarm: false, deltaPct, note: 'Within 3% tolerance' };
}

function evaluateLatency(candidate, baseline) {
  const deltaPct = calcDeltaPct(candidate.reclaim_duration_ms, baseline.reclaim_duration_ms);

  // Regime shift: Heap-only -> Physical Hardware Swap
  if (baseline.peak_swap_mb === 0 && candidate.peak_swap_mb > 0) {
    return {
      status: '🟢 GAIN',
      isAlarm: false,
      deltaPct,
      note: 'Physical DMA Latency'
    };
  }

  if (deltaPct < -0.5) {
    return { status: '🟢 GAIN', isAlarm: false, deltaPct, note: 'Latency decreased' };
  } else if (deltaPct > 5.0) {
    return { status: '🔴 ALARM', isAlarm: true, deltaPct, note: 'Latency increased > 5%' };
  }
  return { status: '🟡 NEUTRAL', isAlarm: false, deltaPct, note: 'Within 5% tolerance' };
}

function evaluateTailLatency(candidate, baseline) {
  const candP99 = candidate.p99_cycle_latency_ms;
  const baseP99 = baseline.p99_cycle_latency_ms;
  if (!Number.isFinite(candP99) || !Number.isFinite(baseP99) || candP99 <= 0 || baseP99 <= 0) {
    return { status: '🟡 UNMEASURED', isAlarm: false, deltaPct: 0, note: 'P99 missing' };
  }
  const deltaPct = calcDeltaPct(candP99, baseP99);
  if (deltaPct < -0.5) {
    return { status: '🟢 GAIN', isAlarm: false, deltaPct, note: 'Tail latency decreased' };
  } else if (deltaPct > 10.0) {
    return { status: '🔴 ALARM', isAlarm: true, deltaPct, note: 'P99 tail latency increased > 10%' };
  }
  return { status: '🟡 NEUTRAL', isAlarm: false, deltaPct, note: 'Within 10% tolerance' };
}

function main() {
  const args = process.argv.slice(2);
  const isMarkdown = args.includes('--markdown');
  const isJson = args.includes('--json');
  const fileArgs = args.filter(a => !a.startsWith('--'));

  const baselinePath = fileArgs[0] || 'docs/benchmarks/baseline.json';
  const candidatePath = fileArgs[1] || 'docs/benchmarks/history/latest.json';

  if (!fs.existsSync(baselinePath)) {
    console.error(`Baseline benchmark file not found: ${baselinePath}`);
    process.exit(1);
  }
  if (!fs.existsSync(candidatePath)) {
    console.error(`Candidate benchmark file not found: ${candidatePath}`);
    process.exit(1);
  }

  const baseline = parseJson(baselinePath);
  const candidate = parseJson(candidatePath);

  if (baseline.metric_version !== 2 || candidate.metric_version !== 2) {
    const alarms = ['legacy/unqualified stress metrics cannot support a release comparison'];
    if (isJson) {
      console.log(JSON.stringify({ baseline, candidate, alarms, passed: false }, null, 2));
    } else {
      console.error(alarms[0]);
    }
    process.exit(1);
  }
  if (baseline.total_allocated_mb !== candidate.total_allocated_mb
      || baseline.battery_mode !== candidate.battery_mode
      || baseline.cascade_mode !== candidate.cascade_mode) {
    const alarms = ['incomparable workload: allocated RAM or test mode differs'];
    if (isJson) {
      console.log(JSON.stringify({ baseline, candidate, alarms, passed: false }, null, 2));
    } else {
      console.error(alarms[0]);
    }
    process.exit(1);
  }
  if (![baseline.reclaim_speed_gbs, candidate.reclaim_speed_gbs]
      .every(value => Number.isFinite(value) && value > 0)) {
    const alarms = ['unmeasured reclaim throughput cannot support a speed comparison'];
    if (isJson) {
      console.log(JSON.stringify({ baseline, candidate, alarms, passed: false }, null, 2));
    } else {
      console.error(alarms[0]);
    }
    process.exit(1);
  }

  const alarms = [];

  // Evaluate Throughput
  const throughput = evaluateThroughput(candidate, baseline);
  if (throughput.isAlarm) alarms.push(throughput.note);

  // Evaluate Latency
  const latency = evaluateLatency(candidate, baseline);
  if (latency.isAlarm) alarms.push(latency.note);

  // Evaluate Tail Latency (P99 Jitter)
  const tailLatency = evaluateTailLatency(candidate, baseline);
  if (tailLatency.isAlarm) alarms.push(tailLatency.note);

  // Evaluate SSD Spillover
  let ssdStatus = '🟢 GAIN';
  if (candidate.tier3_ssd_mb > 0 && baseline.tier3_ssd_mb === 0) {
    if (candidate.cascade_mode) {
      ssdStatus = '🟢 GAIN';
    } else {
      ssdStatus = '🔴 ALARM';
      alarms.push(`Unintended Tier 3 SSD Spillover: ${candidate.tier3_ssd_mb} MB`);
    }
  }

  // Evaluate PSI Tolerance
  const psiDelta = calcDeltaPct(candidate.peak_pressure_index, baseline.peak_pressure_index);
  const psiStatus = psiDelta >= -3.0 ? '🟢 GAIN' : '🔴 ALARM';
  if (psiStatus === '🔴 ALARM') alarms.push('PSI Pressure Tolerance dropped > 3%');

  // Evaluate Stability Status
  const isPass = candidate.status === 'PASS_ZERO_PANIC';
  if (!isPass) alarms.push(`Host Stability Failed: ${candidate.status}`);
  if (candidate.integrity_status !== 'PASS' || baseline.integrity_status !== 'PASS') {
    alarms.push('independent integrity proof missing');
  }
  if (candidate.kernel_log_status !== 'PASS_ZERO_PANIC'
      || baseline.kernel_log_status !== 'PASS_ZERO_PANIC') {
    alarms.push('independent kernel log proof missing');
  }

  if (isJson) {
    console.log(JSON.stringify({ baseline, candidate, alarms, passed: alarms.length === 0 }, null, 2));
    process.exit(alarms.length === 0 ? 0 : 1);
  }

  const measured = (value, unit = '') => Number.isFinite(value) ? `${value}${unit}` : 'N/A';
  if (isMarkdown) {
    console.log('| Metric | Baseline | Candidate | Assessment |');
    console.log('| --- | ---: | ---: | --- |');
    console.log(`| Allocated RAM | ${measured(baseline.total_allocated_mb, ' MB')} | ${measured(candidate.total_allocated_mb, ' MB')} | Matched workload |`);
    console.log(`| Logical swap engaged | ${measured(baseline.peak_swap_mb, ' MB')} | ${measured(candidate.peak_swap_mb, ' MB')} | Logical occupancy only |`);
    console.log(`| Physical GPU cache | ${measured(baseline.tier2_vram_mb, ' MB')} | ${measured(candidate.tier2_vram_mb, ' MB')} | Daemon cache telemetry, if sampled |`);
    console.log(`| SSD swap | ${measured(baseline.tier3_ssd_mb, ' MB')} | ${measured(candidate.tier3_ssd_mb, ' MB')} | Active swap disk |`);
    console.log(`| Measured reclaim speed | ${measured(baseline.reclaim_speed_gbs, ' GB/s')} | ${measured(candidate.reclaim_speed_gbs, ' GB/s')} | ${throughput.status} |`);
    console.log(`| Measured reclaim duration | ${measured(baseline.reclaim_duration_ms, ' ms')} | ${measured(candidate.reclaim_duration_ms, ' ms')} | ${latency.status} |`);
    console.log(`| P99 cycle latency | ${measured(baseline.p99_cycle_latency_ms, ' ms')} | ${measured(candidate.p99_cycle_latency_ms, ' ms')} | ${tailLatency.status} |`);
    console.log(`| Integrity | ${baseline.integrity_status ?? 'N/A'} | ${candidate.integrity_status ?? 'N/A'} | Independent hash proof required |`);
    console.log(`| Kernel stability | ${baseline.status} | ${candidate.status} | Independent log proof required |`);
  } else {
    console.log(`Benchmark comparison: ${baselinePath} -> ${candidatePath}`);
    console.log(`Reclaim speed: ${measured(baseline.reclaim_speed_gbs, ' GB/s')} -> ${measured(candidate.reclaim_speed_gbs, ' GB/s')} [${throughput.status}]`);
    console.log(`Reclaim duration: ${measured(baseline.reclaim_duration_ms, ' ms')} -> ${measured(candidate.reclaim_duration_ms, ' ms')} [${latency.status}]`);
    console.log(`P99 cycle latency: ${measured(baseline.p99_cycle_latency_ms, ' ms')} -> ${measured(candidate.p99_cycle_latency_ms, ' ms')} [${tailLatency.status}]`);
    console.log(`Status: ${candidate.status}; alarms: ${alarms.join('; ') || 'none'}`);
  }

  process.exit(alarms.length === 0 ? 0 : 1);
}

main();
