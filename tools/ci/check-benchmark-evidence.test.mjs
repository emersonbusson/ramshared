import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
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

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function publicMetric(samples) {
  return {
    unit: 'ms',
    samples,
    aggregation: 'median-nearest-rank-p99-population-stddev',
    ...computeStats(samples),
  }
}

function wsl2PublicPairFixture({ tierMiB = 1024, condition = 'idle' } = {}) {
  const policy = {
    1024: { sampleTimeoutSec: 240, finalizationTimeoutSec: 120, cellOuterTimeoutSec: 1380, cudaHoldSec: 2880 },
    2048: { sampleTimeoutSec: 240, finalizationTimeoutSec: 240, cellOuterTimeoutSec: 1740, cudaHoldSec: 3600 },
    4096: { sampleTimeoutSec: 600, finalizationTimeoutSec: 600, cellOuterTimeoutSec: 3900, cudaHoldSec: 7920 },
  }[tierMiB]
  if (!policy) throw new Error(`unsupported fixture tier: ${tierMiB}`)
  const pairId = `${tierMiB}-${condition}`
  const runId = `wsl2-nbd-${pairId}-fixture`
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-wsl2-public-pair-'))
  const artifactRoot = [
    'docs', 'specs', 'no-milestone', 'wsl2-nbd-product-readiness', 'evidence',
    runId,
  ]
  const directory = path.join(root, ...artifactRoot)
  mkdirSync(directory, { recursive: true })
  const cellTimeoutBudget = {
    sample_timeout_sec: policy.sampleTimeoutSec,
    integrity_finalization_timeout_sec: policy.finalizationTimeoutSec,
    samples: 3,
    setup_cleanup_timeout_sec: 300,
    cell_outer_timeout_sec: policy.cellOuterTimeoutSec,
  }
  const pairTimeoutBudget = {
    cell: cellTimeoutBudget,
    cuda_hold_min_sec: policy.cudaHoldSec,
  }
  const comparisonRecord = {
    schema_version: 'ramshared-nbd-public-pair-comparison/v1',
    pair_id: pairId,
    environment_fingerprint: '3'.repeat(64),
    baseline_verdict: 'BASELINE_CANDIDATE',
    baseline_reason: 'manufactured',
    nbd_vs_disk_median_ratio: 1.045455,
    nbd_vs_disk_p99_ratio: 1.041667,
    nbd_vs_disk_population_stddev_ratio: 1,
    timeout_budget: pairTimeoutBudget,
  }
  const custodyRecord = {
    schema_version: 'ramshared-nbd-public-pair-custody/v1',
    pair_id: pairId,
    release: {
      version: 'manufactured-v1',
      source_commit: '1'.repeat(40),
      installed_manifest_sha256: '2'.repeat(64),
      input_bundle_manifest_sha256: 'not_exposed',
    },
    cells: [
      {
        mode: 'disk-only',
        binary_match: 'N/A',
        context_sha256: 'c'.repeat(64),
        summary_sha256: '4'.repeat(64),
        artifact_inventory_sha256: '5'.repeat(64),
        internal_envelope_sha256: '6'.repeat(64),
        timeout_budget: cellTimeoutBudget,
      },
      {
        mode: 'nbd',
        binary_match: 'PASS',
        context_sha256: 'd'.repeat(64),
        summary_sha256: '7'.repeat(64),
        artifact_inventory_sha256: '8'.repeat(64),
        internal_envelope_sha256: '9'.repeat(64),
        timeout_budget: cellTimeoutBudget,
      },
    ],
    timeout_budget: pairTimeoutBudget,
    cuda_hold_sec: condition === 'bounded' ? policy.cudaHoldSec : 0,
    comparison_sha256: 'a'.repeat(64),
    cleanup: { complete: true, terminal_state: 'PRODUCT_OFF' },
  }
  const comparison = JSON.stringify(comparisonRecord)
  custodyRecord.comparison_sha256 = sha256(comparison)
  const custody = JSON.stringify(custodyRecord)
  const custodyPath = path.join(directory, 'pair-custody.json')
  const comparisonPath = path.join(directory, 'pair-comparison.json')
  writeFileSync(custodyPath, custody)
  writeFileSync(comparisonPath, comparison)
  const artifact = (name, contents) => ({
    path: [...artifactRoot, name].join('/'),
    bytes: Buffer.byteLength(contents),
    sha256: sha256(contents),
  })
  const record = {
    schema_version: 'ramshared-evidence/v1',
    run_id: runId,
    surface: 'wsl2-nbd',
    slug: 'wsl2-nbd-product-readiness',
    utc: { started: '2026-08-12T12:00:00.000Z', ended: '2026-08-12T12:02:00.000Z' },
    source: {
      commit: '1'.repeat(40),
      dirty: false,
      dirty_entry_count: 0,
      invocation: `Invoke-NbdBenchmarkMatrix.ps1 approved pair ${pairId}`,
      harness_revision: 'b'.repeat(64),
    },
    platform: {
      kernel_release: '6.6.0-manufactured',
      gpu_model: 'Manufactured GPU',
      gpu_driver: '1.2.3',
      zram: {
        device: 'zram0',
        algorithm: 'zstd',
        size_kib: 1048576,
        priority: 200,
        identity_sha256: 'e'.repeat(64),
      },
      lower: { type: 'nbd', sink_type: 'directory', sink_identity_sha256: '6'.repeat(64) },
    },
    candidate: {
      classification: 'candidate/noncanonical',
      canonical: false,
      publication_state: 'campaign-root-pending-repository-copy',
      repository_artifact_root: artifactRoot.join('/'),
      installed_manifest_sha256: '2'.repeat(64),
      input_bundle_manifest_sha256: 'not_exposed',
      pair_custody_sha256: sha256(custody),
      pair_comparison_sha256: sha256(comparison),
    },
    workload: {
      profile: 'anonymous_memory_sequential_write',
      parameters: {
        tier_mib: tierMiB,
        condition,
        pattern: 'shake256-v1',
        allocation_chunk_bytes: 67108864,
        worker_threads: 1,
        allocated_mib: tierMiB + 2560,
        timeout_budget: pairTimeoutBudget,
      },
      warmup_seconds: 0,
      runs: 3,
    },
    comparison: {
      platform_fingerprint: '',
      baseline_run_id: 'candidate-self',
      baseline_fingerprint: '',
      qualified: false,
      pair_environment_fingerprint: '3'.repeat(64),
      baseline_verdict: 'BASELINE_CANDIDATE',
    },
    metrics: {
      disk_allocation_to_hold_ms: publicMetric([100, 110, 120]),
      nbd_allocation_to_hold_ms: publicMetric([105, 115, 125]),
    },
    lifecycle: {
      before: { custody_sha256: 'c'.repeat(64) },
      action: { pair_id: pairId, mode_order: ['disk-only', 'nbd'] },
      after: { terminal_state: 'PRODUCT_OFF', custody_sha256: 'd'.repeat(64) },
      binary_match: true,
      legitimate: { verdict: 'PASS' },
      refusals: [{ name: 'approved_live_fixture_seams_forbidden', verdict: 'PASS' }],
      cleanup: { complete: true },
      residue: 0,
    },
    artifacts: [
      artifact('pair-custody.json', custody),
      artifact('pair-comparison.json', comparison),
    ],
    decision: {
      verdict: 'BASELINE',
      promotable: false,
      gaps: ['baseline_absent', 'candidate/noncanonical'],
      rollback_trigger: 'Any NBD identity, comparison, custody, cleanup, or repository-copy validation mismatch.',
    },
  }
  record.comparison.platform_fingerprint = platformFingerprint(record)
  record.comparison.baseline_fingerprint = record.comparison.platform_fingerprint
  return { root, record, custodyPath, custodyRecord, comparisonPath, comparisonRecord }
}

