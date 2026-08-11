import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  computeStats,
  platformFingerprint,
  validateRecord,
  validateRepository,
} from './check-benchmark-evidence.mjs'

const SHA = 'a'.repeat(64)

function validRecord(overrides = {}) {
  const record = {
    schema_version: 'ramshared-evidence/v1',
    run_id: 'fixture-run-001',
    surface: 'userspace',
    slug: 'benchmark-evidence-integrity',
    utc: {
      started: '2026-08-09T12:00:00.000Z',
      ended: '2026-08-09T12:00:03.000Z',
    },
    source: {
      commit: '1'.repeat(40),
      dirty: false,
      dirty_entry_count: 0,
      invocation: 'node scripts/p0/example.mjs --runs 3',
      harness_revision: 'fixture-v1',
    },
    platform: {
      condition: 'idle',
      os: 'linux',
      os_build: 'fixture',
      cpu: 'fixture-cpu',
      ram_mib: 4096,
    },
    candidate: {
      manifest_sha256: SHA,
      loaded_binary_sha256: SHA,
    },
    workload: {
      profile: 'baseline',
      parameters: { block_bytes: 4096, queue_depth: 1 },
      warmup_seconds: 1,
      runs: 3,
    },
    comparison: {
      platform_fingerprint: '',
      baseline_run_id: 'fixture-baseline-001',
      baseline_fingerprint: '',
      qualified: true,
    },
    metrics: {
      latency_ms: {
        unit: 'ms',
        samples: [1, 2, 3],
        n: 3,
        aggregation: 'median-nearest-rank-p99-population-stddev',
        median: 2,
        p99_nearest_rank: 3,
        stddev: Math.sqrt(2 / 3),
        min: 1,
        max: 3,
      },
    },
    lifecycle: {
      before: { clean: true },
      action: { command: 'public fixture' },
      after: { clean: true },
      binary_match: true,
      legitimate: { verdict: 'PASS' },
      refusals: [{ name: 'invalid-input', verdict: 'PASS' }],
      cleanup: { complete: true },
      residue: 0,
    },
    artifacts: [],
    decision: {
      verdict: 'PASS',
      promotable: true,
      gaps: [],
      rollback_trigger: 'one mismatched checksum',
    },
  }
  Object.assign(record, overrides)
  record.comparison.platform_fingerprint = platformFingerprint(record)
  record.comparison.baseline_fingerprint = record.comparison.platform_fingerprint
  return record
}

function fixtureRepo() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-benchmark-evidence-'))
  mkdirSync(path.join(root, 'docs', 'benchmarks'), { recursive: true })
  writeFileSync(
    path.join(root, 'docs', 'BENCHMARKS.md'),
    '<!-- ramshared-benchmark-id: fixture-benchmark -->\n## 2026-08-09 12:00 UTC — fixture\n',
  )
  writeFileSync(
    path.join(root, 'docs', 'benchmarks', 'results.jsonl'),
    `${JSON.stringify(validRecord())}\n`,
  )
  writeFileSync(
    path.join(root, 'docs', 'benchmarks', 'legacy-unqualified.json'),
    `${JSON.stringify({ schema_version: 'ramshared-legacy-benchmarks/v1', entries: [] })}\n`,
  )
  writeFileSync(
    path.join(root, 'docs', 'benchmarks', 'benchmark-map.json'),
    `${JSON.stringify({ schema_version: 'ramshared-benchmark-map/v1', entries: [{ benchmark_id: 'fixture-benchmark', run_id: 'fixture-run-001' }] })}\n`,
  )
  return root
}

test('valid_v1_record_passes', () => {
  assert.deepEqual(validateRecord(validRecord(), { root: process.cwd() }), [])
})

test('missing_required_group_fails', () => {
  const record = validRecord()
  delete record.lifecycle
  assert.match(validateRecord(record, { root: process.cwd() }).join('\n'), /required-lifecycle/)
})

test('rejects_missing_schema_version_duplicate_run_id_or_dirty_provenance', () => {
  const missing = validRecord()
  delete missing.schema_version
  assert.match(validateRecord(missing, { root: process.cwd() }).join('\n'), /schema-version/)

  const dirty = validRecord()
  dirty.source = { ...dirty.source, dirty: true }
  delete dirty.source.dirty_entry_count
  assert.match(validateRecord(dirty, { root: process.cwd() }).join('\n'), /dirty-provenance/)

  const root = fixtureRepo()
  const line = JSON.stringify(validRecord())
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'results.jsonl'), `${line}\n${line}\n`)
  assert.match(validateRepository({ root }).findings.join('\n'), /duplicate-run-id/)
})

