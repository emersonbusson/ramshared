#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const SHA_RE = /^[0-9a-f]{64}$/i
const MAX_MANIFEST_BYTES = 1024 * 1024
const MAX_ARTIFACT_BYTES = 8 * 1024 * 1024
const MAX_ARTIFACT_TOTAL_BYTES = 64 * 1024 * 1024
const MAX_ARTIFACTS = 128
const MAX_TESTS = 256

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
  if (value && typeof value === 'object') return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`
  return JSON.stringify(value)
}

function hasSensitiveContent(value) {
  const text = canonical(value)
  return [
    /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/i,
    /(?:api[-_ ]?key|token|password|credential)\s*(?:=|:|\s)\s*["']?[^\s"',}]+/i,
    /(?:^|["'\s])\/home\/[A-Za-z0-9._-]+\//,
    /C:\\Users\\[^\\\s"']+/i,
    /\\\\wsl(?:\.localhost|\$)\\[^\\]+\\home\\[^\\]+/i,
    /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i,
    /\b(?:ffff|0000)[0-9a-f]{12,}\b/i,
  ].some((rule) => rule.test(text))
}

function resolvePublicFile(root, rel, findings, rule) {
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

function readVerifiedFile(file, findings, rule, maxBytes = MAX_ARTIFACT_BYTES) {
  if (file.stat.size > maxBytes) {
    findings.push(`${rule}-byte-limit`)
    return null
  }
  try { return readFileSync(file.full) } catch { findings.push(`${rule}-read`); return null }
}

function validateArtifactList(artifacts, root, findings) {
  if (!Array.isArray(artifacts) || artifacts.length < 1) {
    findings.push('artifacts')
    return
  }
  if (artifacts.length > MAX_ARTIFACTS) {
    findings.push('artifact-count-limit')
    return
  }
  let totalBytes = 0
  for (const item of artifacts) {
    const file = resolvePublicFile(root, item?.path, findings, 'artifact')
    if (!file) continue
    if (!Number.isSafeInteger(item?.bytes) || item.bytes < 0 || file.stat.size !== item.bytes) findings.push('artifact-size')
    totalBytes += file.stat.size
    if (file.stat.size > MAX_ARTIFACT_BYTES) findings.push('artifact-byte-limit')
    if (totalBytes > MAX_ARTIFACT_TOTAL_BYTES) findings.push('artifact-total-byte-limit')
    const bytes = readVerifiedFile(file, findings, 'artifact')
    if (!bytes) continue
    if (!SHA_RE.test(item?.sha256 ?? '') || sha256(bytes) !== item.sha256.toLowerCase()) findings.push('artifact-hash')
    if (hasSensitiveContent(bytes.toString('utf8'))) findings.push('artifact-sensitive-content')
  }
}

function validEnvironmentBlocker(item) {
  return item && typeof item.blocker === 'string' && item.blocker.length >= 3 && typeof item.next_proof === 'string' && item.next_proof.length >= 3
}

export function validateClaimManifest(record, root = ROOT) {
  const findings = []
  if (!record || typeof record !== 'object' || Array.isArray(record)) return ['manifest-type']
  if (record.schema_version !== 'ramshared-spec-evidence/v1') findings.push('schema-version')
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(record.slug ?? '')) findings.push('slug')
  if (!['PARTIAL', 'DONE'].includes(record.status)) findings.push('status')

  const specFile = resolvePublicFile(root, record.spec?.path, findings, 'spec')
  if (specFile) {
    const bytes = readVerifiedFile(specFile, findings, 'spec')
    if (bytes && (!SHA_RE.test(record.spec?.sha256 ?? '') || sha256(bytes) !== record.spec.sha256.toLowerCase())) findings.push('spec-hash')
  }
  resolvePublicFile(root, record.impl_path, findings, 'impl')
  resolvePublicFile(root, record.validation_path, findings, 'validation')

  if (!Array.isArray(record.tests) || record.tests.length < 1) findings.push('named-tests')
  else if (record.tests.length > MAX_TESTS) findings.push('test-count-limit')
  else {
    for (const item of record.tests) {
      const testFile = resolvePublicFile(root, item?.path, findings, 'test')
      if (typeof item?.name !== 'string' || item.name.length < 3) findings.push('test-name')
      if (item?.exit_code !== 0) findings.push(`test-exit:${item?.name ?? 'missing'}`)
      const bytes = testFile && readVerifiedFile(testFile, findings, 'test')
      if (bytes && !bytes.toString('utf8').includes(item.name)) findings.push(`test-name-not-found:${item.name}`)
    }
  }

  if (!Array.isArray(record.cover) || record.cover.length < 1) findings.push('cover-matrix')
  else {
    for (const item of record.cover) {
      const measured = Number.isFinite(item?.line_percent) && item.line_percent >= 80
      const justified = typeof item?.classification === 'string' && item.classification.startsWith('N/A') && typeof item.justification === 'string' && item.justification.length >= 8
      if (!measured && !justified) findings.push(`cover-row:${item?.path ?? 'missing'}`)
    }
  }

  const envBound = record.gaps?.env_bound
  const open = record.gaps?.open
  if (!Array.isArray(envBound) || !Array.isArray(open)) findings.push('gaps')
  else if (!envBound.every(validEnvironmentBlocker)) findings.push('env-bound-shape')

  if (record.status === 'DONE') {
    if ((envBound?.length ?? 0) > 0) findings.push('done-env-bound')
    if ((open?.length ?? 0) > 0) findings.push('done-open-gap')
    const live = record.live
    if (!live?.required || !live.before || !live.action || !live.after) findings.push('done-live-path')
    if (live?.legitimate?.verdict !== 'PASS') findings.push('done-legitimate')
    if (!Array.isArray(live?.refusals) || live.refusals.length < 1 || live.refusals.some((item) => item.verdict !== 'PASS')) findings.push('done-refusals')
    if (live?.cleanup?.complete !== true || live?.cleanup?.residue !== 0) findings.push('done-cleanup')
    if (!Array.isArray(live?.evidence_artifacts) || live.evidence_artifacts.length < 1) findings.push('done-live-artifacts')
    else if (live.evidence_artifacts.length > MAX_ARTIFACTS) findings.push('live-artifact-count-limit')
    else for (const item of live.evidence_artifacts) resolvePublicFile(root, item, findings, 'live-artifact')
    if (record.binary_match?.required && (record.binary_match.passed !== true || !Array.isArray(record.binary_match.identities) || record.binary_match.identities.length < 1)) findings.push('done-binary-match')
  } else if ((envBound?.length ?? 0) < 1 && (open?.length ?? 0) < 1) {
    findings.push('partial-without-gap')
  }

  validateArtifactList(record.artifacts, root, findings)
  if (hasSensitiveContent(record)) findings.push('sensitive-content')
  if (typeof record.rollback_trigger !== 'string' || record.rollback_trigger.length < 8) findings.push('rollback-trigger')
  return [...new Set(findings)].sort()
}

function* walk(dir, root, findings) {
  let entries = []
  try { entries = readdirSync(dir, { withFileTypes: true }) } catch { return }
  for (const entry of entries) {
    const full = path.join(dir, entry.name)
    if (entry.isSymbolicLink()) {
      if (entry.name === 'evidence-manifest.json' || entry.name === 'claim-status.json') findings.push(`evidence-symlink:${path.relative(root, full)}`)
    } else if (entry.isDirectory()) yield* walk(full, root, findings)
    else if (entry.isFile()) yield full
  }
}

function parseManifest(file, root, findings) {
  let stat
  try { stat = lstatSync(file) } catch { findings.push(`manifest-missing:${path.relative(root, file)}`); return null }
  if (stat.isSymbolicLink()) { findings.push(`manifest-symlink:${path.relative(root, file)}`); return null }
  if (!stat.isFile()) { findings.push(`manifest-not-file:${path.relative(root, file)}`); return null }
  if (stat.size > MAX_MANIFEST_BYTES) { findings.push(`manifest-byte-limit:${path.relative(root, file)}`); return null }
  try { return JSON.parse(readFileSync(file, 'utf8')) } catch { findings.push(`manifest-parse:${path.relative(root, file)}`); return null }
}

export function validateRepositoryClaims({ root = ROOT } = {}) {
  const findings = []
  const specsRoot = path.join(root, 'docs', 'specs')
  const files = [...walk(specsRoot, root, findings)]
  const manifests = files.filter((file) => path.basename(file) === 'evidence-manifest.json')
  const claimMarkers = files.filter((file) => path.basename(file) === 'claim-status.json')
  const manifestDirs = new Set(manifests.map(path.dirname))
  for (const marker of claimMarkers) if (!manifestDirs.has(path.dirname(marker))) findings.push(`claim-without-manifest:${path.relative(root, marker)}`)
  for (const file of manifests) {
    const record = parseManifest(file, root, findings)
    if (!record) continue
    for (const finding of validateClaimManifest(record, root)) findings.push(`${path.relative(root, file)}:${finding}`)
  }
  return { ok: findings.length === 0, findings: [...new Set(findings)].sort(), count: manifests.length }
}

function main() {
  if (!process.argv.includes('--check') || process.argv.length > 3) {
    console.error('usage: check-spec-evidence.mjs --check')
    return 64
  }
  const result = validateRepositoryClaims({ root: ROOT })
  if (!result.ok) {
    for (const finding of result.findings) console.error(`spec-evidence — ${finding}`)
    return 1
  }
  console.log(`✓ SPEC evidence manifests OK (count=${result.count})`)
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