function rewritePublicPairCustody(fixture, custody, { updateCandidate = true } = {}) {
  const contents = typeof custody === 'string' ? custody : JSON.stringify(custody)
  writeFileSync(fixture.custodyPath, contents)
  const artifact = fixture.record.artifacts.find(({ path: artifactPath }) => artifactPath.endsWith('/pair-custody.json'))
  artifact.bytes = Buffer.byteLength(contents)
  artifact.sha256 = sha256(contents)
  if (updateCandidate) fixture.record.candidate.pair_custody_sha256 = artifact.sha256
}

function rewritePublicPairComparison(fixture, comparison) {
  const contents = typeof comparison === 'string' ? comparison : JSON.stringify(comparison)
  writeFileSync(fixture.comparisonPath, contents)
  const artifact = fixture.record.artifacts.find(({ path: artifactPath }) => artifactPath.endsWith('/pair-comparison.json'))
  artifact.bytes = Buffer.byteLength(contents)
  artifact.sha256 = sha256(contents)
}

function rewriteBoundPublicPairComparison(fixture, comparison) {
  const contents = typeof comparison === 'string' ? comparison : JSON.stringify(comparison)
  rewritePublicPairComparison(fixture, contents)
  const custody = structuredClone(fixture.custodyRecord)
  custody.comparison_sha256 = sha256(contents)
  rewritePublicPairCustody(fixture, custody)
  fixture.record.candidate.pair_comparison_sha256 = sha256(contents)
}

