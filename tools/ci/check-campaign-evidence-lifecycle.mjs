#!/usr/bin/env node
import { createHash } from 'node:crypto'
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const POLICY_RELATIVE = 'docs/governance/campaign-evidence-lifecycle.json'
const CATALOG_RELATIVE = 'docs/governance/campaign-evidence-catalog.generated.json'
const MANIFEST_NAME = 'campaign-manifest.json'
const MANIFEST_SCHEMA = 'ramshared-campaign-evidence/v1'
const MAX_POLICY_BYTES = 256 * 1024
const MAX_MANIFEST_BYTES = 256 * 1024
const SHA256_RE = /^[0-9a-f]{64}$/
const COMMIT_RE = /^[0-9a-f]{40}$/
const RUN_ID_RE = /^[a-z0-9][a-z0-9-]{2,127}$/
const OWNER_RE = /^[a-z][a-z0-9-]{2,63}$/
const RFC3339_UTC_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?Z$/
const LIFECYCLES = new Set(['writing', 'complete', 'failed', 'blocked'])
const CLAIM_STATES = new Set(['PASS', 'PARTIAL', 'FAIL', 'BLOCKED'])
const SURFACES = new Set(['wsl2-freeze', 'windows-storage', 'kernel-lab', 'benchmark', 'other'])
const ENVIRONMENT_TIERS = new Set(['isolated', 'shared', 'physical'])

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`
  }
  return JSON.stringify(value)
}

function isPlainObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function push(findings, value) {
  if (!findings.includes(value)) findings.push(value)
}

function safeRelative(value) {
  if (typeof value !== 'string' || !value || value.length > 512) return null
  if (value.includes('\\') || path.isAbsolute(value) || /^[A-Za-z]:[\\/]/.test(value)) return null
  const normalized = path.posix.normalize(value)
  if (normalized !== value || normalized === '.' || normalized.startsWith('../') || value.split('/').includes('..')) return null
  return normalized
}

function resolveWithin(root, relative) {
  const safe = safeRelative(relative)
  if (!safe) return null
  const base = path.resolve(root)
  const full = path.resolve(base, safe)
  return full.startsWith(`${base}${path.sep}`) ? full : null
}

function relativeFrom(root, full) {
  return path.relative(root, full).split(path.sep).join('/')
}

function trackedPath(trackedPaths, relative) {
  return trackedPaths === null || trackedPaths.has(relative)
}

function trackedDescendant(trackedPaths, relative) {
  if (trackedPaths === null) return true
  const prefix = `${relative}/`
  return [...trackedPaths].some((item) => item === relative || item.startsWith(prefix))
}

function trackedEvidencePaths(root, policy) {
  try {
    const roots = policy.roots.map((item) => item.prefix.slice(0, -1))
    const output = execFileSync('git', ['ls-files', '-z', '--', ...roots], {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
      maxBuffer: 4 * 1024 * 1024,
    })
    const paths = new Set()
    for (const candidate of output.split('\0')) {
      const safe = safeRelative(candidate)
      if (!safe || !policy.roots.some((item) => safe.startsWith(item.prefix))) continue
      paths.add(safe)
    }
    return paths
  } catch {
    return null
  }
}

function boundedFile(full, maximum) {
  const stat = lstatSync(full)
  if (stat.isSymbolicLink()) throw new Error('symlink')
  if (!stat.isFile()) throw new Error('not-file')
  if (stat.size > maximum) throw new Error('size-limit')
  return { stat, bytes: readFileSync(full) }
}

function readJson(full, maximum, findings, label) {
  try {
    return JSON.parse(boundedFile(full, maximum).bytes.toString('utf8'))
  } catch {
    push(findings, `${label}-parse`)
    return null
  }
}

function sensitive(value) {
  const text = typeof value === 'string' ? value : canonicalJson(value)
  return [
    /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/i,
    /(?:api[-_ ]?key|token|password|credential|secret)\s*(?:=|:|\s)\s*["']?[^\s"',}]+/i,
    /(?:^|["'\s])\/home\/[A-Za-z0-9._-]+\//,
    /(?:^|["'\s])\/Users\/[A-Za-z0-9._-]+\//,
    /C:\\Users\\[^\\\s"']+/i,
    /\\\\wsl(?:\.localhost|\$)\\[^\\]+\\home\\[^\\]+/i,
    /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i,
    /\b(?:0x)?ffff[0-9a-f]{8,}\b/i,
  ].some((rule) => rule.test(text))
}

function validText(value, minimum = 3, maximum = 4096) {
  return typeof value === 'string' && value.length >= minimum && value.length <= maximum
}

function parseInstant(value, now, findings, label) {
  if (typeof value !== 'string' || !RFC3339_UTC_RE.test(value)) {
    push(findings, `${label}-timestamp`)
    return null
  }
  const instant = Date.parse(value)
  if (!Number.isFinite(instant)) {
    push(findings, `${label}-timestamp`)
    return null
  }
  if (instant > now) push(findings, `${label}-future`)
  return instant
}

function walkFiles(directory, findings, prefix = '', budget = null, trackedPaths = null, root = null) {
  const files = []
  let entries = []
  try { entries = readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)) } catch {
    push(findings, 'run-read')
    return files
  }
  for (const entry of entries) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name
    const full = path.join(directory, entry.name)
    const repositoryRelative = root ? relativeFrom(root, full) : null
    if (repositoryRelative && !trackedDescendant(trackedPaths, repositoryRelative)) continue
    let stat
    try { stat = lstatSync(full) } catch { push(findings, 'artifact-stat'); continue }
    if (stat.isSymbolicLink()) {
      push(findings, 'artifact-symlink')
      continue
    }
    if (stat.isDirectory()) {
      const descendants = walkFiles(full, findings, relative, budget, trackedPaths, root)
      if (descendants.length === 0) push(findings, 'artifact-orphan-directory')
      files.push(...descendants)
      continue
    }
    if (repositoryRelative && !trackedPath(trackedPaths, repositoryRelative)) continue
    if (!stat.isFile()) {
      push(findings, 'artifact-nonregular')
      continue
    }
    if (budget && budget.count >= budget.maximum) {
      push(findings, budget.finding)
      return files
    }
    if (budget) budget.count++
    files.push({ relative, full, stat })
  }
  return files
}

function rootForRun(runRelative, policy) {
  return policy.roots.find((item) => runRelative.startsWith(item.prefix) && runRelative !== item.prefix.slice(0, -1)) ?? null
}

function normalizePolicy(policy) {
  const findings = []
  // The numeric form is accepted by the exported API so focused consumers can
  // exercise the contract without having to reproduce repository metadata.
  // On disk, the named schema is the only accepted policy spelling.
  if (!isPlainObject(policy) || !['ramshared-campaign-evidence-policy/v1', 1].includes(policy.schema_version)) {
    return { findings: ['policy-schema'], policy: null }
  }
  const limits = policy.limits
  if (!isPlainObject(limits) || !Number.isInteger(limits.max_artifacts_per_run) || limits.max_artifacts_per_run < 1 || limits.max_artifacts_per_run > 4096 || !Number.isInteger(limits.max_artifact_bytes) || limits.max_artifact_bytes < 1 || limits.max_artifact_bytes > 64 * 1024 * 1024 || !Number.isInteger(limits.max_total_artifact_bytes) || limits.max_total_artifact_bytes < limits.max_artifact_bytes || limits.max_total_artifact_bytes > 256 * 1024 * 1024 || !Number.isInteger(limits.max_catalog_entries) || limits.max_catalog_entries < 1 || limits.max_catalog_entries > 65536) push(findings, 'policy-limits')
  const legacy = policy.legacy
  if (!isPlainObject(legacy) || !OWNER_RE.test(legacy.owner_role ?? '') || legacy.classification !== 'legacy-unqualified' || legacy.retention_class !== 'historical-immutable' || legacy.immutable !== true || !validText(legacy.reason, 8, 512)) push(findings, 'policy-legacy')
  if (!Array.isArray(policy.roots) || policy.roots.length < 1 || policy.roots.length > 64) push(findings, 'policy-roots')
  const roots = []
  const prefixes = new Set()
  for (const item of policy.roots ?? []) {
    const prefix = safeRelative(item?.prefix?.endsWith('/') ? item.prefix.slice(0, -1) : '')
    if (!prefix || !item.prefix.endsWith('/') || !prefix.startsWith('docs/') || !prefix.endsWith('/evidence') || prefixes.has(prefix) || !OWNER_RE.test(item?.owner_role ?? '') || !SURFACES.has(item?.surface)) {
      push(findings, 'policy-root')
      continue
    }
    prefixes.add(prefix)
    roots.push({ prefix: `${prefix}/`, owner_role: item.owner_role, surface: item.surface })
  }
  return {
    findings,
    policy: findings.length ? null : {
      schema_version: policy.schema_version === 1 ? 'ramshared-campaign-evidence-policy/v1' : policy.schema_version,
      limits,
      legacy,
      roots: roots.sort((a, b) => a.prefix.localeCompare(b.prefix)),
    },
  }
}

function manifestFiles(root, policy, findings = [], trackedPaths = null) {
  const found = []
  let visited = 0
  let exhausted = false
  const scan = (directory) => {
    if (exhausted) return
    let entries = []
    try { entries = readdirSync(directory, { withFileTypes: true }) } catch { return }
    for (const entry of entries) {
      const full = path.join(directory, entry.name)
      const relative = relativeFrom(root, full)
      if (!trackedDescendant(trackedPaths, relative)) continue
      if (visited >= policy.limits.max_catalog_entries) {
        push(findings, 'catalog-entry-limit')
        exhausted = true
        return
      }
      visited++
      // Discover the filename before trusting its type. A symlink called
      // campaign-manifest.json is itself a terminal custody failure.
      if (entry.name === MANIFEST_NAME && trackedPath(trackedPaths, relative)) {
        found.push(relative)
        continue
      }
      if (entry.isDirectory()) scan(full)
    }
  }
  for (const configured of policy.roots) {
    const evidenceRoot = resolveWithin(root, configured.prefix.slice(0, -1))
    if (evidenceRoot && existsSync(evidenceRoot)) scan(evidenceRoot)
  }
  return [...new Set(found)].sort()
}

function readCampaignManifest(root, manifestRelative, findings) {
  const full = resolveWithin(root, manifestRelative)
  if (!full || !existsSync(full)) {
    push(findings, 'manifest-missing')
    return null
  }
  return readJson(full, MAX_MANIFEST_BYTES, findings, 'manifest')
}

function validArtifactPath(value) {
  const safe = safeRelative(value)
  return safe && safe !== MANIFEST_NAME ? safe : null
}

export function validateCampaignManifest(manifest, { root = ROOT, runRelative, policy, now = new Date().toISOString(), trackedPaths = null } = {}) {
  const findings = []
  const normalizedPolicy = normalizePolicy(policy)
  for (const finding of normalizedPolicy.findings) push(findings, finding)
  if (!normalizedPolicy.policy) return findings.sort()
  const run = safeRelative(runRelative)
  if (!run || !rootForRun(run, normalizedPolicy.policy)) return ['run-path']
  const configured = rootForRun(run, normalizedPolicy.policy)
  const runPath = resolveWithin(root, run)
  if (!runPath || !existsSync(runPath)) push(findings, 'run-missing')
  else {
    try {
      const stat = lstatSync(runPath)
      if (stat.isSymbolicLink()) push(findings, 'run-symlink')
      else if (!stat.isDirectory()) push(findings, 'run-not-directory')
    } catch { push(findings, 'run-stat') }
  }
  if (!isPlainObject(manifest)) return ['manifest-type']
  if (sensitive(manifest)) push(findings, 'manifest-sensitive')
  if (manifest.schema_version !== MANIFEST_SCHEMA) push(findings, 'schema-version')
  if (!RUN_ID_RE.test(manifest.run_id ?? '')) push(findings, 'run-id')
  if (!OWNER_RE.test(manifest.owner_role ?? '') || manifest.owner_role !== configured.owner_role) push(findings, 'owner-role')
  if (!SURFACES.has(manifest.surface) || manifest.surface !== configured.surface) push(findings, 'surface')
  if (!LIFECYCLES.has(manifest.lifecycle)) push(findings, 'lifecycle')
  if (!CLAIM_STATES.has(manifest.claim_state)) push(findings, 'claim-state')

  const nowInstant = Date.parse(now)
  const effectiveNow = Number.isFinite(nowInstant) ? nowInstant : Date.now()
  const started = parseInstant(manifest.started_at, effectiveNow, findings, 'started')
  const finished = manifest.finished_at === undefined ? null : parseInstant(manifest.finished_at, effectiveNow, findings, 'finished')
  const published = manifest.published_at === undefined ? null : parseInstant(manifest.published_at, effectiveNow, findings, 'published')
  if (started !== null && finished !== null && finished < started) push(findings, 'finished-before-started')
  if (finished !== null && published !== null && published < finished) push(findings, 'published-before-finished')
  if (manifest.lifecycle === 'writing') {
    if (manifest.claim_state === 'PASS') push(findings, 'writing-claim-pass')
    if (finished !== null || published !== null) push(findings, 'writing-published')
  }
  if (manifest.lifecycle === 'complete') {
    if (!['PASS', 'PARTIAL'].includes(manifest.claim_state)) push(findings, 'complete-claim-state')
    if (finished === null) push(findings, 'complete-finished')
    if (published === null) push(findings, 'complete-published')
  }
  if (['failed', 'blocked'].includes(manifest.lifecycle)) {
    if (manifest.claim_state === 'PASS') push(findings, `${manifest.lifecycle}-claim-pass`)
    if (finished === null) push(findings, `${manifest.lifecycle}-finished`)
  }

  if (!isPlainObject(manifest.source) || !COMMIT_RE.test(manifest.source.commit ?? '') || typeof manifest.source.dirty !== 'boolean') push(findings, 'source')
  if (!isPlainObject(manifest.environment) || !ENVIRONMENT_TIERS.has(manifest.environment.tier) || manifest.environment.sanitization !== 'public') push(findings, 'environment')
  for (const field of ['before', 'action', 'after', 'rollback_trigger']) if (!validText(manifest[field], field === 'rollback_trigger' ? 8 : 3)) push(findings, `${field}-text`)
  if (!isPlainObject(manifest.legitimate) || !validText(manifest.legitimate.name) || manifest.legitimate.verdict !== 'PASS') push(findings, 'legitimate')
  if (!Array.isArray(manifest.refusals) || manifest.refusals.length < 1 || manifest.refusals.length > 64 || manifest.refusals.some((item) => !isPlainObject(item) || !validText(item.name) || item.verdict !== 'PASS')) push(findings, 'refusals')
  if (!isPlainObject(manifest.cleanup) || typeof manifest.cleanup.complete !== 'boolean' || !Number.isInteger(manifest.cleanup.residue) || manifest.cleanup.residue < 0) push(findings, 'cleanup')
  if (manifest.lifecycle === 'complete' && (manifest.cleanup?.complete !== true || manifest.cleanup?.residue !== 0)) push(findings, 'complete-cleanup')
  if (!isPlainObject(manifest.retention) || !validText(manifest.retention.class) || typeof manifest.retention.immutable !== 'boolean') push(findings, 'retention')

  const artifacts = manifest.artifacts
  if (!Array.isArray(artifacts) || artifacts.length > normalizedPolicy.policy.limits.max_artifacts_per_run || (manifest.lifecycle === 'complete' && artifacts.length < 1)) {
    push(findings, 'artifacts')
  }
  const declared = new Set()
  let totalBytes = 0
  for (const item of Array.isArray(artifacts) ? artifacts : []) {
    const artifactRelative = validArtifactPath(item?.path)
    if (!artifactRelative || declared.has(artifactRelative)) {
      push(findings, 'artifact-path')
      continue
    }
    declared.add(artifactRelative)
    if (!Number.isInteger(item?.bytes) || item.bytes < 0 || item.bytes > normalizedPolicy.policy.limits.max_artifact_bytes || !SHA256_RE.test(item?.sha256 ?? '') || item.sanitized !== true) {
      push(findings, 'artifact-metadata')
      continue
    }
    totalBytes += item.bytes
    const artifactPath = path.resolve(runPath ?? path.resolve(root), artifactRelative)
    if (!runPath || !artifactPath.startsWith(`${path.resolve(runPath)}${path.sep}`) || !existsSync(artifactPath)) {
      push(findings, 'artifact-missing')
      continue
    }
    if (!trackedPath(trackedPaths, relativeFrom(root, artifactPath))) {
      push(findings, 'artifact-untracked')
      continue
    }
    let before
    let bytes
    let after
    try {
      before = lstatSync(artifactPath)
      if (before.isSymbolicLink()) { push(findings, 'artifact-symlink'); continue }
      if (!before.isFile()) { push(findings, 'artifact-nonregular'); continue }
      if (before.size !== item.bytes) push(findings, 'artifact-bytes')
      if (before.size > normalizedPolicy.policy.limits.max_artifact_bytes) { push(findings, 'artifact-size-limit'); continue }
      bytes = readFileSync(artifactPath)
      after = lstatSync(artifactPath)
    } catch { push(findings, 'artifact-read'); continue }
    if (before.dev !== after.dev || before.ino !== after.ino || before.size !== after.size || before.mtimeMs !== after.mtimeMs) push(findings, 'artifact-race')
    if (sha256(bytes) !== item.sha256) push(findings, 'artifact-hash')
    if (sensitive(bytes.toString('utf8'))) push(findings, 'artifact-sensitive')
  }
  if (totalBytes > normalizedPolicy.policy.limits.max_total_artifact_bytes) push(findings, 'artifact-total-limit')

  if (runPath && existsSync(runPath)) {
    const discovered = walkFiles(runPath, findings, '', null, trackedPaths, root)
    const actual = new Set(discovered.map((item) => item.relative))
    const expected = new Set(declared)
    if (actual.has(MANIFEST_NAME)) expected.add(MANIFEST_NAME)
    for (const item of actual) if (!expected.has(item)) push(findings, 'artifact-orphan')
    for (const item of expected) if (!actual.has(item) && item !== MANIFEST_NAME) push(findings, 'artifact-missing')
  }
  return findings.sort()
}

function configuredRoots(root, policy) {
  return policy.roots.map((configured) => ({
    ...configured,
    full: resolveWithin(root, configured.prefix.slice(0, -1)),
  })).filter((configured) => configured.full && existsSync(configured.full))
}

export function buildEvidenceCatalog({ root = ROOT, policy, trackedPaths = null } = {}) {
  const normalized = normalizePolicy(policy)
  if (!normalized.policy) throw new Error(`invalid-policy:${normalized.findings.join(',')}`)
  const catalogFindings = []
  const manifestPaths = manifestFiles(root, normalized.policy, catalogFindings, trackedPaths)
  const manifests = new Map()
  for (const manifestPath of manifestPaths) {
    const findings = []
    const manifest = readCampaignManifest(root, manifestPath, findings)
    const runRelative = path.posix.dirname(manifestPath)
    const validation = manifest ? validateCampaignManifest(manifest, {
      root,
      runRelative,
      policy: normalized.policy,
      trackedPaths,
    }) : findings
    manifests.set(runRelative, { manifest_path: manifestPath, manifest, findings: validation })
  }
  const entries = []
  const budget = {
    count: 0,
    maximum: normalized.policy.limits.max_catalog_entries,
    finding: 'catalog-entry-limit',
  }
  for (const configured of configuredRoots(root, normalized.policy)) {
    const discoveryFindings = []
    const files = walkFiles(configured.full, discoveryFindings, '', budget, trackedPaths, root)
    // Empty historical directories carry no public artifact and are not a
    // promotion surface. Exact-set custody remains strict for a v1 run.
    for (const finding of discoveryFindings) {
      if (finding !== 'artifact-orphan-directory') push(catalogFindings, finding)
    }
    for (const item of files) {
      const relative = relativeFrom(root, item.full)
      const containing = [...manifests.keys()].find((run) => relative === run || relative.startsWith(`${run}/`))
      if (containing) continue
      if (item.stat.size > normalized.policy.limits.max_artifact_bytes) {
        catalogFindings.push(`legacy-artifact-size-limit:${relative}`)
        entries.push({
          kind: 'artifact-observation',
          path: relative,
          owner_role: configured.owner_role,
          surface: configured.surface,
          classification: normalized.policy.legacy.classification,
          retention_class: normalized.policy.legacy.retention_class,
          immutable: normalized.policy.legacy.immutable,
          promotion_eligible: false,
          reason: normalized.policy.legacy.reason,
          bytes: item.stat.size,
          sha256: null,
          bounded: false,
        })
        continue
      }
      entries.push({
        kind: 'artifact-observation',
        path: relative,
        owner_role: configured.owner_role,
        surface: configured.surface,
        classification: normalized.policy.legacy.classification,
        retention_class: normalized.policy.legacy.retention_class,
        immutable: normalized.policy.legacy.immutable,
        promotion_eligible: false,
        reason: normalized.policy.legacy.reason,
        bytes: item.stat.size,
        sha256: sha256(readFileSync(item.full)),
        bounded: true,
      })
    }
  }
  for (const [runRelative, item] of manifests) {
    const manifest = item.manifest
    entries.push({
      kind: 'campaign-run',
      path: runRelative,
      manifest_path: item.manifest_path,
      run_id: manifest?.run_id ?? null,
      owner_role: manifest?.owner_role ?? null,
      surface: manifest?.surface ?? null,
      lifecycle: manifest?.lifecycle ?? null,
      claim_state: manifest?.claim_state ?? null,
      classification: item.findings.length === 0 ? 'campaign-v1' : 'campaign-invalid',
      promotion_eligible: item.findings.length === 0 && manifest?.lifecycle === 'complete' && manifest?.claim_state === 'PASS',
      findings: item.findings,
    })
  }
  entries.sort((a, b) => a.path.localeCompare(b.path) || a.kind.localeCompare(b.kind))
  return {
    schema_version: 'ramshared-campaign-evidence-catalog/v1',
    policy_schema_version: normalized.policy.schema_version,
    findings: catalogFindings.sort(),
    entries,
  }
}

export function renderEvidenceCatalog(catalog) {
  return `${JSON.stringify(catalog, null, 2)}\n`
}

function nearestManifestRun(root, evidencePrefix, changed) {
  const safe = safeRelative(changed)
  if (!safe || !safe.startsWith(evidencePrefix)) return null
  const rootRelative = evidencePrefix.slice(0, -1)
  let cursor = path.posix.dirname(safe)
  while (cursor.startsWith(rootRelative) && cursor !== rootRelative) {
    const candidate = `${cursor}/${MANIFEST_NAME}`
    const full = resolveWithin(root, candidate)
    if (full && existsSync(full)) return cursor
    cursor = path.posix.dirname(cursor)
  }
  const tail = safe.slice(evidencePrefix.length).split('/')[0]
  return { missing: `${evidencePrefix}${tail}` }
}

export function validateProspectiveEvidence({ changedPaths = [], root = ROOT, policy, trackedPaths = null } = {}) {
  const normalized = normalizePolicy(policy)
  if (!normalized.policy) return normalized.findings.sort()
  const findings = []
  const checked = new Set()
  for (const changed of changedPaths) {
    const safe = safeRelative(changed)
    if (!safe) continue
    const configured = normalized.policy.roots.find((item) => safe.startsWith(item.prefix))
    if (!configured) continue
    const candidate = nearestManifestRun(root, configured.prefix, safe)
    if (!candidate) continue
    if (typeof candidate === 'object') {
      push(findings, `new-evidence-manifest-missing:${candidate.missing}`)
      continue
    }
    if (checked.has(candidate)) continue
    checked.add(candidate)
    const manifestPath = `${candidate}/${MANIFEST_NAME}`
    const parseFindings = []
    const manifest = readCampaignManifest(root, manifestPath, parseFindings)
    for (const finding of manifest ? validateCampaignManifest(manifest, {
      root,
      runRelative: candidate,
      policy: normalized.policy,
      trackedPaths,
    }) : parseFindings) push(findings, `new-evidence-manifest-invalid:${candidate}:${finding}`)
  }
  return findings.sort()
}

function loadPolicy(root, findings) {
  const policyPath = resolveWithin(root, POLICY_RELATIVE)
  if (!policyPath || !existsSync(policyPath)) {
    push(findings, 'policy-missing')
    return null
  }
  const policy = readJson(policyPath, MAX_POLICY_BYTES, findings, 'policy')
  const normalized = normalizePolicy(policy)
  for (const finding of normalized.findings) push(findings, finding)
  return normalized.policy
}

function gitAddedPaths(root, base) {
  try {
    const output = execFileSync('git', ['diff', '--name-only', '--diff-filter=A', base, '--'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
    return output.split(/\r?\n/).filter(Boolean)
  } catch {
    throw new Error('base-diff')
  }
}

export function validateRepository({ root = ROOT, base, trackedPaths = undefined } = {}) {
  const findings = []
  const policy = loadPolicy(root, findings)
  if (!policy) return { ok: false, findings: findings.sort(), count: 0 }
  const tracked = trackedPaths === undefined ? trackedEvidencePaths(root, policy) : trackedPaths
  if (tracked === null) push(findings, 'tracked-files')
  let catalog
  if (tracked !== null) {
    try { catalog = buildEvidenceCatalog({ root, policy, trackedPaths: tracked }) } catch { push(findings, 'catalog-build') }
  }
  const catalogPath = resolveWithin(root, CATALOG_RELATIVE)
  if (!catalogPath || !existsSync(catalogPath)) push(findings, 'catalog-missing')
  else if (catalog && readFileSync(catalogPath, 'utf8') !== renderEvidenceCatalog(catalog)) push(findings, 'catalog-stale')
  for (const finding of catalog?.findings ?? []) push(findings, finding)
  const manifestDiscoveryFindings = []
  for (const manifestPath of tracked === null ? [] : manifestFiles(root, policy, manifestDiscoveryFindings, tracked)) {
    const parseFindings = []
    const manifest = readCampaignManifest(root, manifestPath, parseFindings)
    const runRelative = path.posix.dirname(manifestPath)
    const validation = manifest ? validateCampaignManifest(manifest, {
      root,
      runRelative,
      policy,
      trackedPaths: tracked,
    }) : parseFindings
    for (const finding of validation) push(findings, `manifest:${runRelative}:${finding}`)
  }
  for (const finding of manifestDiscoveryFindings) push(findings, finding)
  if (base) {
    try {
      for (const finding of validateProspectiveEvidence({
        changedPaths: gitAddedPaths(root, base),
        root,
        policy,
        trackedPaths: tracked,
      })) push(findings, finding)
    } catch { push(findings, 'base-diff') }
  }
  return { ok: findings.length === 0, findings: findings.sort(), count: catalog?.entries.length ?? 0 }
}

function writeAtomically(destination, content) {
  const directory = path.dirname(destination)
  mkdirSync(directory, { recursive: true })
  const temporary = path.join(directory, `.${path.basename(destination)}.${process.pid}.tmp`)
  try {
    writeFileSync(temporary, content, { encoding: 'utf8', mode: 0o644 })
    renameSync(temporary, destination)
  } finally {
    if (existsSync(temporary)) rmSync(temporary, { force: true })
  }
}

function usage() {
  console.error('usage: check-campaign-evidence-lifecycle.mjs --check [--base <git-ref>] | --generate')
  return 64
}

export function runCli(args, root = ROOT) {
  if (args.length === 1 && args[0] === '--generate') {
    const findings = []
    const policy = loadPolicy(root, findings)
    if (!policy) {
      for (const finding of findings) console.error(`campaign-evidence — ${finding}`)
      return 1
    }
    const tracked = trackedEvidencePaths(root, policy)
    if (tracked === null) {
      console.error('campaign-evidence — tracked-files')
      return 1
    }
    writeAtomically(resolveWithin(root, CATALOG_RELATIVE), renderEvidenceCatalog(buildEvidenceCatalog({
      root,
      policy,
      trackedPaths: tracked,
    })))
    console.log('✓ campaign evidence catalog generated')
    return 0
  }
  if (args[0] !== '--check' || !(args.length === 1 || (args.length === 3 && args[1] === '--base' && args[2]))) return usage()
  const result = validateRepository({ root, base: args[2] })
  if (!result.ok) {
    for (const finding of result.findings) console.error(`campaign-evidence — ${finding}`)
    return 1
  }
  console.log(`✓ campaign evidence lifecycle OK (observations=${result.count})`)
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(runCli(process.argv.slice(2)))
}
