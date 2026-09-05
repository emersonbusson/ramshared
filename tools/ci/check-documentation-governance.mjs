#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import { evaluateClaimClosures, loadClaimClosures } from './documentation-claim-closure.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const MAX_FILE_BYTES = 512 * 1024
const MAX_FILES = 2000
const REQUIRED_PARITY = [
  'architecture and topology', 'capability state', 'prd and spec requirements',
  'operation', 'empirical validation', 'benchmark comparison',
  'reliability gaps', 'postmortem closure',
]
const REQUIRED_REFERENCE = [
  'build and test', 'linux or wsl2 lifecycle', 'windows lifecycle',
  'host-safety campaign', 'benchmark', 'reliability gaps',
  'architecture decisions', 'ssdv3 change', 'investigate and close an incident',
]

function finding(pathname, line, rule, reason) {
  return { path: pathname, line, rule, reason }
}

function sorted(findings) {
  return findings.sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line || a.rule.localeCompare(b.rule) || a.reason.localeCompare(b.reason))
}

function extractPath(value) {
  const markdown = value.match(/\[[^\]]*\]\(([^)]+)\)/)
  if (markdown) return markdown[1].split('#')[0]
  const code = value.match(/`([^`]+)`/)
  return code ? code[1] : value.trim()
}

function safeRelative(rel) {
  return typeof rel === 'string' && rel.length > 0 && !path.isAbsolute(rel) && !/^[A-Za-z]:[\\/]/.test(rel) && !rel.includes('://') && !rel.split(/[\\/]/).includes('..')
}

export function parseMarkdownTable(text, sourcePath = 'document.md') {
  const lines = text.split(/\r?\n/)
  const rows = []
  for (let index = 0; index < lines.length; index++) {
    if (!lines[index].trim().startsWith('|')) continue
    const cells = lines[index].split('|').slice(1, -1).map((cell) => cell.trim())
    if (cells.length < 2 || cells.every((cell) => /^-+$/.test(cell.replaceAll(' ', '')))) continue
    rows.push({ cells, line: index + 1, path: sourcePath })
  }
  if (rows.length > 0) rows.shift()
  return rows
}

function validateRouting(text, root, required, sourcePath, keyIndex) {
  const findings = []
  const rows = parseMarkdownTable(text, sourcePath)
  const seen = new Set()
  for (const row of rows) {
    const key = row.cells[keyIndex]?.toLowerCase()
    if (!key) continue
    if (seen.has(key)) findings.push(finding(sourcePath, row.line, 'duplicate-objective', 'duplicate-objective'))
    seen.add(key)
    const rel = extractPath(row.cells[1] ?? '')
    if (!safeRelative(rel)) findings.push(finding(sourcePath, row.line, 'unsafe-target', 'unsafe-target'))
    else if (!existsSync(path.join(root, rel))) findings.push(finding(sourcePath, row.line, 'missing-target', 'missing-target'))
  }
  for (const item of required) if (![...seen].some((key) => key.includes(item))) findings.push(finding(sourcePath, 1, 'missing-objective', item))
  return sorted(findings)
}

export function validateParityDocument(text, root = ROOT) {
  return validateRouting(text, root, REQUIRED_PARITY, 'docs/DOCUMENTATION-PARITY.md', 0)
}

export function validateReferenceIndex(text, root = ROOT) {
  return validateRouting(text, root, REQUIRED_REFERENCE, 'docs/reference/REFERENCE-INDEX.md', 0)
}

const ROUTER_PAIRS = [
  ['architecture', 'architecture and topology', 'architecture decisions'],
  ['linux-operation', 'operation', 'linux or wsl2 lifecycle'],
  ['campaign-evidence', 'campaign evidence custody', 'publish or inspect campaign evidence'],
  ['space-recovery', 'workstation-space recovery', 'recover workstation disk space'],
  ['document-lifecycle', 'document lifecycle coverage', 'document owners and freshness'],
  ['threat-model', 'governance threat model', 'governance threat boundaries'],
  ['timestamps', 'task and evidence timestamps', 'task and validation timestamps'],
  ['benchmarks', 'benchmark comparison', 'register or compare a benchmark'],
  ['reliability-gaps', 'reliability gaps', 'reliability gaps remain open'],
  ['ssdv3', 'prd and spec requirements', 'make an ssdv3 change'],
  ['postmortems', 'postmortem closure', 'investigate and close an incident'],
]

function extractPaths(value, { allowParent = false } = {}) {
  const paths = []
  for (const match of value.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) paths.push(match[1].split('#')[0])
  for (const match of value.matchAll(/`([^`]+)`/g)) paths.push(match[1])
  return [...new Set(allowParent ? paths : paths.filter(safeRelative))]
}