test('bounded_pair_custody_requires_the_current_tier_cuda_hold', () => {
  for (const { tierMiB, cudaHoldSec } of [
    { tierMiB: 1024, cudaHoldSec: 2880 },
    { tierMiB: 2048, cudaHoldSec: 3600 },
    { tierMiB: 4096, cudaHoldSec: 7920 },
  ]) {
    const fixture = wsl2PublicPairFixture({ tierMiB, condition: 'bounded' })
    try {
      assert.deepEqual(validateRecord(fixture.record, { root: fixture.root }), [], `P${tierMiB} exact CUDA hold passes`)
      const custody = structuredClone(fixture.custodyRecord)
      custody.cuda_hold_sec = cudaHoldSec - 1
      rewritePublicPairCustody(fixture, custody)
      assert.match(
        validateRecord(fixture.record, { root: fixture.root }).join('\n'),
        /pair-custody-cuda-hold/,
        `P${tierMiB} tampered CUDA hold is refused`,
      )
    } finally {
      rmSync(fixture.root, { recursive: true, force: true })
    }
  }
})

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
  mkdirSync(path.join(root, 'docs', 'marketing'), { recursive: true })
  writeFileSync(path.join(root, 'README.md'), '# RamShared\n')
  writeFileSync(path.join(root, 'README.pt-BR.md'), '# RamShared\n')
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
  writeFileSync(
    path.join(root, 'docs', 'benchmarks', 'public-claims.json'),
    `${JSON.stringify({ schema_version: 'ramshared-public-benchmark-claims/v1', entries: [] })}\n`,
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

test('rejects_symlinked_or_oversized_artifacts_without_following_them', () => {
  const root = fixtureRepo()
  const evidence = path.join(root, 'evidence')
  mkdirSync(evidence)
  const target = path.join(evidence, 'target.json')
  const artifact = path.join(evidence, 'result.json')
  writeFileSync(target, '{}')
  symlinkSync(target, artifact)
  const linked = validRecord({ artifacts: [{ path: 'evidence/result.json', bytes: 2, sha256: SHA }] })
  assert.match(validateRecord(linked, { root }).join('\n'), /artifact-symlink/)

  rmSync(artifact)
  writeFileSync(artifact, 'x'.repeat(8 * 1024 * 1024 + 1))
  const large = validRecord({ artifacts: [{ path: 'evidence/result.json', bytes: 8 * 1024 * 1024 + 1, sha256: SHA }] })
  assert.match(validateRecord(large, { root }).join('\n'), /artifact-byte-limit/)
})

test('rejects_artifact_inventory_exhaustion_and_symlinked_registry_files', () => {
  const record = validRecord({ artifacts: Array.from({ length: 129 }, () => ({ path: 'artifact.json', bytes: 0, sha256: SHA })) })
  assert.match(validateRecord(record, { root: process.cwd() }).join('\n'), /artifact-count-limit/)

  const root = fixtureRepo()
  const results = path.join(root, 'docs', 'benchmarks', 'results.jsonl')
  const target = path.join(root, 'docs', 'benchmarks', 'target.jsonl')
  writeFileSync(target, readFileSync(results))
  rmSync(results)
  symlinkSync(target, results)
  assert.match(validateRepository({ root }).findings.join('\n'), /results-symlink/)
})

test('rejects_artifact_traversal_and_malformed_jsonl_without_echoing_contents', () => {
  const traversal = validRecord({ artifacts: [{ path: '../private.json', bytes: 0, sha256: SHA }] })
  assert.match(validateRecord(traversal, { root: process.cwd() }).join('\n'), /artifact-path/)

  const root = fixtureRepo()
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'results.jsonl'), '{private-token}\n')
  const findings = validateRepository({ root }).findings.join('\n')
  assert.match(findings, /jsonl-parse:1/)
  assert.doesNotMatch(findings, /private-token/)
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

  const malformed = validRecord()
  malformed.metrics = { latency_ms: { unit: 'ms', samples: [] } }
  assert.match(validateRecord(malformed, { root: process.cwd() }).join('\n'), /metric-samples:latency_ms/)
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

test('public_numeric_claim_requires_benchmark_identity', () => {
  const root = fixtureRepo()
  const claim = '# Performance\np99 ~9 ms\n'
  writeFileSync(path.join(root, 'README.md'), claim)
  assert.match(validateRepository({ root }).findings.join('\n'), /public-claim-unqualified/)

  writeFileSync(path.join(root, 'docs', 'benchmarks', 'public-claims.json'), JSON.stringify({
    schema_version: 'ramshared-public-benchmark-claims/v1',
    entries: [{
      id: 'qualified-readme-fixture', path: 'README.md', file_sha256: sha256(claim),
      disposition: 'qualified', benchmark_id: 'fixture-benchmark', claims: ['p99 ~9 ms'],
      reason: 'Fixture claim is bound to the qualified benchmark record.',
    }],
  }))
  assert.deepEqual(validateRepository({ root }).findings, [])
})

test('legacy_unqualified_public_claim_is_exactly_bound', () => {
  const root = fixtureRepo()
  const claim = '# Historical\nlegacy p99 ~9 ms\n'
  writeFileSync(path.join(root, 'README.pt-BR.md'), claim)
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'public-claims.json'), JSON.stringify({
    schema_version: 'ramshared-public-benchmark-claims/v1',
    entries: [{
      id: 'legacy-readme-fixture', path: 'README.pt-BR.md', file_sha256: sha256(claim),
      disposition: 'legacy-unqualified', benchmark_id: null, claims: ['legacy p99 ~9 ms'],
      reason: 'Fixture is explicitly historical and cannot promote a benchmark.',
    }],
  }))
  assert.deepEqual(validateRepository({ root }).findings, [])
})

