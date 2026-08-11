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
  let artifactTotalBytes = 0
  for (const artifact of record.artifacts.slice(0, MAX_ARTIFACTS)) {
    const file = resolveRepositoryFile(root, artifact?.path, findings, 'artifact')
    if (!file) continue
    if (!Number.isSafeInteger(artifact?.bytes) || artifact.bytes < 0 || file.stat.size !== artifact.bytes) findings.push('artifact-size')
    artifactTotalBytes += file.stat.size
    if (artifactTotalBytes > MAX_ARTIFACT_TOTAL_BYTES) findings.push('artifact-total-byte-limit')
    const bytes = readArtifact(file, findings)
    if (!bytes) continue
    if (!SHA256_RE.test(artifact.sha256 ?? '') || sha256(bytes) !== artifact.sha256.toLowerCase()) findings.push('artifact-hash')
    if (sensitiveRule(bytes.toString('utf8'))) findings.push('artifact-sensitive-content')
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
