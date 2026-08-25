#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const MANIFEST_PATH = 'docs/localization/manifest.json'
const REQUIRED_LOCALIZED = new Set(['README.pt-BR.md', 'docs/pt-BR/README.md'])
const REQUIRED_MANIFEST_KEYS = new Set([
  'schema_version', 'canonical_language', 'current_policy',
  'protected_document_classes', 'entries',
])
const ENTRY_KEYS = new Set([
  'canonical_source', 'localized_path', 'source_sha256', 'translation_sha256',
  'state', 'state_reason', 'policy', 'reviewer', 'review_receipt', 'reviewed_at',
])
const RECEIPT_KEYS = new Set([
  'schema_version', 'canonical_source', 'localized_path', 'source_sha256',
  'translation_sha256', 'reviewer', 'reviewed_at', 'verdict',
])
const PROTECTED_CLASSES = new Set(['prd', 'spec', 'impl', 'adr', 'ci', 'evidence', 'benchmark'])
const POLICY = 'informational-non-normative'
const LOCALIZATION_STATES = new Set(['current', 'stale', 'partial'])
const MAX_FILE_BYTES = 512 * 1024
const MAX_FILES = 2000
const OBJECTIVES = [
  { id: 'quickstart', terms: ['quick start', 'quickstart', 'in\u00edcio r\u00e1pido', 'comece'], targets: ['README.md'] },
  { id: 'installation', terms: ['installation', 'install', 'instala\u00e7\u00e3o'], targets: ['docs/packaging/INSTALLABLES.md'] },
  { id: 'safe-operation', terms: ['safe operation', 'opera\u00e7\u00e3o segura'], targets: ['README.md'] },
  { id: 'troubleshooting', terms: ['troubleshooting', 'solu\u00e7\u00e3o de problemas', 'problemas'], targets: ['docs/FAQ.md'] },
  { id: 'architecture', terms: ['architecture', 'arquitet\u0075ra'], targets: ['ARCHITECTURE.md'] },
]

function finding(pathname, line, rule, reason) {
  return { path: pathname, line, rule, reason }
}

function sorted(findings) {
  return findings.sort((a, b) => (
    a.path.localeCompare(b.path) || a.line - b.line ||
    a.rule.localeCompare(b.rule) || a.reason.localeCompare(b.reason)
  ))
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 &&
    !path.posix.isAbsolute(value) && !/^[A-Za-z]:[\\/]/.test(value) &&
    !value.includes('://') && !value.split('/').includes('..') &&
    !/[\u0000-\u001f]/.test(value)
}

function safeLinkRelative(value) {
  return typeof value === 'string' && value.length > 0 &&
    !path.posix.isAbsolute(value) && !/^[A-Za-z]:[\\/]/.test(value) &&
    !value.includes('://') && !/[\u0000-\u001f]/.test(value)
}

function relativePath(root, value) {
  if (!safeRelative(value)) return null
  const resolved = path.resolve(root, value)
  const rootPath = path.resolve(root)
  if (resolved !== rootPath && !resolved.startsWith(`${rootPath}${path.sep}`)) return null
  return resolved
}

function fileText(root, relative) {
  const absolute = relativePath(root, relative)
  if (!absolute || !existsSync(absolute)) return null
  try {
    if (!statSync(absolute).isFile() || statSync(absolute).size > MAX_FILE_BYTES) return null
    return readFileSync(absolute, 'utf8')
  } catch {
    return null
  }
}

function hashFile(root, relative) {
  const absolute = relativePath(root, relative)
  if (!absolute || !existsSync(absolute)) return null
  try {
    const bytes = readFileSync(absolute)
    if (bytes.length > MAX_FILE_BYTES) return null
    return createHash('sha256').update(bytes).digest('hex')
  } catch {
    return null
  }
}

function manifestKeysAreExact(manifest) {
  return manifest && Object.keys(manifest).sort().join(',') === [...REQUIRED_MANIFEST_KEYS].sort().join(',')
}