test('changed_legacy_surface_hash_fails', () => {
  const root = fixtureRepo()
  const claim = '# Historical\nlegacy p99 ~9 ms\n'
  writeFileSync(path.join(root, 'README.md'), claim)
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'public-claims.json'), JSON.stringify({
    schema_version: 'ramshared-public-benchmark-claims/v1',
    entries: [{
      id: 'legacy-readme-fixture', path: 'README.md', file_sha256: sha256(claim),
      disposition: 'legacy-unqualified', benchmark_id: null, claims: ['legacy p99 ~9 ms'],
      reason: 'Fixture is explicitly historical and cannot promote a benchmark.',
    }],
  }))
  writeFileSync(path.join(root, 'README.md'), `${claim}Editorial change.\n`)
  assert.match(validateRepository({ root }).findings.join('\n'), /public-claim-surface-hash/)
})

test('public_scanner_detects_contextual_numeric_claims_in_english_portuguese_and_svg', () => {
  const root = fixtureRepo()
  const surfaces = [
    {
      id: 'english-contextual-claims',
      path: 'README.md',
      text: '# Efficiency\nLeaves all 8 host CPU cores available.\n6.5 times faster.\n',
      claims: ['Leaves all 8 host CPU cores available.', '6.5 times faster.'],
    },
    {
      id: 'portuguese-contextual-claims',
      path: 'README.pt-BR.md',
      text: '# Eficiência\nMantém os 8 núcleos da CPU disponíveis.\n6,5 vezes mais rápido.\nEconomia de CPU de 81 por cento.\n',
      claims: ['Mantém os 8 núcleos da CPU disponíveis.', '6,5 vezes mais rápido.', 'Economia de CPU de 81 por cento.'],
    },
    {
      id: 'svg-contextual-claims',
      path: 'docs/marketing/efficiency.svg',
      text: '<svg><text><tspan>81 percent CPU savings</tspan></text><text>Leaves host CPU cores 100% available</text></svg>\n',
      claims: ['81 percent CPU savings', 'Leaves host CPU cores 100% available'],
    },
  ]
  for (const surface of surfaces) writeFileSync(path.join(root, surface.path), surface.text)
  writeFileSync(path.join(root, 'docs', 'benchmarks', 'public-claims.json'), JSON.stringify({
    schema_version: 'ramshared-public-benchmark-claims/v1',
    entries: surfaces.map((surface) => ({
      id: surface.id,
      path: surface.path,
      file_sha256: sha256(surface.text),
      disposition: 'legacy-unqualified',
      benchmark_id: null,
      claims: surface.claims,
      reason: 'Contextual numeric fixture has no qualified benchmark identity and cannot promote.',
    })),
  }))
  assert.deepEqual(validateRepository({ root }).findings, [])
})

test('public_scanner_does_not_treat_inventory_versions_or_repeat_counts_as_performance_claims', () => {
  const root = fixtureRepo()
  writeFileSync(path.join(root, 'README.md'), '# Inventory\nWindows 11 on RTX 2060 with 6 GB VRAM.\nThe host has 8 CPU cores and 32 GiB RAM.\nRun the probe 3 times.\n')
  writeFileSync(path.join(root, 'README.pt-BR.md'), '# Inventário\nHost com 8 núcleos de CPU e 32 GiB de RAM.\nExecute o teste 3 vezes.\n')
  writeFileSync(path.join(root, 'docs', 'marketing', 'inventory.svg'), '<svg><text>Build 26200 · 4 logical CPUs · GPU 0</text></svg>\n')
  assert.deepEqual(validateRepository({ root }).findings, [])
})

