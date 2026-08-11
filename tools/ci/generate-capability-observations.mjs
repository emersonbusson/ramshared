#!/usr/bin/env node
/**
 * Passive capability observations for RamShared documentation surfaces.
 *
 * This generator never publishes capability state. `claims.json` remains the
 * only qualified claim registry; its state is copied here as reconciliation
 * context and every generated row remains OBSERVED.
 */
import { lstatSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const POLICY_PATH = 'docs/governance/capability-observation-policy.json'
const CLAIMS_PATH = 'docs/governance/claims.json'
const DOCUMENT_NAMES = Object.freeze({ prd: 'PRD.md', spec: 'SPEC.md', implementation: 'IMPL.md' })
const OBSERVATION_DOCUMENT_NAMES = Object.freeze([...Object.values(DOCUMENT_NAMES), 'README.md'])
const CLAIM_STATES = new Set(['PRD', 'SPEC', 'UNQUALIFIED', 'PARTIAL', 'BLOCKED', 'DONE', 'N/A'])

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable)
  if (!isObject(value)) return value
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]))
}

function finding(rule, detail = '') {
  return { rule, detail }
}

function isRegularFile(filePath) {
  try {
    const stat = lstatSync(filePath)
    return stat.isFile() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function isDirectory(directoryPath) {
  try {
    const stat = lstatSync(directoryPath)
    return stat.isDirectory() && !stat.isSymbolicLink()
  } catch {
    return false
  }
}

function inspectDirectoryEntry(entryPath) {
  try {
    const stat = lstatSync(entryPath)
    return {
      present: true,
      regularFile: stat.isFile() && !stat.isSymbolicLink(),
      unreadable: false,
    }
  } catch (error) {
    return {
      present: error?.code !== 'ENOENT',
      regularFile: false,
      unreadable: error?.code !== 'ENOENT',
    }
  }
}

function hasNamedObservationDocument(directoryPath) {
  return OBSERVATION_DOCUMENT_NAMES.some((filename) =>
    inspectDirectoryEntry(path.join(directoryPath, filename)).present)
}

function isUsableNamedDocument(entry, relative, findings) {
  if (entry.unreadable) {
    findings.push(finding('document-read', relative))
    return false
  }
  if (!entry.regularFile) {
    findings.push(finding('document-unsafe', relative))
    return false
  }
  return true
}

function normalisePath(value) {
  return value.replaceAll('\\', '/')
}

export function validateObservationPolicy(policy) {
  const findings = []
  if (!isObject(policy) || policy.schema_version !== 1) findings.push(finding('policy-schema'))
  if (!safeRelative(policy?.catalog_path) || !policy.catalog_path.endsWith('.json')) findings.push(finding('catalog-path'))
  if (!safeRelative(policy?.spec_root) || !policy.spec_root.startsWith('docs/specs/')) findings.push(finding('spec-root'))
  if (!Number.isInteger(policy?.max_specs) || policy.max_specs < 1 || policy.max_specs > 512) findings.push(finding('spec-limit'))
  if (!Number.isInteger(policy?.max_document_bytes) || policy.max_document_bytes < 256 || policy.max_document_bytes > 1024 * 1024) findings.push(finding('document-byte-limit'))
  if (!Array.isArray(policy?.allowed_surface_prefixes) || policy.allowed_surface_prefixes.length === 0 ||
      policy.allowed_surface_prefixes.some((prefix) => !safeRelative(prefix) || prefix.includes('/'))) {
    findings.push(finding('surface-prefixes'))
  }
  return findings
}

function readJson(root, relativePath, findings, rule) {
  const target = path.join(root, relativePath)
  if (!isRegularFile(target)) {
    findings.push(finding(rule, relativePath))
    return null
  }
  try {
    return JSON.parse(readFileSync(target, 'utf8'))
  } catch {
    findings.push(finding(rule, relativePath))
    return null
  }
}

function loadClaims(root, findings) {
  const registry = readJson(root, CLAIMS_PATH, findings, 'claims-registry')
  if (!registry || registry.schema_version !== 1 || !Array.isArray(registry.claims)) {
    if (registry) findings.push(finding('claims-schema'))
    return new Map()
  }
  const result = new Map()
  for (const claim of registry.claims) {
    if (!isObject(claim) || typeof claim.slug !== 'string' || !CLAIM_STATES.has(claim.state) || result.has(claim.slug)) {
      findings.push(finding('claims-schema'))
      continue
    }
    result.set(claim.slug, { state: claim.state, owner_role: typeof claim.owner_role === 'string' ? claim.owner_role : null })
  }
  return result
}

function collectSpecDirectories(root, policy, findings) {
  const base = path.join(root, policy.spec_root)
  if (!isDirectory(base)) {
    findings.push(finding('spec-root-missing', policy.spec_root))
    return []
  }
  const directories = []
  for (const name of readdirSync(base).sort()) {
    if (!/^[a-z0-9][a-z0-9-]*$/.test(name)) continue
    const candidate = path.join(base, name)
    if (isDirectory(candidate) && hasNamedObservationDocument(candidate)) {
      directories.push({ slug: name, absolute: candidate, relative: `${policy.spec_root}/${name}` })
    }
  }
  if (directories.length > policy.max_specs) findings.push(finding('spec-limit', String(directories.length)))
  return directories.slice(0, policy.max_specs)
}

function extractSurfaceReferences(text, root, policy) {
  const references = new Set()
  const token = /`([^`\r\n]+)`|\]\(([^)\s]+)\)/g
  for (const match of text.matchAll(token)) {
    const candidate = normalisePath((match[1] ?? match[2] ?? '').replace(/^\.\//, ''))
    if (!safeRelative(candidate) || !policy.allowed_surface_prefixes.some((prefix) => candidate === prefix || candidate.startsWith(`${prefix}/`))) continue
    if (!isRegularFile(path.join(root, candidate))) continue
    references.add(candidate)
  }
  return [...references].sort()
}

function isTestPath(value) {
  return /(^|\/)(?:tests?|testdata)\//.test(value) || /(?:\.test\.|_test\.|_test$)/.test(value)
}

function documentPathsForDirectory(root, directory, policy, findings) {
  const documents = { prd: null, spec: null, implementation: null }
  const allReferences = new Set()
  const readmeRelative = `${directory.relative}/README.md`
  const readmeEntry = inspectDirectoryEntry(path.join(root, readmeRelative))
  if (readmeEntry.present) isUsableNamedDocument(readmeEntry, readmeRelative, findings)
  for (const [kind, filename] of Object.entries(DOCUMENT_NAMES)) {
    const relative = `${directory.relative}/${filename}`
    const absolute = path.join(root, relative)
    const entry = inspectDirectoryEntry(absolute)
    if (!entry.present) continue
    if (!isUsableNamedDocument(entry, relative, findings)) continue
    let text
    try {
      text = readFileSync(absolute, 'utf8')
    } catch {
      findings.push(finding('document-read', relative))
      continue
    }
    if (Buffer.byteLength(text, 'utf8') > policy.max_document_bytes) {
      findings.push(finding('document-byte-limit', relative))
      continue
    }
    documents[kind] = relative
    for (const ref of extractSurfaceReferences(text, root, policy)) allReferences.add(ref)
  }
  const observed = [...allReferences].sort()
  return {
    documents,
    documented_surface: {
      implementation_paths: observed.filter((item) => !isTestPath(item)),
      test_paths: observed.filter(isTestPath),
    },
  }
}

export function buildCapabilityObservations(root = ROOT, suppliedPolicy = null) {
  const findings = []
  const policy = suppliedPolicy ?? readJson(root, POLICY_PATH, findings, 'policy-read')
  findings.push(...validateObservationPolicy(policy))
  if (findings.length > 0) return { findings, catalog: null }
  const claims = loadClaims(root, findings)
  const observations = collectSpecDirectories(root, policy, findings).map((directory) => {
    const surface = documentPathsForDirectory(root, directory, policy, findings)
    const claim = claims.get(directory.slug)
    return {
      slug: directory.slug,
      observation_state: 'OBSERVED',
      documents: surface.documents,
      documented_surface: surface.documented_surface,
      claim_reconciliation: {
        registry_path: CLAIMS_PATH,
        registry_state: claim?.state ?? null,
        claim_present: Boolean(claim),
        observation_is_not_a_claim: true,
      },
      promotion: {
        permitted: false,
        authority: CLAIMS_PATH,
        reason: 'Only the qualified claims registry may publish capability state.',
      },
    }
  })
  return {
    findings: findings.sort((left, right) => left.rule.localeCompare(right.rule) || left.detail.localeCompare(right.detail)),
    catalog: {
      schema_version: 'ramshared-capability-observations/v1',
      kind: 'passive-document-and-surface-observations',
      policy_path: POLICY_PATH,
      observation_state: 'OBSERVED',
      qualification_authority: CLAIMS_PATH,
      observations,
    },
  }
}

export function renderCapabilityObservations(catalog) {
  return `${JSON.stringify(stable(catalog), null, 2)}\n`
}

/* node:coverage disable */
function main() {
  const arguments_ = process.argv.slice(2)
  if (arguments_.length !== 1 || !['--check', '--write'].includes(arguments_[0])) {
    process.stderr.write('usage: node tools/ci/generate-capability-observations.mjs --check|--write\n')
    return 2
  }
  const result = buildCapabilityObservations(ROOT)
  if (result.findings.length > 0 || !result.catalog) {
    for (const item of result.findings) process.stderr.write(`capability-observations:${item.rule}${item.detail ? `:${item.detail}` : ''}\n`)
    return 1
  }
  const policy = JSON.parse(readFileSync(path.join(ROOT, POLICY_PATH), 'utf8'))
  const outputPath = path.join(ROOT, policy.catalog_path)
  const rendered = renderCapabilityObservations(result.catalog)
  if (arguments_[0] === '--write') {
    writeFileSync(outputPath, rendered, 'utf8')
    process.stdout.write(`✓ wrote ${policy.catalog_path} (${result.catalog.observations.length} observations)\n`)
    return 0
  }
  const current = isRegularFile(outputPath) ? readFileSync(outputPath, 'utf8') : ''
  if (current === rendered) {
    process.stdout.write(`✓ capability observations are in sync (${result.catalog.observations.length} observations)\n`)
    return 0
  }
  process.stderr.write(`capability observations are out of sync; run: node tools/ci/generate-capability-observations.mjs --write\n`)
  return 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
/* node:coverage enable */