function routerRows(text, sourcePath) {
  return parseMarkdownTable(text, sourcePath).map((row) => ({
    key: row.cells[0]?.toLowerCase() ?? '',
    sources: extractPaths(row.cells[1] ?? ''),
    line: row.line,
  }))
}

function compatibleSources(left, right) {
  return left.some((a) => right.some((b) => a === b ||
    (a.endsWith('/') && b.startsWith(a)) || (b.endsWith('/') && a.startsWith(b))))
}

function landingTarget(value) {
  if (typeof value !== 'string' || value.length === 0 || path.posix.isAbsolute(value) ||
      /^[A-Za-z]:[\\/]/.test(value) || value.includes('://')) return null
  const normalized = path.posix.normalize(path.posix.join('docs/governance', value))
  return safeRelative(normalized) ? normalized : null
}

export function validateRouterConsistency(parityText, referenceText, governanceText) {
  const findings = []
  const parityRows = routerRows(parityText, 'docs/DOCUMENTATION-PARITY.md')
  const referenceRows = routerRows(referenceText, 'docs/reference/REFERENCE-INDEX.md')
  for (const [topic, parityNeedle, referenceNeedle] of ROUTER_PAIRS) {
    const parity = parityRows.find((row) => row.key.includes(parityNeedle))
    const reference = referenceRows.find((row) => row.key.includes(referenceNeedle))
    if (!parity || !reference) {
      findings.push(finding('docs/governance/README.md', 1, 'ROUTER_PAIR_MISSING', topic))
    } else if (!compatibleSources(parity.sources, reference.sources)) {
      findings.push(finding('docs/governance/README.md', 1, 'ROUTER_SOURCE_MISMATCH', topic))
    }
  }
  const landingLinks = []
  for (const rawTarget of extractPaths(governanceText, { allowParent: true })) {
    const target = landingTarget(rawTarget)
    if (target) landingLinks.push(target)
    else findings.push(finding('docs/governance/README.md', 1, 'ROUTER_LANDING_UNSAFE', 'unsafe-target'))
  }
  for (const required of [
    'docs/DOCUMENTATION-PARITY.md',
    'docs/reference/REFERENCE-INDEX.md',
    'docs/governance/claims.json',
  ]) {
    if (!landingLinks.includes(required)) findings.push(finding('docs/governance/README.md', 1, 'ROUTER_LANDING_LINK', required))
  }
  if (parseMarkdownTable(governanceText, 'docs/governance/README.md').length > 0) {
    findings.push(finding('docs/governance/README.md', 1, 'ROUTER_DUPLICATION', 'landing-page-must-not-copy-router-tables'))
  }
  return sorted(findings)
}

function pathExists(root, rel, findings, rule, line = 1) {
  if (!safeRelative(rel)) { findings.push(finding('docs/governance/claims.json', line, rule, 'unsafe-path')); return false }
  if (!existsSync(path.join(root, rel))) { findings.push(finding('docs/governance/claims.json', line, rule, 'missing-path')); return false }
  return true
}