test('repository_cpu_efficiency_artwork_is_explicitly_legacy_unqualified', () => {
  const registry = JSON.parse(readFileSync(new URL('../../docs/benchmarks/public-claims.json', import.meta.url), 'utf8'))
  const entry = registry.entries.find((item) => item.path === 'docs/marketing/benchmark-comparison.svg')
  assert.equal(entry.disposition, 'legacy-unqualified')
  assert.equal(entry.benchmark_id, null)
  assert.ok(entry.claims.includes('81% CPU Savings'))
  assert.ok(entry.claims.includes('Leaves host CPU cores 100% available'))
})

test('repository_public_scanner_covers_readmes_and_svg_surfaces', () => {
  const result = validateRepository({ root: process.cwd() })
  assert.equal(result.ok, true, result.findings.join('\n'))
  assert.ok(result.counts.public_surfaces >= 5)
  assert.ok(result.counts.public_claims >= 18)
})

test('public_pair_evidence_matches_repository_validator_fixture', () => {
  const fixture = wsl2PublicPairFixture()
  try {
    assert.deepEqual(validateRecord(fixture.record, { root: fixture.root }), [])

    writeFileSync(fixture.custodyPath, '{"tampered":true}')
    assert.match(validateRecord(fixture.record, { root: fixture.root }).join('\n'), /artifact-hash/)

    const invalidLifecycle = structuredClone(fixture.record)
    invalidLifecycle.lifecycle.binary_match = false
    assert.match(validateRecord(invalidLifecycle, { root: fixture.root }).join('\n'), /lifecycle-incomplete/)

    const invalidPromotion = structuredClone(fixture.record)
    invalidPromotion.decision = { ...invalidPromotion.decision, verdict: 'PASS', promotable: true }
    assert.match(validateRecord(invalidPromotion, { root: fixture.root }).join('\n'), /pass-not-qualified/)
  } finally {
    rmSync(fixture.root, { recursive: true, force: true })
  }
})

test('public_pair_custody_requires_exact_schema_and_cross_binding', () => {
  const cases = [
    {
      name: 'empty custody',
      mutate: () => ({}),
      finding: /pair-custody-schema/,
    },
    {
      name: 'missing disk cell',
      mutate: (custody) => ({ ...custody, cells: [custody.cells[1]] }),
      finding: /pair-custody-cell-count/,
    },
    {
      name: 'missing nbd cell',
      mutate: (custody) => ({ ...custody, cells: [custody.cells[0]] }),
      finding: /pair-custody-cell-count/,
    },
    {
      name: 'missing inventory custody marker',
      mutate: (custody) => {
        delete custody.cells[0].artifact_inventory_sha256
        return custody
      },
      finding: /pair-custody-cell-schema/,
    },
    {
      name: 'invalid inventory custody marker',
      mutate: (custody) => {
        custody.cells[0].artifact_inventory_sha256 = 'A'.repeat(64)
        return custody
      },
      finding: /pair-custody-cell-hash/,
    },
    {
      name: 'missing internal envelope marker',
      mutate: (custody) => {
        delete custody.cells[1].internal_envelope_sha256
        return custody
      },
      finding: /pair-custody-cell-schema/,
    },
    {
      name: 'invalid internal envelope marker',
      mutate: (custody) => {
        custody.cells[1].internal_envelope_sha256 = 'A'.repeat(64)
        return custody
      },
      finding: /pair-custody-cell-hash/,
    },
    {
      name: 'extra custody property',
      mutate: (custody) => ({ ...custody, unreviewed: true }),
      finding: /pair-custody-schema/,
    },
    {
      name: 'wrong pair identifier',
      mutate: (custody) => ({ ...custody, pair_id: '2048-idle' }),
      finding: /pair-custody-pair-binding/,
    },
    {
      name: 'wrong nbd mode',
      mutate: (custody) => {
        custody.cells[1].mode = 'disk-only'
        return custody
      },
      finding: /pair-custody-cell-contract/,
    },
    {
      name: 'wrong lifecycle context marker',
      mutate: (custody) => {
        custody.cells[1].context_sha256 = 'e'.repeat(64)
        return custody
      },
      finding: /pair-custody-lifecycle-binding/,
    },
    {
      name: 'invalid comparison marker',
      mutate: (custody) => ({ ...custody, comparison_sha256: 'A'.repeat(64) }),
      finding: /pair-custody-comparison-hash/,
    },
  ]

  for (const { name, mutate, finding } of cases) {
    const fixture = wsl2PublicPairFixture()
    try {
      const custody = mutate(structuredClone(fixture.custodyRecord))
      rewritePublicPairCustody(fixture, custody)
      assert.match(validateRecord(fixture.record, { root: fixture.root }).join('\n'), finding, name)
    } finally {
      rmSync(fixture.root, { recursive: true, force: true })
    }
  }

  const duplicateFixture = wsl2PublicPairFixture()
  try {
    const duplicate = JSON.stringify(duplicateFixture.custodyRecord)
      .replace('"pair_id":"1024-idle"', '"pair_id":"1024-idle","pair_id":"1024-idle"')
    rewritePublicPairCustody(duplicateFixture, duplicate)
    assert.match(
      validateRecord(duplicateFixture.record, { root: duplicateFixture.root }).join('\n'),
      /pair-custody-duplicate-key/,
      'duplicate custody property',
    )
  } finally {
    rmSync(duplicateFixture.root, { recursive: true, force: true })
  }

  const candidateFixture = wsl2PublicPairFixture()
  try {
    const custody = structuredClone(candidateFixture.custodyRecord)
    custody.cleanup.terminal_state = 'UNVERIFIED'
    rewritePublicPairCustody(candidateFixture, custody, { updateCandidate: false })
    assert.match(
      validateRecord(candidateFixture.record, { root: candidateFixture.root }).join('\n'),
      /pair-custody-candidate-hash/,
      'candidate custody hash mismatch',
    )
  } finally {
    rmSync(candidateFixture.root, { recursive: true, force: true })
  }

  const comparisonFixture = wsl2PublicPairFixture()
  try {
    const comparison = structuredClone(comparisonFixture.comparisonRecord)
    comparison.pair_id = '2048-idle'
    rewritePublicPairComparison(comparisonFixture, comparison)
    assert.match(
      validateRecord(comparisonFixture.record, { root: comparisonFixture.root }).join('\n'),
      /pair-comparison-pair-binding/,
      'comparison pair identifier',
    )
  } finally {
    rmSync(comparisonFixture.root, { recursive: true, force: true })
  }
})