function entryKeysAreExact(entry) {
  return entry && Object.keys(entry).sort().join(',') === [...ENTRY_KEYS].sort().join(',')
}

function receiptKeysAreExact(receipt) {
  return receipt && Object.keys(receipt).sort().join(',') === [...RECEIPT_KEYS].sort().join(',')
}

function reviewedAt(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) return false
  const instant = Date.parse(value)
  return Number.isFinite(instant) && new Date(instant).toISOString() === value.replace(/Z$/, '.000Z')
}

function protectedPath(relative) {
  const lower = relative.toLowerCase()
  if (lower.startsWith('docs/specs/') || lower.startsWith('docs/decisions/') ||
      lower.startsWith('docs/evidence/') || lower.startsWith('docs/benchmarks/') ||
      lower.startsWith('.github/')) return true
  if (/(^|\/)prd\.md$/.test(lower) || /(^|\/)spec(?:v\d+)?\.md$/.test(lower)) return true
  if (/(^|\/)impl\.md$/.test(lower) || /(^|\/)adr(?:-|\d|\.md)/.test(lower)) return true
  if (/(^|\/)\.github\/workflows\//.test(lower) || /(^|\/)ci(?:\/|[-_])/i.test(relative)) return true
  if (/(^|\/)evidence(?:\/|[-_])/i.test(relative) || /(^|\/)benchmark(?:s)?(?:\/|[-_])/i.test(relative)) return true
  return false
}

export function loadManifest(root = ROOT) {
  const absolute = path.join(root, MANIFEST_PATH)
  try {
    return JSON.parse(readFileSync(absolute, 'utf8'))
  } catch {
    return null
  }
}

export function validateManifest(manifest, root = ROOT) {
  const findings = []
  if (!manifest || !manifestKeysAreExact(manifest)) {
    return [finding(MANIFEST_PATH, 1, 'MANIFEST_SCHEMA', 'invalid-manifest-shape')]
  }
  if (manifest.schema_version !== 2) findings.push(finding(MANIFEST_PATH, 1, 'MANIFEST_SCHEMA', 'unsupported-version'))
  if (manifest.canonical_language !== 'en') findings.push(finding(MANIFEST_PATH, 1, 'MANIFEST_LANGUAGE', 'canonical-language-must-be-en'))
  if (manifest.current_policy !== POLICY) findings.push(finding(MANIFEST_PATH, 1, 'MANIFEST_POLICY', 'unsupported-current-policy'))
  if (!Array.isArray(manifest.protected_document_classes) ||
      manifest.protected_document_classes.length !== PROTECTED_CLASSES.size ||
      new Set(manifest.protected_document_classes).size !== PROTECTED_CLASSES.size ||
      manifest.protected_document_classes.some((item) => !PROTECTED_CLASSES.has(item))) {
    findings.push(finding(MANIFEST_PATH, 1, 'MANIFEST_PROTECTED_CLASSES', 'invalid-protected-class-list'))
  }
  if (!Array.isArray(manifest.entries) || manifest.entries.length !== REQUIRED_LOCALIZED.size) {
    findings.push(finding(MANIFEST_PATH, 1, 'MANIFEST_ENTRIES', 'required-entry-count'))
    return sorted(findings)
  }
  const localized = new Set()
  for (const [index, entry] of manifest.entries.entries()) {
    const line = index + 1
    if (!entryKeysAreExact(entry)) {
      findings.push(finding(MANIFEST_PATH, line, 'ENTRY_SCHEMA', 'invalid-entry-shape'))
      continue
    }
    if (entry.canonical_source !== 'README.md') findings.push(finding(MANIFEST_PATH, line, 'CANONICAL_SOURCE', 'canonical-source-must-be-readme'))
    if (!safeRelative(entry.canonical_source) || !safeRelative(entry.localized_path)) {
      findings.push(finding(MANIFEST_PATH, line, 'UNSAFE_PATH', 'manifest-path-must-be-relative'))
    }
    if (localized.has(entry.localized_path)) findings.push(finding(MANIFEST_PATH, line, 'DUPLICATE_LOCALIZED_PATH', 'localized-path-is-duplicated'))
    localized.add(entry.localized_path)
    if (!REQUIRED_LOCALIZED.has(entry.localized_path)) findings.push(finding(MANIFEST_PATH, line, 'UNEXPECTED_LOCALIZATION', 'path-is-not-required'))
    if (protectedPath(entry.localized_path)) findings.push(finding(MANIFEST_PATH, line, 'PROTECTED_PATH', 'normative-document-class-is-not-localized'))
    if (!/^[a-f0-9]{64}$/.test(entry.source_sha256)) findings.push(finding(MANIFEST_PATH, line, 'SOURCE_HASH', 'source-sha256-must-be-lowercase-hex'))
    if (!/^[a-f0-9]{64}$/.test(entry.translation_sha256)) findings.push(finding(MANIFEST_PATH, line, 'TRANSLATION_HASH', 'translation-sha256-must-be-lowercase-hex'))
    if (!LOCALIZATION_STATES.has(entry.state)) findings.push(finding(MANIFEST_PATH, line, 'STATE', 'unsupported-localization-state'))
    if (entry.state === 'current') {
      if (typeof entry.reviewer !== 'string' || !/^[a-z0-9][a-z0-9._-]{2,63}$/.test(entry.reviewer)) findings.push(finding(MANIFEST_PATH, line, 'REVIEWER', 'current-localization-requires-nonidentifying-reviewer-role'))
      if (!safeRelative(entry.review_receipt) || !entry.review_receipt.startsWith('docs/localization/reviews/')) findings.push(finding(MANIFEST_PATH, line, 'REVIEW_RECEIPT', 'current-localization-requires-scoped-receipt'))
      if (!reviewedAt(entry.reviewed_at)) findings.push(finding(MANIFEST_PATH, line, 'REVIEWED_AT', 'current-localization-requires-review-time'))
      if (entry.state_reason !== null) findings.push(finding(MANIFEST_PATH, line, 'STATE_REASON', 'current-localization-must-not-have-incomplete-reason'))
    } else {
      if (typeof entry.state_reason !== 'string' || !entry.state_reason.trim()) findings.push(finding(MANIFEST_PATH, line, 'STATE_REASON', 'incomplete-localization-requires-reason'))
      if (entry.reviewer !== null || entry.review_receipt !== null || entry.reviewed_at !== null) findings.push(finding(MANIFEST_PATH, line, 'REVIEW_STATE', 'incomplete-localization-cannot-claim-review'))
    }
    if (entry.policy !== POLICY) findings.push(finding(MANIFEST_PATH, line, 'POLICY', 'localized-policy-must-be-informational'))
    if (safeRelative(entry.canonical_source) && !existsSync(path.join(root, entry.canonical_source))) {
      findings.push(finding(MANIFEST_PATH, line, 'MISSING_CANONICAL', 'canonical-source-is-missing'))
    }
  }
  for (const required of REQUIRED_LOCALIZED) {
    if (!localized.has(required)) findings.push(finding(MANIFEST_PATH, 1, 'MISSING_ENTRY', 'required-localization-entry-is-missing'))
  }
  return sorted(findings)
}

function validateReviewReceipt(entry, root) {
  if (entry.state !== 'current' || !safeRelative(entry.review_receipt)) return []
  const findings = []
  const text = fileText(root, entry.review_receipt)
  let receipt
  try { receipt = text === null ? null : JSON.parse(text) } catch { receipt = null }
  if (!receipt || !receiptKeysAreExact(receipt)) {
    return [finding(entry.review_receipt, 1, 'REVIEW_RECEIPT', 'missing-or-invalid-review-receipt')]
  }
  const expected = {
    schema_version: 'ramshared-localization-review/v1',
    canonical_source: entry.canonical_source,
    localized_path: entry.localized_path,
    source_sha256: entry.source_sha256,
    translation_sha256: entry.translation_sha256,
    reviewer: entry.reviewer,
    reviewed_at: entry.reviewed_at,
    verdict: 'approved',
  }
  for (const [key, value] of Object.entries(expected)) {
    if (receipt[key] !== value) findings.push(finding(entry.review_receipt, 1, 'REVIEW_BINDING', key))
  }
  return findings
}

export function scanMarkdownLinks(text, sourcePath) {
  const links = []
  const pattern = /!?\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g
  let match
  while ((match = pattern.exec(text)) !== null) {
    const target = match[2].split('#')[0]
    const line = text.slice(0, match.index).split('\n').length
    links.push({ label: match[1], target, line })
  }
  return links
}

function linkTarget(root, source, target) {
  if (!target || target.startsWith('#') || /^(?:https?:|mailto:)/i.test(target)) return true
  if (!safeLinkRelative(target)) return false
  const sourceDir = path.posix.dirname(source)
  const relative = path.posix.normalize(path.posix.join(sourceDir, target))
  const absolute = relativePath(root, relative)
  return Boolean(absolute && existsSync(absolute))
}

function hasSwitch(links, target, source) {
  return links.some((link) => safeLinkRelative(link.target) &&
    path.posix.normalize(path.posix.join(path.posix.dirname(source), link.target)) === target)
}

function hasDisclaimer(text) {
  return /(?:informativ|informational)[^\n]{0,180}(?:não|not|non)[^\n]{0,100}(?:normativ|substitu|authorit|canonical|official|normativ)/i.test(text) ||
    /(?:não|not)\s+(?:é|e|is)?\s*(?:normativ|substitu|a\s+fonte|the\s+canonical)/i.test(text)
}

function authorityFinding(text, source) {
  const lines = text.split(/\r?\n/)
  const patterns = [
    /\b(?:this|este|esta)\s+(?:localized\s+)?(?:document|translation|page|documento|tradução|página)[^\n]{0,120}\b(?:is|é|constitui|serve\s+como)\b[^\n]{0,100}\b(?:normativ|canonical|official|authorit|fonte|normativ)/i,
    /\b(?:normative authority|canonical source|official source|source of truth|autoridade normativa|fonte normativa|fonte canônica|fonte oficial)\b/i,
  ]
  const findings = []
  for (const [index, line] of lines.entries()) {
    const previous = lines[index - 1] ?? ''
    const context = line
    const continuedDisclaimer = line.trimStart().startsWith('>') &&
      previous.trimStart().startsWith('>') && /(?:informativ|informational)/i.test(previous)
    if (continuedDisclaimer || /(?:não|not|non)\b[^\n]{0,180}(?:normativ|canonical|official|authorit|fonte)/i.test(context)) continue
    if (patterns.some((pattern) => pattern.test(line))) findings.push(finding(source, index + 1, 'AUTHORITY_CLAIM', 'localized-file-claims-technical-authority'))
  }
  return findings
}

function objectiveFindings(text, links, root, source) {
  const lower = text.toLocaleLowerCase('en-US')
  const findings = []
  for (const objective of OBJECTIVES) {
    const named = objective.terms.some((term) => lower.includes(term))
    const target = links.some((link) => objective.targets.some((candidate) => {
      if (!safeLinkRelative(link.target)) return false
      const relative = path.posix.normalize(path.posix.join(path.posix.dirname(source), link.target))
      return relative === candidate && Boolean(relativePath(root, relative) && existsSync(path.join(root, relative)))
    }))
    if (!named || !target) findings.push(finding(source, 1, 'PORTAL_OBJECTIVE', objective.id))
  }
  return findings
}

export function validateLocalizations(manifest, root = ROOT) {
  const findings = validateManifest(manifest, root)
  if (!manifest || !Array.isArray(manifest.entries)) return sorted(findings)
  const entries = manifest.entries.filter((entry) => entry && typeof entry.localized_path === 'string')
  const sourceSwitches = new Map()
  for (const entry of entries) {
    const localized = entry.localized_path
    const text = fileText(root, localized)
    if (text === null) {
      findings.push(finding(localized, 1, 'MISSING_LOCALIZATION', 'required-localized-file-is-missing-or-too-large'))
      continue
    }
    const links = scanMarkdownLinks(text, localized)
    for (const link of links) {
      if (!linkTarget(root, localized, link.target)) findings.push(finding(localized, link.line, 'BROKEN_LINK', 'localized-link-target-is-missing-or-unsafe'))
    }
    if (!hasDisclaimer(text)) findings.push(finding(localized, 1, 'DISCLAIMER', 'localized-file-must-declare-informational-policy'))
    findings.push(...authorityFinding(text, localized))
    if (localized === 'README.pt-BR.md') sourceSwitches.set('pt', hasSwitch(links, 'README.md', localized))
    if (localized === 'docs/pt-BR/README.md') {
      sourceSwitches.set('portal', hasSwitch(links, 'README.pt-BR.md', localized))
      findings.push(...objectiveFindings(text, links, root, localized))
    }
    const expectedHash = hashFile(root, entry.canonical_source)
    if (expectedHash === null || expectedHash !== entry.source_sha256) findings.push(finding(MANIFEST_PATH, 1, 'STALE_HASH', 'canonical-source-hash-does-not-match'))
    const translationHash = hashFile(root, entry.localized_path)
    if (translationHash === null || translationHash !== entry.translation_sha256) findings.push(finding(MANIFEST_PATH, 1, 'STALE_TRANSLATION_HASH', 'localized-content-hash-does-not-match'))
    findings.push(...validateReviewReceipt(entry, root))
  }
  const english = fileText(root, 'README.md')
  if (english === null) {
    findings.push(finding('README.md', 1, 'MISSING_CANONICAL', 'canonical-readme-is-missing-or-too-large'))
  } else {
    const links = scanMarkdownLinks(english, 'README.md')
    if (!hasSwitch(links, 'README.pt-BR.md', 'README.md')) findings.push(finding('README.md', 1, 'LANGUAGE_SWITCH', 'english-readme-must-link-to-portuguese-readme'))
  }
  if (sourceSwitches.get('pt') !== true) findings.push(finding('README.pt-BR.md', 1, 'LANGUAGE_SWITCH', 'portuguese-readme-must-link-to-english-readme'))
  if (sourceSwitches.get('portal') !== true) findings.push(finding('docs/pt-BR/README.md', 1, 'LANGUAGE_SWITCH', 'portal-must-link-to-portuguese-readme'))
  return sorted(findings)
}

export function run({ root = ROOT } = {}) {
  const manifest = loadManifest(root)
  const findings = validateLocalizations(manifest, root)
  const files = manifest?.entries?.length ?? 0
  const incomplete = (manifest?.entries ?? []).some((entry) => entry?.state !== 'current')
  const status = findings.length > 0 ? 'NO-GO' : incomplete ? 'PARTIAL' : 'PASS'
  return { ok: findings.length === 0, status, findings, counts: { files, findings: findings.length } }
}

export function main(argv = process.argv.slice(2)) {
  if (!(argv.length === 1 && argv[0] === '--all')) {
    console.error('usage: check-documentation-localization.mjs --all')
    return 2
  }
  const result = run({ root: ROOT })
  console.log('SCOPE=localization')
  console.log(`FILES=${result.counts.files}`)
  for (const item of result.findings) console.error(`${item.path}:${item.line} — ${item.rule} — ${item.reason}`)
  console.log(`FINDINGS=${result.counts.findings}`)
  console.log(`LOCALIZATION_STATUS=${result.status}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