export function validateClaims(registry, root = ROOT) {
  const findings = []
  if (registry?.schema_version !== 1 || !Array.isArray(registry.claims)) return [finding('docs/governance/claims.json', 1, 'claims-schema', 'invalid-schema')]
  const slugs = new Set()
  for (const claim of registry.claims) {
    const prefix = `claim-${claim?.slug ?? 'missing'}`
    if (!claim?.slug || slugs.has(claim.slug)) findings.push(finding('docs/governance/claims.json', 1, 'duplicate-slug', prefix))
    slugs.add(claim?.slug)
    if (!claim?.owner_role) findings.push(finding('docs/governance/claims.json', 1, 'owner', prefix))
    if (!['PRD', 'SPEC', 'UNQUALIFIED', 'PARTIAL', 'BLOCKED', 'DONE', 'N/A'].includes(claim?.state)) findings.push(finding('docs/governance/claims.json', 1, 'claim-state', prefix))
    if (claim?.canonical_spec) pathExists(root, claim.canonical_spec, findings, 'canonical-spec')
    for (const rel of claim?.implementation_paths ?? []) pathExists(root, rel, findings, 'implementation-path')
    for (const item of claim?.named_tests ?? []) {
      if (pathExists(root, item.path, findings, 'test-path')) {
        const text = readFileSync(path.join(root, item.path), 'utf8')
        if (!item.name || !text.includes(item.name)) findings.push(finding('docs/governance/claims.json', 1, 'test-name', prefix))
      }
    }
    if (claim?.cover?.evidence_path) pathExists(root, claim.cover.evidence_path, findings, 'cover-evidence')
    if (claim?.validation?.record_path) pathExists(root, claim.validation.record_path, findings, 'validation-path')
    for (const rel of claim?.validation?.evidence_paths ?? []) pathExists(root, rel, findings, 'missing-path')
    if (['PARTIAL', 'BLOCKED'].includes(claim?.state) && (!claim.environment_blocker || !claim.missing_gate || !claim.next_proof)) findings.push(finding('docs/governance/claims.json', 1, 'partial-blocker', prefix))
    if (claim?.state === 'DONE') {
      if (!claim.implementation_paths?.length || !claim.named_tests?.length || !claim.cover?.evidence_path || claim.validation?.verdict !== '✅' || !claim.validation?.evidence_paths?.length || claim.environment_blocker) findings.push(finding('docs/governance/claims.json', 1, 'done-evidence', prefix))
      if (claim.binary_match_required && !claim.validation.evidence_paths.some((rel) => existsSync(path.join(root, rel)) && readFileSync(path.join(root, rel), 'utf8').includes('binary_match'))) findings.push(finding('docs/governance/claims.json', 1, 'binary-match', prefix))
    }
    if (!claim?.rollback_trigger || /\b(?:TBD|TODO|none|if it goes wrong)\b/i.test(claim.rollback_trigger)) findings.push(finding('docs/governance/claims.json', 1, 'rollback-trigger', prefix))
  }
  return sorted(findings)
}

function validateAllowlist(allowlist) {
  const findings = []
  for (const item of allowlist?.entries ?? []) {
    if (!item.id || !item.rule || !item.pattern || !Array.isArray(item.scope) || !item.reason || !item.owner_role || !item.review_by || !item.expires) findings.push(finding('docs/governance/provenance-allowlist.json', 1, 'ALLOWLIST_SHAPE', 'invalid-entry'))
    if (item.scope?.some((scope) => scope === '**' || scope.endsWith('/'))) findings.push(finding('docs/governance/provenance-allowlist.json', 1, 'ALLOWLIST_SCOPE', 'broad-scope'))
    if (['SECRET', 'KERNEL_ADDRESS'].includes(item.rule)) findings.push(finding('docs/governance/provenance-allowlist.json', 1, 'ALLOWLIST_RULE', 'non-allowlistable'))
  }
  return findings
}

function validateBaseline(baseline) {
  const findings = []
  for (const item of baseline?.entries ?? []) {
    if (!item.path || !item.rule || !item.fingerprint || !/^[0-9a-f]{64}$/i.test(item.file_sha256 ?? '') || !item.reason || !item.owner_role || !item.review_by || !item.expires) findings.push(finding('docs/governance/provenance-baseline.json', 1, 'BASELINE_SHAPE', 'invalid-entry'))
  }
  return findings
}

const forbiddenProductPatterns = [
  new RegExp(['ad', 'voq'].join(''), 'i'),
  new RegExp(['menu', 'orders'].join('[- ]?'), 'i'),
]