test('public_pair_custody_binds_exact_comparison_artifact_and_candidate_hash', () => {
  const fixture = wsl2PublicPairFixture()
  try {
    const comparison = structuredClone(fixture.comparisonRecord)
    comparison.nbd_vs_disk_median_ratio = 1.2
    rewritePublicPairComparison(fixture, comparison)
    assert.match(
      validateRecord(fixture.record, { root: fixture.root }).join('\n'),
      /pair-custody-comparison-binding/,
      'pair custody must bind the exact comparison artifact bytes',
    )
  } finally {
    rmSync(fixture.root, { recursive: true, force: true })
  }

  const candidateFixture = wsl2PublicPairFixture()
  try {
    candidateFixture.record.candidate.pair_comparison_sha256 = 'b'.repeat(64)
    assert.match(
      validateRecord(candidateFixture.record, { root: candidateFixture.root }).join('\n'),
      /pair-custody-candidate-comparison-binding/,
      'candidate hash must bind the exact comparison artifact bytes',
    )
  } finally {
    rmSync(candidateFixture.root, { recursive: true, force: true })
  }
})

test('public_pair_custody_refuses_missing_and_semantically_invalid_artifact_bindings', () => {
  const cases = [
    {
      name: 'candidate comparison hash format',
      mutate: (fixture) => { fixture.record.candidate.pair_comparison_sha256 = 'A'.repeat(64) },
      finding: /pair-custody-candidate-schema/,
    },
    {
      name: 'repository artifact root',
      mutate: (fixture) => { fixture.record.candidate.repository_artifact_root += '-other' },
      finding: /pair-custody-artifact-schema/,
    },
    {
      name: 'missing comparison artifact',
      mutate: (fixture) => { rmSync(fixture.comparisonPath) },
      finding: /pair-custody-artifact-missing/,
    },
    {
      name: 'release source binding',
      mutate: (fixture) => {
        const custody = structuredClone(fixture.custodyRecord)
        custody.release.source_commit = 'f'.repeat(40)
        rewritePublicPairCustody(fixture, custody)
      },
      finding: /pair-custody-release-binding/,
    },
    {
      name: 'pair timeout budget',
      mutate: (fixture) => {
        const custody = structuredClone(fixture.custodyRecord)
        custody.timeout_budget.cell.sample_timeout_sec = 121
        rewritePublicPairCustody(fixture, custody)
      },
      finding: /pair-custody-timeout-budget/,
    },
    {
      name: 'idle pair cuda hold custody',
      mutate: (fixture) => {
        const custody = structuredClone(fixture.custodyRecord)
        custody.cuda_hold_sec = 2160
        rewritePublicPairCustody(fixture, custody)
      },
      finding: /pair-custody-cuda-hold/,
    },
    {
      name: 'comparison JSON syntax',
      mutate: (fixture) => { rewritePublicPairComparison(fixture, '{"truncated":') },
      finding: /pair-comparison-json/,
    },
    {
      name: 'comparison exact schema',
      mutate: (fixture) => {
        rewriteBoundPublicPairComparison(fixture, { ...fixture.comparisonRecord, unreviewed: true })
      },
      finding: /pair-comparison-schema/,
    },
    {
      name: 'comparison numeric contract',
      mutate: (fixture) => {
        rewriteBoundPublicPairComparison(fixture, {
          ...fixture.comparisonRecord,
          nbd_vs_disk_p99_ratio: 0,
        })
      },
      finding: /pair-comparison-schema/,
    },
  ]

  for (const { name, mutate, finding } of cases) {
    const fixture = wsl2PublicPairFixture()
    try {
      mutate(fixture)
      assert.match(validateRecord(fixture.record, { root: fixture.root }).join('\n'), finding, name)
    } finally {
      rmSync(fixture.root, { recursive: true, force: true })
    }
  }
})

