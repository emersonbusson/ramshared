#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const SHA_RE = /^[0-9a-f]{64}$/i

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function resolvePublicPath(root, rel, findings, rule) {
  if (typeof rel !== 'string' || !rel || path.isAbsolute(rel) || /^[A-Za-z]:[\\/]/.test(rel) || rel.split(/[\\/]/).includes('..')) {
    findings.push(`${rule}-path`)
    return null
  }
  const full = path.resolve(root, rel)
  if (!full.startsWith(`${path.resolve(root)}${path.sep}`) || !existsSync(full)) {
    findings.push(`${rule}-missing`)
    return null
  }
  return full
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

  const specPath = resolvePublicPath(root, record.spec?.path, findings, 'spec')
  if (specPath) {
    const actual = sha256(readFileSync(specPath))
    if (!SHA_RE.test(record.spec?.sha256 ?? '') || actual !== record.spec.sha256.toLowerCase()) findings.push('spec-hash')
  }
  const implPath = resolvePublicPath(root, record.impl_path, findings, 'impl')
  const validationPath = resolvePublicPath(root, record.validation_path, findings, 'validation')
  void implPath
  void validationPath

  if (!Array.isArray(record.tests) || record.tests.length < 1) findings.push('named-tests')
  else {
    for (const item of record.tests) {
      const testPath = resolvePublicPath(root, item?.path, findings, 'test')
      if (typeof item?.name !== 'string' || item.name.length < 3) findings.push('test-name')
      if (item?.exit_code !== 0) findings.push(`test-exit:${item?.name ?? 'missing'}`)
      if (testPath && !readFileSync(testPath, 'utf8').includes(item.name)) findings.push(`test-name-not-found:${item.name}`)
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
    else for (const item of live.evidence_artifacts) resolvePublicPath(root, item, findings, 'live-artifact')
    if (record.binary_match?.required && (record.binary_match.passed !== true || !Array.isArray(record.binary_match.identities) || record.binary_match.identities.length < 1)) findings.push('done-binary-match')
  } else if ((envBound?.length ?? 0) < 1 && (open?.length ?? 0) < 1) {
    findings.push('partial-without-gap')
  }

  if (!Array.isArray(record.artifacts) || record.artifacts.length < 1) findings.push('artifacts')
  else {
    for (const item of record.artifacts) {
      const full = resolvePublicPath(root, item?.path, findings, 'artifact')
      if (!full) continue
      const stat = statSync(full)
      if (!stat.isFile() || stat.size !== item.bytes) findings.push('artifact-size')
      const actual = sha256(readFileSync(full))
      if (!SHA_RE.test(item.sha256 ?? '') || actual !== item.sha256.toLowerCase()) findings.push('artifact-hash')
    }
  }
  if (typeof record.rollback_trigger !== 'string' || record.rollback_trigger.length < 8) findings.push('rollback-trigger')
  return [...new Set(findings)].sort()
}

function* walk(dir) {
  let entries = []
  try { entries = readdirSync(dir, { withFileTypes: true }) } catch { return }
  for (const entry of entries) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) yield* walk(full)
    else if (entry.isFile()) yield full
  }
}

export function validateRepositoryClaims({ root = ROOT } = {}) {
  const findings = []
  const specsRoot = path.join(root, 'docs', 'specs')
  const files = [...walk(specsRoot)]
  const manifests = files.filter((file) => path.basename(file) === 'evidence-manifest.json')
  const claimMarkers = files.filter((file) => path.basename(file) === 'claim-status.json')
  const manifestDirs = new Set(manifests.map(path.dirname))
  for (const marker of claimMarkers) if (!manifestDirs.has(path.dirname(marker))) findings.push(`claim-without-manifest:${path.relative(root, marker)}`)
  for (const file of manifests) {
    let record
    try { record = JSON.parse(readFileSync(file, 'utf8')) } catch { findings.push(`manifest-parse:${path.relative(root, file)}`); continue }
    for (const finding of validateClaimManifest(record, root)) findings.push(`${path.relative(root, file)}:${finding}`)
  }
  return { ok: findings.length === 0, findings: findings.sort(), count: manifests.length }
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