test('rejects_absolute_missing_or_hash_mismatched_artifact', () => {
  const absolute = validRecord({ artifacts: [{ path: '/private/result.json', bytes: 2, sha256: SHA }] })
  assert.match(validateRecord(absolute, { root: process.cwd() }).join('\n'), /artifact-path/)

  const root = fixtureRepo()
  const missing = validRecord({ artifacts: [{ path: 'evidence/missing.json', bytes: 2, sha256: SHA }] })
  assert.match(validateRecord(missing, { root }).join('\n'), /artifact-missing/)

  mkdirSync(path.join(root, 'evidence'))
  writeFileSync(path.join(root, 'evidence', 'result.json'), '{}')
  const mismatch = validRecord({ artifacts: [{ path: 'evidence/result.json', bytes: 2, sha256: SHA }] })
  assert.match(validateRecord(mismatch, { root }).join('\n'), /artifact-hash/)
})

test('rejects_secret_pii_or_kernel_address_without_echoing_value', () => {
  const sensitive = 'token-' + 'private-value'
  const record = validRecord()
  record.source.invocation = `run --api-key ${sensitive}`
  const output = validateRecord(record, { root: process.cwd() }).join('\n')
  assert.match(output, /sensitive-content/)
  assert.doesNotMatch(output, new RegExp(sensitive))

  record.source.invocation = `kernel fault at ${['ffff', '888012345678'].join('')}`
  assert.match(validateRecord(record, { root: process.cwd() }).join('\n'), /sensitive-content/)
})

test('recomputes_statistics_and_rejects_forgery', () => {
  const stats = computeStats([1, 2, 3])
  assert.equal(stats.median, 2)
  assert.equal(stats.p99_nearest_rank, 3)
  assert.equal(stats.n, 3)

  const forged = validRecord()
  forged.metrics.latency_ms.median = 99
  assert.match(validateRecord(forged, { root: process.cwd() }).join('\n'), /metric-median/)
})

test('incompatible_fingerprint_is_incomparable', () => {
  const record = validRecord()
  record.comparison.baseline_fingerprint = 'b'.repeat(64)
  assert.match(validateRecord(record, { root: process.cwd() }).join('\n'), /comparison-verdict/)

  record.decision = { ...record.decision, verdict: 'INCOMPARABLE', promotable: false }
  assert.doesNotMatch(validateRecord(record, { root: process.cwd() }).join('\n'), /comparison-verdict/)
})

test('nonpass_verdict_cannot_promote_or_print_pass', () => {
  for (const verdict of ['RED', 'YELLOW', 'INCOMPARABLE', 'BASELINE']) {
    const record = validRecord()
    record.decision = { ...record.decision, verdict, promotable: true }
    assert.match(validateRecord(record, { root: process.cwd() }).join('\n'), /nonpass-promotable/)
  }
})

test('dated_benchmark_requires_exactly_one_registry_mapping', () => {
  const root = fixtureRepo()
  assert.deepEqual(validateRepository({ root }).findings, [])
  writeFileSync(
    path.join(root, 'docs', 'benchmarks', 'benchmark-map.json'),
    JSON.stringify({ schema_version: 'ramshared-benchmark-map/v1', entries: [] }),
  )
  assert.match(validateRepository({ root }).findings.join('\n'), /benchmark-unmapped/)
})

test('historical_record_requires_explicit_legacy_unqualified_marker', () => {
  const root = fixtureRepo()
  const legacy = { run_id: 'old-run', timestamp: '2026-01-01T00:00:00Z' }
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'results.jsonl'), `${JSON.stringify(legacy)}\n`)
  assert.match(validateRepository({ root }).findings.join('\n'), /legacy-record-unmapped/)
})

test('legacy_marker_cannot_promote', () => {
  const root = fixtureRepo()
  const legacyPath = path.join(root, 'docs', 'benchmarks', 'legacy-unqualified.json')
  writeFileSync(legacyPath, JSON.stringify({
    schema_version: 'ramshared-legacy-benchmarks/v1',
    entries: [{ benchmark_id: 'old', run_id: 'old-run', qualified: true, reason: 'missing raw context' }],
  }))
  const legacy = JSON.parse(readFileSync(legacyPath, 'utf8'))
  assert.equal(legacy.entries[0].qualified, true)
  assert.match(validateRepository({ root }).findings.join('\n'), /legacy-qualified/)
})