test('public_pair_comparison_recomputes_exact_rounded_ratios_from_public_metrics', () => {
  const ratioMutations = [
    { name: 'median', key: 'nbd_vs_disk_median_ratio', value: 999 },
    { name: 'p99', key: 'nbd_vs_disk_p99_ratio', value: 999 },
    { name: 'population stddev', key: 'nbd_vs_disk_population_stddev_ratio', value: 999 },
  ]

  for (const { name, key, value } of ratioMutations) {
    const fixture = wsl2PublicPairFixture()
    try {
      const comparison = { ...fixture.comparisonRecord, [key]: value }
      rewriteBoundPublicPairComparison(fixture, comparison)
      assert.match(
        validateRecord(fixture.record, { root: fixture.root }).join('\n'),
        /pair-comparison-ratio/,
        `${name} ratio forged after exact byte binding`,
      )
    } finally {
      rmSync(fixture.root, { recursive: true, force: true })
    }
  }

  const zeroStddev = wsl2PublicPairFixture()
  try {
    zeroStddev.record.metrics.disk_allocation_to_hold_ms = publicMetric([100, 100, 100])
    zeroStddev.record.metrics.nbd_allocation_to_hold_ms = publicMetric([105, 105, 105])
    rewriteBoundPublicPairComparison(zeroStddev, {
      ...zeroStddev.comparisonRecord,
      nbd_vs_disk_median_ratio: 1.05,
      nbd_vs_disk_p99_ratio: 1.05,
      nbd_vs_disk_population_stddev_ratio: null,
    })
    assert.deepEqual(validateRecord(zeroStddev.record, { root: zeroStddev.root }), [])

    rewriteBoundPublicPairComparison(zeroStddev, {
      ...zeroStddev.comparisonRecord,
      nbd_vs_disk_median_ratio: 1.05,
      nbd_vs_disk_p99_ratio: 1.05,
      nbd_vs_disk_population_stddev_ratio: 1,
    })
    assert.match(
      validateRecord(zeroStddev.record, { root: zeroStddev.root }).join('\n'),
      /pair-comparison-ratio/,
      'zero disk standard deviation requires null comparison ratio',
    )
  } finally {
    rmSync(zeroStddev.root, { recursive: true, force: true })
  }

  const midpoint = wsl2PublicPairFixture()
  try {
    midpoint.record.metrics.disk_allocation_to_hold_ms = publicMetric([1, 1, 1])
    midpoint.record.metrics.nbd_allocation_to_hold_ms = publicMetric([1.2345665, 1.2345665, 1.2345665])
    rewriteBoundPublicPairComparison(midpoint, {
      ...midpoint.comparisonRecord,
      nbd_vs_disk_median_ratio: 1.234566,
      nbd_vs_disk_p99_ratio: 1.234566,
      nbd_vs_disk_population_stddev_ratio: null,
    })
    assert.deepEqual(validateRecord(midpoint.record, { root: midpoint.root }), [])

    rewriteBoundPublicPairComparison(midpoint, {
      ...midpoint.comparisonRecord,
      nbd_vs_disk_median_ratio: 1.234567,
      nbd_vs_disk_p99_ratio: 1.234567,
      nbd_vs_disk_population_stddev_ratio: null,
    })
    assert.match(
      validateRecord(midpoint.record, { root: midpoint.root }).join('\n'),
      /pair-comparison-ratio/,
      'six-decimal midpoint uses the controller’s to-even rounding',
    )
  } finally {
    rmSync(midpoint.root, { recursive: true, force: true })
  }
})