export function scanProvenance(files, allowlist = { entries: [] }, baseline = { entries: [] }) {
  const findings = [...validateAllowlist(allowlist), ...validateBaseline(baseline)]
  const patterns = [
    ['SECRET', /(?:(?:password|api[-_ ]?key|token|credential)\s*(?:=|:)\s*["']?(?!(?:<REDACTED>|…|\.\.\.))[^\s"',}]+|--(?:password|api[-_]?key|token|credential)\s+(?!\$|<REDACTED>|…|\.\.\.)[^\s]+)/i],
    ['PRIVATE_KEY', /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/i],
    ['PRIVATE_PATH', /(?:\/home\/[A-Za-z0-9._-]+\/|C:\\Users\\[^\\\s"']+|\\\\wsl(?:\.localhost|\$)\\[^\\]+\\home\\[^\\]+)/i],
    ['EMAIL', /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i],
    ['KERNEL_ADDRESS', /\bffff[0-9a-f]{12,16}\b/i],
  ]
  for (const file of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    const lines = String(file.text).split(/\r?\n/)
    for (let index = 0; index < lines.length; index++) {
      for (const [rule, regex] of patterns) if (regex.test(lines[index])) findings.push(finding(file.path, index + 1, rule, rule.toLowerCase()))
      for (const regex of forbiddenProductPatterns) if (regex.test(lines[index])) findings.push(finding(file.path, index + 1, 'FOREIGN_PRODUCT', 'foreign-product'))
    }
  }
  return sorted(findings)
}

const FORBIDDEN_README_PATTERNS = [
  ['FORBIDDEN_AGENT_REF', /\bjules\b/i],
  ['FORBIDDEN_CENSUS_JARGON', /\b(?:162|383)\s+prs?\b/i],
  ['FORBIDDEN_INTERNAL_BRANCH', /\bfeat\/consolidate-jules\b/i],
]

export function validateReadmeHygiene(files) {
  const findings = []
  const targets = files.filter((f) => f.path === 'README.md' || f.path === 'README.pt-BR.md')
  for (const file of targets) {
    const lines = String(file.text).split(/\r?\n/)
    for (let index = 0; index < lines.length; index++) {
      for (const [rule, regex] of FORBIDDEN_README_PATTERNS) {
        if (regex.test(lines[index])) {
          findings.push(finding(file.path, index + 1, rule, rule.toLowerCase()))
        }
      }
    }
  }
  return sorted(findings)
}

function normalizedWords(text) {
  return text.replace(/```[\s\S]*?```/g, ' ').replace(/\[[^\]]*\]\([^)]+\)/g, ' ').replace(/^#+.*$/gm, ' ').toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim().split(/\s+/).filter(Boolean).slice(0, 10000)
}

export function findDuplicateNormativeBlocks(files) {
  const findings = []
  const prepared = files.map((file) => ({ ...file, words: normalizedWords(file.text) })).filter((file) => file.words.length >= 40)
  for (let left = 0; left < prepared.length; left++) for (let right = left + 1; right < prepared.length; right++) {
    const a = new Set(prepared[left].words), b = new Set(prepared[right].words)
    const intersection = [...a].filter((word) => b.has(word)).length
    const union = new Set([...a, ...b]).size
    if (union > 0 && intersection / union >= 0.92) findings.push(finding(prepared[left].path, 1, 'DUPLICATE_NORMATIVE', prepared[right].path))
  }
  return sorted(findings)
}

export function validateJourneyManifest(manifest, root = ROOT) {
  const findings = []
  const add = (rule) => findings.push(rule)
  if (manifest?.schema_version !== 1) add('schema')
  if (!Number.isInteger(manifest?.version) || manifest.version < 1) add('version')
  if (!Number.isInteger(manifest?.seed) || manifest.seed < 0) add('seed')
  if (!['fixed-utc', 'monotonic'].includes(manifest?.clock_policy)) add('clock')
  if (!Number.isInteger(manifest?.timeout_seconds) || manifest.timeout_seconds < 1 || manifest.timeout_seconds > 3600) add('timeout')
  if (!Array.isArray(manifest?.actions) || manifest.actions.some((item) => !['process-exit', 'file-exists', 'none'].includes(item.wait_strategy) || item.timeout_seconds < 1 || item.timeout_seconds > 3600)) add('wait-strategy')
  if (!manifest?.checkpoints?.before || !manifest?.checkpoints?.action || !manifest?.checkpoints?.after) add('checkpoints')
  if (!manifest?.legitimate_case || !Array.isArray(manifest?.refusal_cases) || manifest.refusal_cases.length < 2) add('cases')
  if (!manifest?.cleanup?.idempotent || !['none', 'remove-temporary-report'].includes(manifest?.cleanup?.mode)) add('cleanup')
  if (!Array.isArray(manifest?.consumer_paths) || manifest.consumer_paths.length < 1) add('consumer')
  for (const rel of [...(manifest?.runner_refs ?? []), ...(manifest?.consumer_paths ?? [])]) if (!safeRelative(rel) || !existsSync(path.join(root, rel))) add('consumer-path')
  for (const rel of manifest?.artifacts ?? []) if (!safeRelative(rel) || (!rel.startsWith('tmp/') && !existsSync(path.join(root, rel)))) add('artifact-path')
  return [...new Set(findings)].sort()
}

export function validatePostmortemEffectiveness(text, sourcePath) {
  if (!/\*\*Governance schema:\*\*\s*1/.test(text)) return []
  const closure = text.match(/\*\*Closure state:\*\*\s*(\w+)/i)?.[1]?.toLowerCase()
  if (closure !== 'effective') return []
  const findings = []
  if (!/\*\*Regression command:\*\*\s*\S+/.test(text)) findings.push('regression')
  if (!/\*\*Threshold:\*\*\s*.*\d/.test(text)) findings.push('threshold')
  const revalidation = text.match(/\*\*Revalidation:\*\*\s*(.*)/i)?.[1] ?? ''
  if (!/legitimate/i.test(revalidation)) findings.push('revalidation-legitimate')
  if (!/refusal/i.test(revalidation)) findings.push('critical-refusal')
  if (!/\*\*Observed result:\*\*\s*.+/.test(text)) findings.push('observed-result')
  if (!/\*\*Evidence:\*\*\s*.+/.test(text)) findings.push('evidence')
  return findings.map((reason) => `${sourcePath}:${reason}`)
}

function* walk(dir) {
  let entries = []
  try { entries = readdirSync(dir, { withFileTypes: true }) } catch { return }
  for (const entry of entries) {
    if (['.git', 'target', 'tmp', 'node_modules', 'artifacts'].includes(entry.name)) continue
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) yield* walk(full)
    else if (entry.isFile()) yield full
  }
}

function structuralFiles(root) {
  const extensions = new Set(['.md', '.json', '.jsonl', '.yml', '.yaml', '.sh', '.ps1', '.mjs'])
  const files = []
  for (const full of walk(root)) {
    const rel = path.relative(root, full).replaceAll('\\', '/')
    if (!extensions.has(path.extname(full).toLowerCase())) continue
    if (!(rel === 'README.md' || rel === 'README.pt-BR.md' || rel === 'ARCHITECTURE.md' || rel === 'CLAUDE.md' || rel === 'AGENTS.md' || rel === 'validation.md' || rel.startsWith('.claude/rules/') || rel.startsWith('docs/') || rel === 'scripts/docs-check.sh')) continue
    const stat = statSync(full)
    if (stat.size > MAX_FILE_BYTES) { files.push({ path: rel, text: '', oversize: true }); continue }
    files.push({ path: rel, text: readFileSync(full, 'utf8') })
    if (files.length > MAX_FILES) break
  }
  return files
}

function readJson(root, rel) {
  return JSON.parse(readFileSync(path.join(root, rel), 'utf8'))
}

export function run({ root = ROOT } = {}) {
  const findings = []
  const parity = readFileSync(path.join(root, 'docs/DOCUMENTATION-PARITY.md'), 'utf8')
  const reference = readFileSync(path.join(root, 'docs/reference/REFERENCE-INDEX.md'), 'utf8')
  const governance = readFileSync(path.join(root, 'docs/governance/README.md'), 'utf8')
  findings.push(...validateParityDocument(parity, root), ...validateReferenceIndex(reference, root))
  findings.push(...validateRouterConsistency(parity, reference, governance))
  const claims = readJson(root, 'docs/governance/claims.json')
  findings.push(...validateClaims(claims, root))
  const closureResult = evaluateClaimClosures(claims, loadClaimClosures(root), { root })
  for (const reason of closureResult.findings) findings.push(finding('docs/governance/claim-closures.json', 1, 'CLAIM_CLOSURE', reason))
  const files = structuralFiles(root)
  for (const file of files.filter((item) => item.oversize)) findings.push(finding(file.path, 1, 'FILE_LIMIT', 'file-size-limit'))
  findings.push(...scanProvenance(files, readJson(root, 'docs/governance/provenance-allowlist.json'), readJson(root, 'docs/governance/provenance-baseline.json')))
  findings.push(...validateReadmeHygiene(files))
  const journey = readJson(root, 'docs/governance/journeys/documentation-governance-smoke.json')
  for (const reason of validateJourneyManifest(journey, root)) findings.push(finding('docs/governance/journeys/documentation-governance-smoke.json', 1, 'JOURNEY', reason))
  for (const file of files.filter((item) => item.path.startsWith('docs/postmortems/') && item.path.endsWith('.md'))) for (const reason of validatePostmortemEffectiveness(file.text, file.path)) findings.push(finding(file.path, 1, 'POSTMORTEM', reason))
  return { ok: findings.length === 0, findings: sorted(findings), counts: { files: files.length, findings: findings.length } }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  if (!(argv.length === 1 && ['--all', '--check'].includes(argv[0]))) {
    console.error('usage: check-documentation-governance.mjs --all|--check')
    return 2
  }
  const result = run({ root: ROOT })
  console.log('SCOPE=structural')
  console.log(`FILES=${result.counts.files}`)
  console.log(`FINDINGS=${result.counts.findings}`)
  for (const item of result.findings) console.error(`${item.path}:${item.line} — ${item.rule} — ${item.reason}`)
  console.log(`GOVERNANCE_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
/* node:coverage enable */
