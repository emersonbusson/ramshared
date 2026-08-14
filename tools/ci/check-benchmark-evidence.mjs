#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { lstatSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const MAX_FILE_BYTES = 8 * 1024 * 1024
const MAX_RECORDS = 10000
const MAX_ARTIFACTS = 128
const MAX_ARTIFACT_TOTAL_BYTES = 64 * 1024 * 1024
const SHA256_RE = /^[0-9a-f]{64}$/i
const VERDICTS = new Set(['RED', 'YELLOW', 'INCOMPARABLE', 'BASELINE', 'PASS'])

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`
  }
  return JSON.stringify(value)
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function finiteNumbers(samples) {
  return Array.isArray(samples) && samples.length > 0 && samples.every(Number.isFinite)
}

export function computeStats(samples) {
  if (!finiteNumbers(samples)) throw new Error('samples must be a non-empty finite-number array')
  const ordered = [...samples].sort((a, b) => a - b)
  const n = ordered.length
  const middle = Math.floor(n / 2)
  const median = n % 2 === 0 ? (ordered[middle - 1] + ordered[middle]) / 2 : ordered[middle]
  const mean = ordered.reduce((sum, value) => sum + value, 0) / n
  const variance = ordered.reduce((sum, value) => sum + ((value - mean) ** 2), 0) / n
  return {
    n,
    median,
    p99_nearest_rank: ordered[Math.max(0, Math.ceil(0.99 * n) - 1)],
    stddev: Math.sqrt(variance),
    min: ordered[0],
    max: ordered[n - 1],
  }
}

function fingerprintFields(record) {
  return {
    schema_version: record.schema_version,
    harness_revision: record.source?.harness_revision,
    platform: record.platform,
    workload: record.workload,
  }
}

export function platformFingerprint(record) {
  return sha256(canonical(fingerprintFields(record)))
}

function sensitiveRule(value) {
  const text = canonical(value)
  const rules = [
    /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/i,
    /(?:api[-_ ]?key|token|password|credential)\s*(?:=|:|\s)\s*["']?[^\s"',}]+/i,
    /(?:^|["'\s])\/home\/[A-Za-z0-9._-]+\//,
    /C:\\Users\\[^\\\s"']+/i,
    /\\\\wsl(?:\.localhost|\$)\\[^\\]+\\home\\[^\\]+/i,
    /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i,
    /\b(?:ffff|0000)[0-9a-f]{12,}\b/i,
  ]
  return rules.some((rule) => rule.test(text))
}

function resolveRepositoryFile(root, rel, findings, rule) {
  if (typeof rel !== 'string' || !rel || path.isAbsolute(rel) || /^[A-Za-z]:[\\/]/.test(rel) || rel.split(/[\\/]/).includes('..')) {
    findings.push(`${rule}-path`)
    return null
  }
  const resolvedRoot = path.resolve(root)
  const full = path.resolve(resolvedRoot, rel)
  if (!full.startsWith(`${resolvedRoot}${path.sep}`)) {
    findings.push(`${rule}-path`)
    return null
  }
  let stat
  try { stat = lstatSync(full) } catch { findings.push(`${rule}-missing`); return null }
  if (stat.isSymbolicLink()) { findings.push(`${rule}-symlink`); return null }
  if (!stat.isFile()) { findings.push(`${rule}-not-file`); return null }
  return { full, stat }
}

function readArtifact(file, findings) {
  if (file.stat.size > MAX_FILE_BYTES) {
    findings.push('artifact-byte-limit')
    return null
  }
  try { return readFileSync(file.full) } catch { findings.push('artifact-read'); return null }
}

function readRepositoryFile(file, findings, rule) {
  let stat
  try { stat = lstatSync(file) } catch { findings.push(`missing-${rule}`); return null }
  if (stat.isSymbolicLink()) { findings.push(`${rule}-symlink`); return null }
  if (!stat.isFile()) { findings.push(`${rule}-not-file`); return null }
  if (stat.size > MAX_FILE_BYTES) { findings.push(`${rule}-byte-limit`); return null }
  try { return readFileSync(file, 'utf8') } catch { findings.push(`${rule}-read`); return null }
}

function requiredObject(record, key, findings) {
  if (!record[key] || typeof record[key] !== 'object' || Array.isArray(record[key])) {
    findings.push(`required-${key}`)
    return false
  }
  return true
}

function almostEqual(a, b) {
  return Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) <= 1e-9 * Math.max(1, Math.abs(a), Math.abs(b))
}

function roundControllerRatio(value, allowZero = false) {
  if (!Number.isFinite(value) || value < 0 || (!allowZero && value === 0)) return null
  const scale = 1_000_000
  const scaled = value * scale
  if (!Number.isFinite(scaled)) return null
  const lower = Math.floor(scaled)
  const fractional = scaled - lower
  const rounded = fractional > 0.5
    ? lower + 1
    : fractional < 0.5
      ? lower
      : lower % 2 === 0
        ? lower
        : lower + 1
  return rounded / scale
}

function expectedPublicPairRatios(metrics) {
  const disk = metrics?.disk_allocation_to_hold_ms
  const nbd = metrics?.nbd_allocation_to_hold_ms
  const diskMedian = disk?.median
  const diskP99 = disk?.p99_nearest_rank
  const diskStddev = disk?.stddev
  const nbdMedian = nbd?.median
  const nbdP99 = nbd?.p99_nearest_rank
  const nbdStddev = nbd?.stddev
  if (![diskMedian, diskP99, nbdMedian, nbdP99].every((value) => Number.isFinite(value) && value > 0) ||
      ![diskStddev, nbdStddev].every((value) => Number.isFinite(value) && value >= 0)) return null
  const median = roundControllerRatio(nbdMedian / diskMedian)
  const p99 = roundControllerRatio(nbdP99 / diskP99)
  const stddev = diskStddev > 0 ? roundControllerRatio(nbdStddev / diskStddev, true) : null
  if (median === null || p99 === null || (diskStddev > 0 && stddev === null)) return null
  return {
    nbd_vs_disk_median_ratio: median,
    nbd_vs_disk_p99_ratio: p99,
    nbd_vs_disk_population_stddev_ratio: stddev,
  }
}

const PAIR_CUSTODY_KEYS = [
  'schema_version', 'pair_id', 'release', 'cells', 'timeout_budget', 'cuda_hold_sec', 'comparison_sha256', 'cleanup',
]
const PAIR_CELL_KEYS = [
  'mode', 'binary_match', 'context_sha256', 'summary_sha256', 'artifact_inventory_sha256',
  'internal_envelope_sha256', 'timeout_budget',
]
const PAIR_RELEASE_KEYS = [
  'version', 'source_commit', 'installed_manifest_sha256', 'input_bundle_manifest_sha256',
]
const PAIR_CLEANUP_KEYS = ['complete', 'terminal_state']
const PAIR_TIMEOUT_KEYS = ['cell', 'cuda_hold_min_sec']
const CELL_TIMEOUT_KEYS = [
  'sample_timeout_sec', 'integrity_finalization_timeout_sec', 'samples',
  'setup_cleanup_timeout_sec', 'cell_outer_timeout_sec',
]
const PAIR_COMPARISON_KEYS = [
  'schema_version', 'pair_id', 'environment_fingerprint', 'baseline_verdict', 'baseline_reason',
  'nbd_vs_disk_median_ratio', 'nbd_vs_disk_p99_ratio', 'nbd_vs_disk_population_stddev_ratio',
  'timeout_budget',
]
const PAIR_CANDIDATE_KEYS = [
  'classification', 'canonical', 'publication_state', 'repository_artifact_root',
  'installed_manifest_sha256', 'input_bundle_manifest_sha256', 'pair_custody_sha256',
  'pair_comparison_sha256',
]
const LOWER_SHA256_RE = /^[0-9a-f]{64}$/
const LOWER_COMMIT_RE = /^[0-9a-f]{40}$/
const PAIR_BASELINE_VERDICTS = new Set(['BASELINE_CANDIDATE', 'NOT_COMPARABLE', 'GREEN', 'YELLOW', 'RED'])
const PAIR_PUBLIC_DECISIONS = {
  BASELINE_CANDIDATE: { verdict: 'BASELINE', promotable: false, qualified: false },
  NOT_COMPARABLE: { verdict: 'INCOMPARABLE', promotable: false, qualified: false },
  GREEN: { verdict: 'PASS', promotable: true, qualified: true },
  YELLOW: { verdict: 'YELLOW', promotable: false, qualified: true },
  RED: { verdict: 'RED', promotable: false, qualified: true },
}

function plainObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function exactObject(value, keys) {
  if (!plainObject(value)) return false
  const actual = Object.keys(value)
  return actual.length === keys.length && keys.every((key) => Object.hasOwn(value, key))
}

function lowerSha256(value) {
  return typeof value === 'string' && LOWER_SHA256_RE.test(value)
}

function safeArtifactRoot(value) {
  return typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9._-]*(?:\/[A-Za-z0-9][A-Za-z0-9._-]*)*$/.test(value)
}

function pairBudgetForTier(tierMiB) {
  const cells = {
    1024: { sample_timeout_sec: 120, integrity_finalization_timeout_sec: 120, samples: 3, setup_cleanup_timeout_sec: 300, cell_outer_timeout_sec: 1020 },
    2048: { sample_timeout_sec: 240, integrity_finalization_timeout_sec: 240, samples: 3, setup_cleanup_timeout_sec: 300, cell_outer_timeout_sec: 1740 },
    4096: { sample_timeout_sec: 600, integrity_finalization_timeout_sec: 600, samples: 3, setup_cleanup_timeout_sec: 300, cell_outer_timeout_sec: 3900 },
  }
  const cell = cells[tierMiB]
  return cell ? { cell, cuda_hold_min_sec: (2 * cell.cell_outer_timeout_sec) + 120 } : null
}

function matchesBudget(value, expected) {
  if (!exactObject(value, PAIR_TIMEOUT_KEYS) || !exactObject(value.cell, CELL_TIMEOUT_KEYS) ||
      !Number.isSafeInteger(value.cuda_hold_min_sec) || value.cuda_hold_min_sec !== expected.cuda_hold_min_sec) return false
  return CELL_TIMEOUT_KEYS.every((key) => Number.isSafeInteger(value.cell[key]) && value.cell[key] === expected.cell[key])
}

function parseStringToken(text, state) {
  const start = state.index
  state.index += 1
  while (state.index < text.length) {
    const character = text[state.index]
    state.index += 1
    if (character === '\\') {
      state.index += 1
    } else if (character === '"') {
      return JSON.parse(text.slice(start, state.index))
    }
  }
  throw new Error('unterminated JSON string')
}

function containsDuplicateJsonObjectKey(text) {
  const state = { index: 0, duplicate: false }
  const skipWhitespace = () => {
    while (state.index < text.length && /\s/.test(text[state.index])) state.index += 1
  }
  const parseValue = () => {
    skipWhitespace()
    const character = text[state.index]
    if (character === '{') return parseObject()
    if (character === '[') return parseArray()
    if (character === '"') { parseStringToken(text, state); return }
    while (state.index < text.length && !/[\s,\]}]/.test(text[state.index])) state.index += 1
  }
  const parseArray = () => {
    state.index += 1
    skipWhitespace()
    if (text[state.index] === ']') { state.index += 1; return }
    while (state.index < text.length) {
      parseValue()
      skipWhitespace()
      if (text[state.index] === ']') { state.index += 1; return }
      if (text[state.index] !== ',') throw new Error('invalid JSON array')
      state.index += 1
    }
    throw new Error('unterminated JSON array')
  }
  const parseObject = () => {
    const keys = new Set()
    state.index += 1
    skipWhitespace()
    if (text[state.index] === '}') { state.index += 1; return }
    while (state.index < text.length) {
      if (text[state.index] !== '"') throw new Error('invalid JSON object key')
      const key = parseStringToken(text, state)
      if (keys.has(key)) state.duplicate = true
      keys.add(key)
      skipWhitespace()
      if (text[state.index] !== ':') throw new Error('invalid JSON object separator')
      state.index += 1
      parseValue()
      skipWhitespace()
      if (text[state.index] === '}') { state.index += 1; return }
      if (text[state.index] !== ',') throw new Error('invalid JSON object')
      state.index += 1
      skipWhitespace()
    }
    throw new Error('unterminated JSON object')
  }
  parseValue()
  skipWhitespace()
  if (state.index !== text.length) throw new Error('trailing JSON input')
  return state.duplicate
}

function parsePairArtifact(bytes, findings, label) {
  const contents = bytes.toString('utf8')
  let value
  try {
    value = JSON.parse(contents)
  } catch {
    findings.push(`${label}-json`)
    return null
  }
  try {
    if (containsDuplicateJsonObjectKey(contents)) findings.push(`${label}-duplicate-key`)
  } catch {
    findings.push(`${label}-json`)
    return null
  }
  return value
}

function expectedWsl2Pair(record) {
  const parameters = record.workload?.parameters
  const tierMiB = parameters?.tier_mib
  const condition = parameters?.condition
  const budget = pairBudgetForTier(tierMiB)
  if (!budget || !Number.isSafeInteger(tierMiB) || !['idle', 'bounded'].includes(condition)) return null
  return { id: `${tierMiB}-${condition}`, budget }
}

function validatePairCell(cell, expectedMode, expectedBinaryMatch, expectedBudget, findings) {
  if (!exactObject(cell, PAIR_CELL_KEYS)) {
    findings.push('pair-custody-cell-schema')
    return
  }
  if (cell.mode !== expectedMode || cell.binary_match !== expectedBinaryMatch) findings.push('pair-custody-cell-contract')
  for (const name of ['context_sha256', 'summary_sha256', 'artifact_inventory_sha256', 'internal_envelope_sha256']) {
    if (!lowerSha256(cell[name])) findings.push('pair-custody-cell-hash')
  }
  if (!exactObject(cell.timeout_budget, CELL_TIMEOUT_KEYS) || !CELL_TIMEOUT_KEYS.every((key) =>
    Number.isSafeInteger(cell.timeout_budget[key]) && cell.timeout_budget[key] === expectedBudget.cell[key])) {
    findings.push('pair-custody-timeout-budget')
  }
}

function validateWsl2PairCustody(record, artifactContents, findings) {
  const expectedPair = expectedWsl2Pair(record)
  const candidate = record.candidate
  if (!expectedPair || !exactObject(candidate, PAIR_CANDIDATE_KEYS) ||
      candidate.classification !== 'candidate/noncanonical' || candidate.canonical !== false ||
      candidate.publication_state !== 'campaign-root-pending-repository-copy' ||
      !safeArtifactRoot(candidate.repository_artifact_root) || !lowerSha256(candidate.installed_manifest_sha256) ||
      !(candidate.input_bundle_manifest_sha256 === 'not_exposed' || lowerSha256(candidate.input_bundle_manifest_sha256)) ||
      !lowerSha256(candidate.pair_custody_sha256) || !lowerSha256(candidate.pair_comparison_sha256)) {
    findings.push('pair-custody-candidate-schema')
    return
  }

  const expectedRoot = `docs/specs/no-milestone/wsl2-nbd-product-readiness/evidence/${record.run_id}`
  const custodyPath = `${candidate.repository_artifact_root}/pair-custody.json`
  const comparisonPath = `${candidate.repository_artifact_root}/pair-comparison.json`
  if (candidate.repository_artifact_root !== expectedRoot || record.artifacts.length !== 2 ||
      record.artifacts[0]?.path !== custodyPath || record.artifacts[1]?.path !== comparisonPath) {
    findings.push('pair-custody-artifact-schema')
    return
  }

  const custodyBytes = artifactContents.get(custodyPath)
  const comparisonBytes = artifactContents.get(comparisonPath)
  if (!custodyBytes || !comparisonBytes) {
    findings.push('pair-custody-artifact-missing')
    return
  }
  if (sha256(custodyBytes) !== candidate.pair_custody_sha256) findings.push('pair-custody-candidate-hash')
  const comparisonHash = sha256(comparisonBytes)
  if (comparisonHash !== candidate.pair_comparison_sha256) findings.push('pair-custody-candidate-comparison-binding')

  const custody = parsePairArtifact(custodyBytes, findings, 'pair-custody')
  const comparison = parsePairArtifact(comparisonBytes, findings, 'pair-comparison')
  if (!custody || !comparison) return
  if (!exactObject(custody, PAIR_CUSTODY_KEYS) || custody.schema_version !== 'ramshared-nbd-public-pair-custody/v1') {
    findings.push('pair-custody-schema')
    return
  }
  if (custody.pair_id !== expectedPair.id) findings.push('pair-custody-pair-binding')
  if (!exactObject(custody.release, PAIR_RELEASE_KEYS) || typeof custody.release.version !== 'string' || !custody.release.version ||
      !LOWER_COMMIT_RE.test(custody.release.source_commit) || !lowerSha256(custody.release.installed_manifest_sha256) ||
      !(custody.release.input_bundle_manifest_sha256 === 'not_exposed' || lowerSha256(custody.release.input_bundle_manifest_sha256)) ||
      custody.release.source_commit !== record.source.commit ||
      custody.release.installed_manifest_sha256 !== candidate.installed_manifest_sha256 ||
      custody.release.input_bundle_manifest_sha256 !== candidate.input_bundle_manifest_sha256) {
    findings.push('pair-custody-release-binding')
  }
  if (!Array.isArray(custody.cells) || custody.cells.length !== 2) {
    findings.push('pair-custody-cell-count')
  } else {
    validatePairCell(custody.cells[0], 'disk-only', 'N/A', expectedPair.budget, findings)
    validatePairCell(custody.cells[1], 'nbd', 'PASS', expectedPair.budget, findings)
  }
  if (!matchesBudget(custody.timeout_budget, expectedPair.budget) || !matchesBudget(record.workload.parameters.timeout_budget, expectedPair.budget)) {
    findings.push('pair-custody-timeout-budget')
  }
  const expectedCudaHoldSec = record.workload.parameters.condition === 'bounded'
    ? expectedPair.budget.cuda_hold_min_sec
    : 0
  if (!Number.isSafeInteger(custody.cuda_hold_sec) || custody.cuda_hold_sec !== expectedCudaHoldSec) {
    findings.push('pair-custody-cuda-hold')
  }
  if (!lowerSha256(custody.comparison_sha256)) findings.push('pair-custody-comparison-hash')
  if (custody.comparison_sha256 !== comparisonHash) findings.push('pair-custody-comparison-binding')
  if (!exactObject(custody.cleanup, PAIR_CLEANUP_KEYS) || custody.cleanup.complete !== true || custody.cleanup.terminal_state !== 'PRODUCT_OFF') {
    findings.push('pair-custody-cleanup')
  }
  if (Array.isArray(custody.cells) && custody.cells.length === 2 &&
      (record.lifecycle.before?.custody_sha256 !== custody.cells[0].context_sha256 ||
       record.lifecycle.after?.custody_sha256 !== custody.cells[1].context_sha256 ||
       record.lifecycle.after?.terminal_state !== 'PRODUCT_OFF' ||
       record.lifecycle.action?.pair_id !== expectedPair.id ||
       canonical(record.lifecycle.action?.mode_order) !== canonical(['disk-only', 'nbd']))) {
    findings.push('pair-custody-lifecycle-binding')
  }

  if (!exactObject(comparison, PAIR_COMPARISON_KEYS) || comparison.schema_version !== 'ramshared-nbd-public-pair-comparison/v1') {
    findings.push('pair-comparison-schema')
    return
  }
  if (comparison.pair_id !== expectedPair.id || comparison.environment_fingerprint !== record.comparison.pair_environment_fingerprint ||
      comparison.baseline_verdict !== record.comparison.baseline_verdict) {
    findings.push('pair-comparison-pair-binding')
  }
  const diskStddev = record.metrics.disk_allocation_to_hold_ms?.stddev
  const stddevRatioShape = diskStddev > 0
    ? Number.isFinite(comparison.nbd_vs_disk_population_stddev_ratio) && comparison.nbd_vs_disk_population_stddev_ratio >= 0
    : comparison.nbd_vs_disk_population_stddev_ratio === null
  if (!lowerSha256(comparison.environment_fingerprint) || !PAIR_BASELINE_VERDICTS.has(comparison.baseline_verdict) ||
      typeof comparison.baseline_reason !== 'string' || !comparison.baseline_reason ||
      !Number.isFinite(comparison.nbd_vs_disk_median_ratio) || comparison.nbd_vs_disk_median_ratio <= 0 ||
      !Number.isFinite(comparison.nbd_vs_disk_p99_ratio) || comparison.nbd_vs_disk_p99_ratio <= 0 ||
      !stddevRatioShape ||
      !matchesBudget(comparison.timeout_budget, expectedPair.budget)) {
    findings.push('pair-comparison-schema')
  }
  const expectedRatios = expectedPublicPairRatios(record.metrics)
  if (!expectedRatios || comparison.nbd_vs_disk_median_ratio !== expectedRatios.nbd_vs_disk_median_ratio ||
      comparison.nbd_vs_disk_p99_ratio !== expectedRatios.nbd_vs_disk_p99_ratio ||
      comparison.nbd_vs_disk_population_stddev_ratio !== expectedRatios.nbd_vs_disk_population_stddev_ratio) {
    findings.push('pair-comparison-ratio')
  }
  const expectedDecision = PAIR_PUBLIC_DECISIONS[comparison.baseline_verdict]
  if (!expectedDecision || record.comparison.baseline_verdict !== comparison.baseline_verdict ||
      record.decision.verdict !== expectedDecision.verdict || record.decision.promotable !== expectedDecision.promotable ||
      record.comparison.qualified !== expectedDecision.qualified) {
    findings.push('pair-comparison-decision-mapping')
  }
}

export function validateRecord(record, { root = ROOT } = {}) {
  const findings = []
  if (!record || typeof record !== 'object' || Array.isArray(record)) return ['record-type']
  if (record.schema_version !== 'ramshared-evidence/v1') findings.push('schema-version')
  if (typeof record.run_id !== 'string' || !/^[a-z0-9][a-z0-9._-]{2,127}$/i.test(record.run_id)) findings.push('run-id')
  if (!requiredObject(record, 'utc', findings)) return findings
  for (const key of ['source', 'platform', 'candidate', 'workload', 'comparison', 'metrics', 'lifecycle', 'decision']) requiredObject(record, key, findings)
  if (!Array.isArray(record.artifacts)) findings.push('required-artifacts')
  if (findings.some((item) => item.startsWith('required-'))) return findings

  if (!/^[0-9a-f]{40}$/i.test(record.source.commit ?? '')) findings.push('source-commit')
  if (typeof record.source.dirty !== 'boolean') findings.push('source-dirty')
  if (!Number.isInteger(record.source.dirty_entry_count) || record.source.dirty_entry_count < 0 || (record.source.dirty && record.source.dirty_entry_count < 1) || (!record.source.dirty && record.source.dirty_entry_count !== 0)) findings.push('dirty-provenance')
  if (typeof record.source.invocation !== 'string' || record.source.invocation.length < 3 || record.source.invocation.length > 4096) findings.push('source-invocation')
  if (typeof record.source.harness_revision !== 'string' || record.source.harness_revision.length < 1) findings.push('harness-revision')
  if (!Number.isInteger(record.workload.runs) || record.workload.runs < 3) findings.push('workload-runs')

  const expectedFingerprint = platformFingerprint(record)
  if (record.comparison.platform_fingerprint !== expectedFingerprint) findings.push('platform-fingerprint')
  const fingerprintsMatch = record.comparison.baseline_fingerprint === expectedFingerprint
  if (!fingerprintsMatch && record.decision.verdict !== 'INCOMPARABLE') findings.push('comparison-verdict')
  if (record.comparison.qualified && !fingerprintsMatch) findings.push('comparison-qualified')

  const metricEntries = Object.entries(record.metrics)
  if (metricEntries.length === 0 || metricEntries.length > 256) findings.push('metric-count')
  for (const [name, metric] of metricEntries) {
    if (!metric || typeof metric !== 'object' || !finiteNumbers(metric.samples)) {
      findings.push(`metric-samples:${name}`)
      continue
    }
    const stats = computeStats(metric.samples)
    for (const field of ['n', 'median', 'p99_nearest_rank', 'stddev', 'min', 'max']) {
      if (!almostEqual(metric[field], stats[field])) findings.push(`metric-${field}:${name}`)
    }
    if (metric.n !== record.workload.runs) findings.push(`metric-run-count:${name}`)
    if (typeof metric.unit !== 'string' || !metric.unit) findings.push(`metric-unit:${name}`)
  }

  if (!VERDICTS.has(record.decision.verdict)) findings.push('decision-verdict')
  if (record.decision.verdict !== 'PASS' && record.decision.promotable !== false) findings.push('nonpass-promotable')
  if (record.decision.verdict === 'PASS' && (!record.decision.promotable || !record.comparison.qualified)) findings.push('pass-not-qualified')
  if (!Array.isArray(record.decision.gaps)) findings.push('decision-gaps')
  if (typeof record.decision.rollback_trigger !== 'string' || record.decision.rollback_trigger.length < 3) findings.push('rollback-trigger')
  if (record.lifecycle.binary_match !== true || record.lifecycle.legitimate?.verdict !== 'PASS' || !Array.isArray(record.lifecycle.refusals) || record.lifecycle.refusals.length < 1 || record.lifecycle.cleanup?.complete !== true || record.lifecycle.residue !== 0) findings.push('lifecycle-incomplete')

  if (record.artifacts.length > MAX_ARTIFACTS) findings.push('artifact-count-limit')
  const artifactContents = new Map()
  let artifactTotalBytes = 0
  for (const artifact of record.artifacts.slice(0, MAX_ARTIFACTS)) {
    const file = resolveRepositoryFile(root, artifact?.path, findings, 'artifact')
    if (!file) continue
    if (!Number.isSafeInteger(artifact?.bytes) || artifact.bytes < 0 || file.stat.size !== artifact.bytes) findings.push('artifact-size')
    artifactTotalBytes += file.stat.size
    if (artifactTotalBytes > MAX_ARTIFACT_TOTAL_BYTES) findings.push('artifact-total-byte-limit')
    const bytes = readArtifact(file, findings)
    if (!bytes) continue
    artifactContents.set(artifact.path, bytes)
    if (!SHA256_RE.test(artifact.sha256 ?? '') || sha256(bytes) !== artifact.sha256.toLowerCase()) findings.push('artifact-hash')
    if (sensitiveRule(bytes.toString('utf8'))) findings.push('artifact-sensitive-content')
  }

  if (record.surface === 'wsl2-nbd' && record.slug === 'wsl2-nbd-product-readiness') {
    validateWsl2PairCustody(record, artifactContents, findings)
  }

  if (sensitiveRule(record)) findings.push('sensitive-content')
  return [...new Set(findings)]
}

function parseJson(contents, findings, rule, file) {
  try {
    return JSON.parse(contents)
  } catch {
    findings.push(`${rule}:${path.basename(file)}`)
    return null
  }
}

function parseJsonl(contents, findings) {
  const lines = contents.split(/\r?\n/).filter((line) => line.trim())
  if (lines.length > MAX_RECORDS) {
    findings.push('record-count-limit')
    return []
  }
  return lines.map((line, index) => {
    try { return JSON.parse(line) } catch { findings.push(`jsonl-parse:${index + 1}`); return null }
  }).filter(Boolean)
}

function benchmarkIds(markdown) {
  const ids = []
  const lines = markdown.split(/\r?\n/)
  let pending = null
  for (let index = 0; index < lines.length; index++) {
    const id = lines[index].match(/<!--\s*ramshared-benchmark-id:\s*([a-z0-9._-]+)\s*-->/i)
    if (id) pending = id[1]
    if (/^## 20\d{2}-\d{2}-\d{2}/.test(lines[index])) {
      if (!pending) ids.push({ id: null, line: index + 1 })
      else ids.push({ id: pending, line: index + 1 })
      pending = null
    }
  }
  return ids
}

export function validateRepository({ root = ROOT } = {}) {
  const findings = []
  const docs = path.join(root, 'docs')
  const bench = path.join(docs, 'benchmarks')
  const required = {
    markdown: path.join(docs, 'BENCHMARKS.md'),
    results: path.join(bench, 'results.jsonl'),
    legacy: path.join(bench, 'legacy-unqualified.json'),
    map: path.join(bench, 'benchmark-map.json'),
  }
  const source = {}
  for (const [key, file] of Object.entries(required)) source[key] = readRepositoryFile(file, findings, key)
  if (findings.length) return { ok: false, findings: [...new Set(findings)].sort() }

  const records = parseJsonl(source.results, findings)
  const legacyDoc = parseJson(source.legacy, findings, 'legacy-parse', required.legacy)
  const mapDoc = parseJson(source.map, findings, 'map-parse', required.map)
  const legacyEntries = Array.isArray(legacyDoc?.entries) ? legacyDoc.entries : []
  const mappings = Array.isArray(mapDoc?.entries) ? mapDoc.entries : []
  if (legacyDoc?.schema_version !== 'ramshared-legacy-benchmarks/v1') findings.push('legacy-schema')
  if (mapDoc?.schema_version !== 'ramshared-benchmark-map/v1') findings.push('map-schema')

  const runIds = new Set()
  for (const record of records) {
    if (runIds.has(record.run_id)) findings.push(`duplicate-run-id:${record.run_id}`)
    runIds.add(record.run_id)
    if (record.schema_version === 'ramshared-evidence/v1') {
      for (const finding of validateRecord(record, { root })) findings.push(`record:${record.run_id}:${finding}`)
    } else if (!legacyEntries.some((entry) => entry.run_id === record.run_id)) {
      findings.push(`legacy-record-unmapped:${record.run_id ?? 'missing'}`)
    }
  }
  const legacyIds = new Set()
  for (const entry of legacyEntries) {
    if (!entry.id || legacyIds.has(entry.id)) findings.push(`legacy-id:${entry.id ?? 'missing'}`)
    legacyIds.add(entry.id)
    if (entry.qualified !== false) findings.push(`legacy-qualified:${entry.id ?? entry.run_id ?? 'missing'}`)
    if (typeof entry.reason !== 'string' || entry.reason.length < 8) findings.push(`legacy-reason:${entry.id ?? 'missing'}`)
  }

  if (sensitiveRule(legacyDoc)) findings.push('legacy-sensitive-content')
  if (sensitiveRule(mapDoc)) findings.push('map-sensitive-content')
  const ids = benchmarkIds(source.markdown)
  const seenBenchmark = new Set()
  for (const item of ids) {
    if (!item.id) { findings.push(`benchmark-id-missing:${item.line}`); continue }
    if (seenBenchmark.has(item.id)) findings.push(`benchmark-id-duplicate:${item.id}`)
    seenBenchmark.add(item.id)
    const matches = mappings.filter((entry) => entry.benchmark_id === item.id)
    if (matches.length !== 1) findings.push(`benchmark-unmapped:${item.id}`)
  }
  for (const mapping of mappings) {
    if (!seenBenchmark.has(mapping.benchmark_id)) findings.push(`map-orphan:${mapping.benchmark_id ?? 'missing'}`)
    const hasRun = mapping.run_id && (runIds.has(mapping.run_id) || legacyEntries.some((entry) => entry.run_id === mapping.run_id))
    const hasLegacy = mapping.legacy_id && legacyIds.has(mapping.legacy_id)
    if (!hasRun && !hasLegacy) findings.push(`map-target:${mapping.benchmark_id ?? 'missing'}`)
  }
  return { ok: findings.length === 0, findings: [...new Set(findings)].sort(), counts: { sections: ids.length, records: records.length, legacy: legacyEntries.length } }
}

function main() {
  if (!process.argv.includes('--check') || process.argv.length > 3) {
    console.error('usage: check-benchmark-evidence.mjs --check')
    return 64
  }
  const result = validateRepository({ root: ROOT })
  if (!result.ok) {
    for (const finding of result.findings) console.error(`benchmark-evidence — ${finding}`)
    return 1
  }
  console.log(`✓ benchmark evidence OK (sections=${result.counts.sections} records=${result.counts.records} legacy=${result.counts.legacy})`)
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