test('public_pair_comparison_accepts_exact_zero_stddev_ratio_only_when_disk_varies', () => {
  const fixture = wsl2PublicPairFixture()
  try {
    fixture.record.metrics.disk_allocation_to_hold_ms = publicMetric([100, 110, 120])
    fixture.record.metrics.nbd_allocation_to_hold_ms = publicMetric([105, 105, 105])
    const validComparison = {
      ...fixture.comparisonRecord,
      nbd_vs_disk_median_ratio: 0.954545,
      nbd_vs_disk_p99_ratio: 0.875,
      nbd_vs_disk_population_stddev_ratio: 0,
    }
    rewriteBoundPublicPairComparison(fixture, validComparison)
    assert.deepEqual(validateRecord(fixture.record, { root: fixture.root }), [])

    for (const [name, ratio] of [
      ['null', null],
      ['negative', -0.1],
      ['wrong positive', 1],
    ]) {
      rewriteBoundPublicPairComparison(fixture, {
        ...validComparison,
        nbd_vs_disk_population_stddev_ratio: ratio,
      })
      assert.match(
        validateRecord(fixture.record, { root: fixture.root }).join('\n'),
        /pair-comparison-(?:schema|ratio)/,
        `disk variation with ${name} stddev ratio must fail closed`,
      )
    }

    const nonfiniteComparison = JSON.stringify(validComparison)
      .replace('"nbd_vs_disk_population_stddev_ratio":0', '"nbd_vs_disk_population_stddev_ratio":1e999')
    rewriteBoundPublicPairComparison(fixture, nonfiniteComparison)
    assert.match(
      validateRecord(fixture.record, { root: fixture.root }).join('\n'),
      /pair-comparison-schema/,
      'nonfinite stddev ratio must fail closed',
    )
  } finally {
    rmSync(fixture.root, { recursive: true, force: true })
  }

  const zeroDisk = wsl2PublicPairFixture()
  try {
    zeroDisk.record.metrics.disk_allocation_to_hold_ms = publicMetric([100, 100, 100])
    zeroDisk.record.metrics.nbd_allocation_to_hold_ms = publicMetric([105, 110, 115])
    rewriteBoundPublicPairComparison(zeroDisk, {
      ...zeroDisk.comparisonRecord,
      nbd_vs_disk_median_ratio: 1.05,
      nbd_vs_disk_p99_ratio: 1.045455,
      nbd_vs_disk_population_stddev_ratio: 0,
    })
    assert.match(
      validateRecord(zeroDisk.record, { root: zeroDisk.root }).join('\n'),
      /pair-comparison-(?:schema|ratio)/,
      'zero disk standard deviation requires null comparison ratio',
    )
  } finally {
    rmSync(zeroDisk.root, { recursive: true, force: true })
  }
})

test('public_pair_baseline_verdict_requires_the_exact_public_decision_mapping', () => {
  const mappings = {
    BASELINE_CANDIDATE: { verdict: 'BASELINE', promotable: false, qualified: false, baselineFingerprint: 'platform' },
    NOT_COMPARABLE: { verdict: 'INCOMPARABLE', promotable: false, qualified: false, baselineFingerprint: 'unavailable' },
    GREEN: { verdict: 'PASS', promotable: true, qualified: true, baselineFingerprint: 'platform' },
    YELLOW: { verdict: 'YELLOW', promotable: false, qualified: true, baselineFingerprint: 'platform' },
    RED: { verdict: 'RED', promotable: false, qualified: true, baselineFingerprint: 'platform' },
  }

  for (const [baselineVerdict, expected] of Object.entries(mappings)) {
    const fixture = wsl2PublicPairFixture()
    try {
      const comparison = { ...fixture.comparisonRecord, baseline_verdict: baselineVerdict }
      rewriteBoundPublicPairComparison(fixture, comparison)
      fixture.record.comparison.baseline_verdict = baselineVerdict
      fixture.record.comparison.qualified = expected.qualified
      fixture.record.comparison.baseline_fingerprint = expected.baselineFingerprint === 'unavailable'
        ? 'unavailable'
        : fixture.record.comparison.platform_fingerprint
      fixture.record.decision = {
        ...fixture.record.decision,
        verdict: expected.verdict,
        promotable: expected.promotable,
      }
      assert.deepEqual(
        validateRecord(fixture.record, { root: fixture.root }),
        [],
        `${baselineVerdict} valid public decision mapping`,
      )

      fixture.record.decision = {
        ...fixture.record.decision,
        verdict: baselineVerdict === 'GREEN' ? 'RED' : 'PASS',
        promotable: baselineVerdict === 'GREEN' ? false : true,
      }
      fixture.record.comparison.qualified = baselineVerdict === 'GREEN' ? false : true
      assert.match(
        validateRecord(fixture.record, { root: fixture.root }).join('\n'),
        /pair-comparison-decision-mapping/,
        `${baselineVerdict} cannot forge a promotable PASS`,
      )
    } finally {
      rmSync(fixture.root, { recursive: true, force: true })
    }
  }
})
