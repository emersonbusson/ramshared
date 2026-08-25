import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'

export const CLAIM_CLOSURE_SCHEMA = 'ramshared-claim-closures/v1'
export const CLAIM_CLOSURE_PATH = 'docs/governance/claim-closures.json'

const SHA256_RE = /^[0-9a-f]{64}$/
const REVISION_RE = /^[0-9a-f]{40}$/
const CLOSURE_KEYS = [
  'slug', 'state', 'base_revision', 'source_revision', 'files',
  'evidence_manifests', 'claim_sha256', 'closure_sha256',
]
const FILE_KEYS = ['path', 'sha256']

export function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`
  }
  if (value === null || typeof value === 'string' || typeof value === 'boolean' ||
      (typeof value === 'number' && Number.isFinite(value))) return JSON.stringify(value)
  throw new TypeError('canonical JSON accepts only finite JSON values')
}

export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

export function computeClaimDigest(claim) {
  return sha256(canonicalJson(claim))
}

function exactObject(value, keys) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key))
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.posix.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.includes('://') &&
    !value.split(/[\\/]/).includes('..') && !/[\u0000-\u001f]/.test(value)
}

function git(root, args, encoding = 'buffer') {
  return execFileSync('git', args, {
    cwd: root,
    encoding,
    maxBuffer: 8 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'ignore'],
  })
}

function revisionExists(root, revision) {
  if (!REVISION_RE.test(revision ?? '')) return false
  try {
    git(root, ['cat-file', '-e', `${revision}^{commit}`])
    return true
  } catch {
    return false
  }
}

function isAncestor(root, base, source) {
  try {
    git(root, ['merge-base', '--is-ancestor', base, source])
    return true
  } catch {
    return false
  }
}

function revisionBlob(root, revision, relative) {
  if (!REVISION_RE.test(revision ?? '') || !safeRelative(relative)) return null
  try {
    return git(root, ['show', `${revision}:${relative}`])
  } catch {
    return null
  }
}

function uniqueSorted(values) {
  return [...new Set(values.filter(Boolean))].sort((a, b) => a.localeCompare(b))
}

function array(value) {
  return Array.isArray(value) ? value : []
}

function manifestReferencedPaths(manifest) {
  return uniqueSorted([
    manifest?.spec?.path,
    manifest?.impl_path,
    manifest?.validation_path,
    ...array(manifest?.tests).map((item) => item?.path),
    ...array(manifest?.cover).map((item) => item?.path),
    ...array(manifest?.artifacts).map((item) => item?.path),
    ...array(manifest?.live?.evidence_artifacts),
  ])
}

export function requiredClaimPaths(claim, manifests = []) {
  return uniqueSorted([
    claim?.canonical_spec,
    ...array(claim?.implementation_paths),
    ...array(claim?.named_tests).map((item) => item?.path),
    claim?.cover?.evidence_path,
    claim?.validation?.record_path,
    ...array(claim?.validation?.evidence_paths),
    ...manifests.flatMap(manifestReferencedPaths),
  ])
}

export function computeClosureDigest(closure) {
  const payload = {
    slug: closure.slug,
    state: closure.state,
    base_revision: closure.base_revision,
    source_revision: closure.source_revision,
    files: closure.files,
    evidence_manifests: closure.evidence_manifests,
    claim_sha256: closure.claim_sha256,
  }
  return sha256(canonicalJson(payload))
}

export function loadClaimClosures(root) {
  const absolute = path.join(root, CLAIM_CLOSURE_PATH)
  if (!existsSync(absolute)) return null
  try {
    return JSON.parse(readFileSync(absolute, 'utf8'))
  } catch {
    return null
  }
}

function validateManifestForClaim(manifest, claim, manifestPath, sourceRevision, root, findings) {
  const prefix = `claim:${claim.slug}`
  if (!manifest || manifest.schema_version !== 'ramshared-spec-evidence/v1') {
    findings.push(`${prefix}:manifest-schema:${manifestPath}`)
    return
  }
  if (manifest.slug !== claim.slug) findings.push(`${prefix}:foreign-manifest:${manifestPath}`)
  if (claim.state === 'DONE' && manifest.status !== 'DONE') findings.push(`${prefix}:partial-manifest:${manifestPath}`)
  if (manifest.spec?.path !== claim.canonical_spec) findings.push(`${prefix}:manifest-spec:${manifestPath}`)
  const specBytes = revisionBlob(root, sourceRevision, manifest.spec?.path)
  if (!specBytes || !SHA256_RE.test(manifest.spec?.sha256 ?? '') || sha256(specBytes) !== manifest.spec.sha256) {
    findings.push(`${prefix}:manifest-spec-digest:${manifestPath}`)
  }
  if (!Array.isArray(manifest.tests)) findings.push(`${prefix}:manifest-tests-shape:${manifestPath}`)
  if (!Array.isArray(manifest.artifacts)) findings.push(`${prefix}:manifest-artifacts-shape:${manifestPath}`)
  for (const named of array(claim.named_tests)) {
    const matches = array(manifest.tests).filter((item) => item?.path === named.path && item?.name === named.name && item?.exit_code === 0)
    if (matches.length !== 1) findings.push(`${prefix}:manifest-test:${manifestPath}`)
  }
  for (const artifact of array(manifest.artifacts)) {
    const bytes = revisionBlob(root, sourceRevision, artifact?.path)
    if (!bytes || !Number.isSafeInteger(artifact?.bytes) || bytes.length !== artifact.bytes ||
        !SHA256_RE.test(artifact?.sha256 ?? '') || sha256(bytes) !== artifact.sha256) {
      findings.push(`${prefix}:manifest-artifact:${manifestPath}`)
    }
  }
}

function validateClosureShape(closure, claim, findings) {
  const initialFindingCount = findings.length
  const prefix = `claim:${claim?.slug ?? 'missing'}`
  if (!exactObject(closure, CLOSURE_KEYS)) {
    findings.push(`${prefix}:closure-shape`)
    return false
  }
  if (closure.slug !== claim.slug || closure.state !== claim.state) findings.push(`${prefix}:closure-claim-binding`)
  if (!['PARTIAL', 'BLOCKED', 'DONE'].includes(closure.state)) findings.push(`${prefix}:closure-state`)
  if (!REVISION_RE.test(closure.base_revision ?? '') || !REVISION_RE.test(closure.source_revision ?? '')) findings.push(`${prefix}:closure-revision-shape`)
  if (!Array.isArray(closure.files) || closure.files.length === 0 || closure.files.some((item) =>
    !exactObject(item, FILE_KEYS) || !safeRelative(item.path) || !SHA256_RE.test(item.sha256 ?? ''))) {
    findings.push(`${prefix}:closure-files-shape`)
  }
  if (!Array.isArray(closure.evidence_manifests) || closure.evidence_manifests.some((item) => !safeRelative(item))) {
    findings.push(`${prefix}:closure-manifests-shape`)
  }
  if (!SHA256_RE.test(closure.claim_sha256 ?? '')) findings.push(`${prefix}:claim-digest-shape`)
  if (!SHA256_RE.test(closure.closure_sha256 ?? '')) findings.push(`${prefix}:closure-digest-shape`)
  return findings.length === initialFindingCount
}

export function evaluateClaimClosure(claim, closure, { root }) {
  if (!closure) {
    return {
      qualified: false,
      status: ['PARTIAL', 'BLOCKED'].includes(claim?.state) ? claim.state : 'UNQUALIFIED',
      revision: null,
      findings: claim?.state === 'DONE' ? [`claim:${claim.slug}:closure-missing`] : [],
    }
  }

  const findings = []
  if (!validateClosureShape(closure, claim, findings)) {
    return { qualified: false, status: 'STALE', revision: null, findings }
  }
  if (!revisionExists(root, closure.base_revision)) findings.push(`claim:${claim.slug}:base-revision-missing`)
  if (!revisionExists(root, closure.source_revision)) findings.push(`claim:${claim.slug}:source-revision-missing`)
  if (closure.base_revision === closure.source_revision) findings.push(`claim:${claim.slug}:base-equals-source`)
  if (closure.source_revision !== claim?.validation?.source_commit) findings.push(`claim:${claim.slug}:source-revision-mismatch`)
  if (computeClaimDigest(claim) !== closure.claim_sha256) findings.push(`claim:${claim.slug}:claim-digest-mismatch`)
  if (revisionExists(root, closure.base_revision) && revisionExists(root, closure.source_revision) &&
      !isAncestor(root, closure.base_revision, closure.source_revision)) findings.push(`claim:${claim.slug}:base-not-ancestor`)

  const declaredManifests = uniqueSorted(array(claim?.validation?.evidence_paths))
  if (declaredManifests.length === 0) findings.push(`claim:${claim.slug}:manifest-set-empty`)
  if (canonicalJson(uniqueSorted(closure.evidence_manifests ?? [])) !== canonicalJson(declaredManifests)) {
    findings.push(`claim:${claim.slug}:manifest-set-mismatch`)
  }
  const manifests = []
  for (const manifestPath of declaredManifests) {
    const bytes = revisionBlob(root, closure.source_revision, manifestPath)
    if (!bytes) {
      findings.push(`claim:${claim.slug}:manifest-missing:${manifestPath}`)
      continue
    }
    try {
      const manifest = JSON.parse(bytes.toString('utf8'))
      manifests.push(manifest)
      validateManifestForClaim(manifest, claim, manifestPath, closure.source_revision, root, findings)
    } catch {
      findings.push(`claim:${claim.slug}:manifest-json:${manifestPath}`)
    }
  }

  const required = requiredClaimPaths(claim, manifests)
  const declaredFiles = [...(closure.files ?? [])].sort((a, b) => a.path.localeCompare(b.path))
  if (canonicalJson(closure.files) !== canonicalJson(declaredFiles)) findings.push(`claim:${claim.slug}:file-order`)
  if (canonicalJson(closure.evidence_manifests) !== canonicalJson(uniqueSorted(closure.evidence_manifests))) findings.push(`claim:${claim.slug}:manifest-order`)
  if (canonicalJson(declaredFiles.map((item) => item.path)) !== canonicalJson(required)) findings.push(`claim:${claim.slug}:file-set-mismatch`)
  const seen = new Set()
  for (const item of declaredFiles) {
    if (seen.has(item.path)) findings.push(`claim:${claim.slug}:duplicate-file`)
    seen.add(item.path)
    const bytes = revisionBlob(root, closure.source_revision, item.path)
    if (!bytes) findings.push(`claim:${claim.slug}:revision-evidence-missing:${item.path}`)
    else if (sha256(bytes) !== item.sha256) findings.push(`claim:${claim.slug}:file-digest-mismatch:${item.path}`)
  }
  if (computeClosureDigest(closure) !== closure.closure_sha256) findings.push(`claim:${claim.slug}:closure-digest-mismatch`)

  const qualified = findings.length === 0
  return {
    qualified,
    status: qualified ? claim.state : 'STALE',
    revision: qualified ? closure.source_revision : null,
    findings: [...new Set(findings)].sort(),
  }
}

export function evaluateClaimClosures(claimsRegistry, closuresRegistry, { root }) {
  const findings = []
  const evaluations = new Map()
  if (!closuresRegistry || !exactObject(closuresRegistry, ['schema_version', 'closures']) ||
      closuresRegistry.schema_version !== CLAIM_CLOSURE_SCHEMA || !Array.isArray(closuresRegistry.closures)) {
    findings.push('claim-closures:invalid-registry')
  }
  const closures = new Map()
  const registryClosures = Array.isArray(closuresRegistry?.closures) ? closuresRegistry.closures : []
  for (const closure of registryClosures) {
    if (closures.has(closure?.slug)) findings.push(`claim-closures:duplicate:${closure?.slug ?? 'missing'}`)
    closures.set(closure?.slug, closure)
  }
  const claims = Array.isArray(claimsRegistry?.claims) ? claimsRegistry.claims : []
  const known = new Set(claims.map((claim) => claim?.slug))
  for (const slug of closures.keys()) if (!known.has(slug)) findings.push(`claim-closures:orphan:${slug ?? 'missing'}`)
  for (const claim of claims) {
    const evaluation = evaluateClaimClosure(claim, closures.get(claim.slug), { root })
    evaluations.set(claim.slug, evaluation)
    findings.push(...evaluation.findings)
  }
  return { evaluations, findings: [...new Set(findings)].sort() }
}
