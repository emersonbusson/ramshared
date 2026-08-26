#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  existsSync,
  lstatSync,
  readFileSync,
  realpathSync,
  statSync,
} from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { TextDecoder } from 'node:util'
import { fileURLToPath } from 'node:url'

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
export const MAX_FILE_BYTES = 512 * 1024
export const MAX_PATHS = 2_000
export const MAX_FINDINGS = 10_000
export const MAX_PROTECTED_INVENTORY_BYTES = 2 * 1024 * 1024
export const RATCHET_BASELINE_PATH = 'tools/ci/comment-language-baseline.json'
export const RATCHET_SCHEMA_PATH = 'tools/ci/comment-language-baseline.schema.json'
export const RATCHET_SCHEMA = 'ramshared-comment-language-ratchet/v1'
export const RATCHET_APPROVER_ROLE = 'repository-maintainer'
export const RATCHET_REVIEW_CHANNEL = 'pull-request-review'
export const MAX_RATCHET_BATCH_FILES = 10
export const MAX_RATCHET_BATCH_LINES = 100

const RATCHET_SCHEMA_REFERENCE = './comment-language-baseline.schema.json'
const SNAPSHOT_KEYS = [
  'mutable_files',
  'mutable_lines',
  'protected_files',
  'protected_lines',
  'protected_paths',
  'protected_inventory_sha256',
]
const APPROVAL_KEYS = ['approver_role', 'channel']
const RATCHET_RECORD_KEYS = ['$schema', 'schema', 'revision', 'approval', 'initial', 'current']

const UTF8 = new TextDecoder('utf-8', { fatal: true })
const SOURCE_EXTENSIONS = new Set(['.rs', '.c', '.h', '.ps1', '.sh', '.mjs'])
const PROSE_EXTENSIONS = new Set(['.md', '.txt', '.yaml', '.yml', '.toml', '.json', '.svg'])
const SCANNABLE_EXTENSIONS = new Set([...SOURCE_EXTENSIONS, ...PROSE_EXTENSIONS])
const LOCALIZED_ALLOWLIST = new Set(['README.pt-BR.md', 'docs/pt-BR/README.md', 'docs/marketing/cascade-diagram-pt.svg'])
const MACHINE_POLICY_DATA = new Set(['tools/ci/check-comment-language.mjs'])

const HIGH_CONFIDENCE_MARKERS = new Set([
  'não', 'nao', 'então', 'entao', 'português', 'portugues', 'comentário',
  'comentario', 'configuração', 'configuracao', 'função', 'funcao', 'usuário',
  'usuario', 'diretório', 'diretorio', 'atualização', 'atualizacao',
  'informação', 'informacao', 'você', 'voce', 'também', 'tambem', 'ação',
  'acao', 'operação', 'operacao', 'versão', 'versao', 'arquitetura',
])

const LOW_CONFIDENCE_MARKERS = new Set([
  'que', 'para', 'com', 'uma', 'mais', 'sobre', 'como', 'esta', 'tudo',
  'certo', 'funcionando', 'foi', 'pelo', 'pela', 'nas', 'nos', 'dos', 'das',
  'seu', 'sua', 'seus', 'suas', 'sem', 'isso', 'sao', 'este', 'estes',
  'estas', 'aquele', 'aquela', 'aqueles', 'aquelas', 'isto', 'aquilo',
  'porque', 'porquê', 'por', 'onde', 'quando', 'quem', 'qual', 'quais',
  'quanto', 'quantos', 'quanta', 'quantas', 'mas', 'porem', 'porém',
  'todavia', 'contudo', 'entretanto', 'enquanto', 'depois', 'antes', 'desde',
  'ate', 'até', 'contra', 'entre', 'perante', 'atras', 'atrás', 'sob', 'após',
  'durante', 'exceto', 'salvo', 'conforme', 'segundo', 'mediante', 'visto',
  'devido', 'graças', 'sendo', 'tendo',
])

const DIRECTIVE_RE = /^(?:!|syntax=|requires\b|go:|line\b|export\b|nolint\b|eslint\b|@ts-|prettier-ignore\b|biome-ignore\b|shellcheck\b|region\b|endregion\b)/i

export class LanguageError extends Error {}

function compareFindings(left, right) {
  return left.path.localeCompare(right.path) || left.line - right.line ||
    left.rule.localeCompare(right.rule) || left.scope.localeCompare(right.scope)
}

function hasControlCharacter(value) {
  return /[\u0000-\u001f\u007f]/.test(value)
}

export function safeRelativePath(value) {
  return typeof value === 'string' && value.length > 0 &&
    !path.posix.isAbsolute(value) && !/^[A-Za-z]:[\\/]/.test(value) && !value.startsWith('-') &&
    !value.includes('\\') && !hasControlCharacter(value) &&
    !value.split('/').includes('..') && !value.split('/').includes('.') &&
    path.posix.normalize(value) === value
}

export function validatePathList(paths) {
  if (!Array.isArray(paths) || paths.length > MAX_PATHS) {
    throw new LanguageError('path-count-limit')
  }
  const normalized = []
  const seen = new Set()
  for (const pathname of paths) {
    if (!safeRelativePath(pathname)) throw new LanguageError('unsafe-path')
    if (seen.has(pathname)) throw new LanguageError('duplicate-path')
    seen.add(pathname)
    normalized.push(pathname)
  }
  return normalized.sort((left, right) => left.localeCompare(right))
}

export function classifyPath(relative) {
  if (!safeRelativePath(relative)) throw new LanguageError('unsafe-path')
  if (MACHINE_POLICY_DATA.has(relative)) return 'machine-policy-data'
  if (LOCALIZED_ALLOWLIST.has(relative)) return 'localized'
  const segments = relative.split('/')
  if (relative === 'CHANGELOG.md' || relative === 'validation.md') return 'protected'
  if (relative.startsWith('docs/') && segments.includes('evidence')) return 'protected'
  if (relative.startsWith('docs/postmortems/') || relative.startsWith('docs/reliability/')) return 'protected'
  if (relative.startsWith('docs/specs/') && path.posix.basename(relative) === 'IMPL.md') return 'protected'
  return 'mutable'
}

function scannablePath(relative) {
  return SCANNABLE_EXTENSIONS.has(path.posix.extname(relative).toLowerCase())
}

function isMachineOnly(text) {
  const trimmed = text.trim()
  return trimmed.length === 0 || /^(?:https?:|file:|git:)/i.test(trimmed) ||
    /^(?:[a-f0-9]{32,}|sha256:[a-f0-9]{32,})$/i.test(trimmed)
}

function addUnit(units, line, text, scope, ext) {
  const cleaned = text.replace(/^\s*\*+\s?/, '').trim()
  if (!cleaned || isMachineOnly(cleaned) || (scope === 'comment' && DIRECTIVE_RE.test(cleaned))) return
  units.push({ line, text: cleaned, scope, ext })
}

function nextOutsideQuoteMarker(line, start, markers) {
  let quote = null
  let escaped = false
  for (let index = start; index < line.length; index++) {
    const character = line[index]
    if (quote) {
      if (escaped) {
        escaped = false
      } else if (character === '\\' || character === '`') {
        escaped = true
      } else if (character === quote) {
        quote = null
      }
      continue
    }
    if (character === '"' || character === "'" || character === '`') {
      quote = character
      continue
    }
    for (const marker of markers) {
      if (line.startsWith(marker, index)) return { index, marker }
    }
  }
  return null
}

function addQuotedUnits(segment, line, units, ext) {
  let quote = null
  let start = -1
  let escaped = false
  for (let index = 0; index < segment.length; index++) {
    const character = segment[index]
    if (!quote) {
      if (character === '"' || character === "'" || character === '`') {
        quote = character
        start = index + 1
      }
      continue
    }
    if (escaped) {
      escaped = false
      continue
    }
    if (character === '\\' || character === '`') {
      escaped = true
      continue
    }
    if (character === quote) {
      addUnit(units, line, segment.slice(start, index), 'source-text', ext)
      quote = null
      start = -1
    }
  }
}

function extractCStyleUnits(lines, ext) {
  const units = []
  let inBlockComment = false
  for (const [offset, line] of lines.entries()) {
    const lineNumber = offset + 1
    let cursor = 0
    while (cursor < line.length) {
      if (inBlockComment) {
        const close = line.indexOf('*/', cursor)
        const end = close === -1 ? line.length : close
        addUnit(units, lineNumber, line.slice(cursor, end), 'comment', ext)
        if (close === -1) {
          cursor = line.length
        } else {
          inBlockComment = false
          cursor = close + 2
        }
        continue
      }
      const marker = nextOutsideQuoteMarker(line, cursor, ['//', '/*'])
      const end = marker ? marker.index : line.length
      addQuotedUnits(line.slice(cursor, end), lineNumber, units, ext)
      if (!marker) break
      if (marker.marker === '//') {
        addUnit(units, lineNumber, line.slice(marker.index + 2), 'comment', ext)
        break
      }
      inBlockComment = true
      cursor = marker.index + 2
    }
  }
  return units
}

function extractHashStyleUnits(lines, ext) {
  const units = []
  const supportsBlock = ext === '.ps1'
  let inBlockComment = false
  for (const [offset, line] of lines.entries()) {
    const lineNumber = offset + 1
    let cursor = 0
    while (cursor < line.length) {
      if (inBlockComment) {
        const close = line.indexOf('#>', cursor)
        const end = close === -1 ? line.length : close
        addUnit(units, lineNumber, line.slice(cursor, end), 'comment', ext)
        if (close === -1) {
          cursor = line.length
        } else {
          inBlockComment = false
          cursor = close + 2
        }
        continue
      }
      const markers = supportsBlock ? ['<#', '#'] : ['#']
      const marker = nextOutsideQuoteMarker(line, cursor, markers)
      const end = marker ? marker.index : line.length
      addQuotedUnits(line.slice(cursor, end), lineNumber, units, ext)
      if (!marker) break
      if (marker.marker === '<#') {
        inBlockComment = true
        cursor = marker.index + 2
        continue
      }
      addUnit(units, lineNumber, line.slice(marker.index + 1), 'comment', ext)
      break
    }
  }
  return units
}

export function isCommentLine(line, ext) {
  const trimmed = line.trim()
  if (!trimmed) return false
  if (ext === '.ps1') return trimmed.startsWith('#') || trimmed.startsWith('<#') || trimmed.startsWith('.#')
  if (ext === '.sh') return trimmed.startsWith('#')
  return ['.rs', '.c', '.h', '.mjs'].includes(ext) &&
    (trimmed.startsWith('//') || trimmed.startsWith('/*') || trimmed.startsWith('*'))
}

export function cleanCommentText(line, ext) {
  const trimmed = line.trim()
  if (ext === '.ps1') return trimmed.replace(/^(?:<#|\.#|#)|#>$/g, '').trim()
  if (ext === '.sh') return trimmed.replace(/^#/, '').trim()
  return trimmed.replace(/^(?:\/\/|\/\*|\*)|\*\/$/g, '').trim()
}

function extractUnits(relative, text) {
  const ext = path.posix.extname(relative).toLowerCase()
  const lines = text.split(/\r?\n/)
  if (PROSE_EXTENSIONS.has(ext)) {
    const units = []
    for (const [offset, line] of lines.entries()) addUnit(units, offset + 1, line, 'document', ext)
    return units
  }
  if (ext === '.ps1' || ext === '.sh') return extractHashStyleUnits(lines, ext)
  return extractCStyleUnits(lines, ext)
}

export function getPtMarkers(text) {
  const normalized = text.normalize('NFC').toLocaleLowerCase('pt-BR')
  const tokens = normalized.match(/[\p{L}\p{N}_-]+/gu) ?? []
  const high = new Set()
  const low = new Set()
  for (const token of tokens) {
    if (HIGH_CONFIDENCE_MARKERS.has(token)) high.add(token)
    if (LOW_CONFIDENCE_MARKERS.has(token)) low.add(token)
  }
  return { high, low }
}

function findingForUnit(relative, unit) {
  const markers = getPtMarkers(unit.text)
  if (markers.high.size > 0) {
    return { path: relative, line: unit.line, rule: 'LANG-PT-001', scope: unit.scope }
  }
  if (markers.low.size >= 2) {
    return { path: relative, line: unit.line, rule: 'LANG-PT-002', scope: unit.scope }
  }
  return null
}

export function scanText(relative, text, lineNumbers = null) {
  if (!safeRelativePath(relative)) throw new LanguageError('unsafe-path')
  if (typeof text !== 'string') throw new LanguageError('invalid-text')
  const selected = lineNumbers === null ? null : new Set(lineNumbers)
  const findings = []
  for (const unit of extractUnits(relative, text)) {
    if (selected !== null && !selected.has(unit.line)) continue
    const finding = findingForUnit(relative, unit)
    if (finding) findings.push(finding)
  }
  return findings.sort(compareFindings)
}

export function scanBuffer(relative, buffer, lineNumbers = null) {
  if (!Buffer.isBuffer(buffer)) throw new LanguageError('invalid-buffer')
  if (buffer.length > MAX_FILE_BYTES) throw new LanguageError('file-size-limit')
  if (buffer.includes(0)) return []
  let text
  try {
    text = UTF8.decode(buffer)
  } catch {
    throw new LanguageError('invalid-utf8')
  }
  return scanText(relative, text, lineNumbers)
}

function rootPath(root) {
  try {
    return realpathSync(root)
  } catch {
    throw new LanguageError('repository-root-unavailable')
  }
}

function repositoryPath(root, relative) {
  if (!safeRelativePath(relative)) throw new LanguageError('unsafe-path')
  const realRoot = rootPath(root)
  const target = path.resolve(realRoot, relative)
  if (target !== realRoot && !target.startsWith(`${realRoot}${path.sep}`)) {
    throw new LanguageError('unsafe-path')
  }
  if (!existsSync(target)) throw new LanguageError('file-missing')
  let stat
  try {
    stat = lstatSync(target)
  } catch {
    throw new LanguageError('file-read-failed')
  }
  if (stat.isSymbolicLink()) {
    let resolved
    try {
      resolved = realpathSync(target)
    } catch {
      throw new LanguageError('unsafe-symlink')
    }
    if (resolved !== realRoot && !resolved.startsWith(`${realRoot}${path.sep}`)) {
      throw new LanguageError('unsafe-symlink')
    }
  }
  try {
    if (!statSync(target).isFile()) throw new LanguageError('file-not-regular')
  } catch (error) {
    if (error instanceof LanguageError) throw error
    throw new LanguageError('file-read-failed')
  }
  return target
}

export function scanFile(root, relative, lineNumbers = null) {
  const buffer = readRepositoryBuffer(root, relative)
  return scanBuffer(relative, buffer, lineNumbers)
}

function readRepositoryBuffer(root, relative) {
  const target = repositoryPath(root, relative)
  let buffer
  try {
    buffer = readFileSync(target)
  } catch {
    throw new LanguageError('file-read-failed')
  }
  return buffer
}

export function checkFile(filePath, root = REPO_ROOT) {
  const realRoot = rootPath(root)
  const relative = path.relative(realRoot, path.resolve(filePath)).split(path.sep).join('/')
  return scanFile(realRoot, relative)
}

function gitBuffer(root, args, allowStatusOne = false) {
  const result = spawnSync('git', args, {
    cwd: root,
    encoding: null,
    maxBuffer: 64 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (result.error || (result.status !== 0 && !(allowStatusOne && result.status === 1))) {
    throw new LanguageError('git-query-failed')
  }
  return Buffer.isBuffer(result.stdout) ? result.stdout : Buffer.alloc(0)
}

function decodeGitPaths(buffer) {
  let text
  try {
    text = UTF8.decode(buffer)
  } catch {
    throw new LanguageError('invalid-git-path')
  }
  const paths = text.split('\0').filter(Boolean)
  return validatePathList(paths)
}

export function enumerateTrackedPaths(root) {
  return decodeGitPaths(gitBuffer(root, ['ls-files', '-z']))
}

function safeBaseRef(baseRef) {
  return typeof baseRef === 'string' && baseRef.length > 0 && baseRef.length <= 256 &&
    /^[A-Za-z0-9][A-Za-z0-9._/~-]*$/.test(baseRef) && !baseRef.includes('..') &&
    !baseRef.includes('//') && !baseRef.endsWith('/')
}

function verifyBaseRef(root, baseRef) {
  if (!safeBaseRef(baseRef)) throw new LanguageError('invalid-base-ref')
  gitBuffer(root, ['rev-parse', '--verify', '--quiet', '--end-of-options', `${baseRef}^{commit}`])
}

function parseAddedLineNumbers(diff) {
  const lines = diff.split(/\r?\n/)
  const added = new Set()
  let current = null
  for (const line of lines) {
    const hunk = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
    if (hunk) {
      current = Number.parseInt(hunk[1], 10)
      continue
    }
    if (current === null) continue
    if (line.startsWith('+') && !line.startsWith('+++')) {
      added.add(current)
      current++
    } else if (line.startsWith(' ')) {
      current++
    }
  }
  return added
}

export function getDiffAddedLines(root, baseRef) {
  verifyBaseRef(root, baseRef)
  const paths = decodeGitPaths(gitBuffer(root, [
    'diff', '--no-ext-diff', '--no-textconv', '--name-only', '-z',
    '--diff-filter=ACMRTUXB', baseRef, '--',
  ]))
  const added = new Map()
  for (const relative of paths) {
    const output = gitBuffer(root, [
      'diff', '--no-ext-diff', '--no-textconv', '--unified=0',
      '--diff-filter=ACMRTUXB', baseRef, '--', relative,
    ], true)
    let lines = new Set()
    try {
      lines = parseAddedLineNumbers(UTF8.decode(output))
    } catch {
      // The selected file is scanned below, where its path policy and UTF-8
      // contract decide whether the bytes are opaque or a configuration error.
    }
    added.set(relative, lines)
  }
  return new Map([...added.entries()].sort(([left], [right]) => left.localeCompare(right)))
}

function appendFindings(target, additions, retained) {
  if (retained + additions.length > MAX_FINDINGS) throw new LanguageError('finding-count-limit')
  target.push(...additions)
  return retained + additions.length
}

function opaqueProtectedInventoryError(error, classification, mode) {
  return mode === 'all' && classification === 'protected' &&
    error instanceof LanguageError && error.message === 'invalid-utf8'
}

function splitFirstLine(buffer) {
  const newline = buffer.indexOf(0x0a)
  if (newline < 0) return null
  let lineEnd = newline
  if (lineEnd > 0 && buffer[lineEnd - 1] === 0x0d) lineEnd--
  return { line: buffer.subarray(0, lineEnd), suffix: buffer.subarray(newline + 1) }
}

function opaqueProtectedRootRedaction(root, baseRef, relative, current) {
  if (typeof baseRef !== 'string') return false
  let base
  try {
    base = gitBuffer(root, ['show', `${baseRef}:${relative}`])
  } catch {
    return false
  }
  const before = splitFirstLine(base)
  const after = splitFirstLine(current)
  if (before === null || after === null || after.line.toString('ascii') !== "'<repo-root>'") return false
  const privateRoot = before.line.toString('ascii')
  if (!/^'\\\\wsl(?:\.localhost)?\\[^\\\r\n]+\\home\\[^\\\r\n]+\\[^'\r\n]+'$/i.test(privateRoot)) return false
  return before.suffix.equals(after.suffix)
}

function countFindings(findings, classification) {
  return {
    files: new Set(findings.map((item) => item.path)).size,
    lines: findings.length,
    classification,
  }
}

function addProtectedDigest(digest, relative, buffer, totalBytes) {
  const nextTotal = totalBytes + buffer.length
  if (nextTotal > MAX_PROTECTED_INVENTORY_BYTES) {
    throw new LanguageError('protected-inventory-size-limit')
  }
  digest.update(relative, 'utf8')
  digest.update('\0', 'utf8')
  digest.update(createHash('sha256').update(buffer).digest())
  return nextTotal
}

function summarize(mutable, protectedCounts) {
  return {
    mutable_files: mutable.files,
    mutable_lines: mutable.lines,
    protected_files: protectedCounts.files,
    protected_lines: protectedCounts.lines,
    finding_count: mutable.lines + protectedCounts.lines,
  }
}

function snapshotFor(counts, protectedPaths, protectedDigest) {
  return {
    mutable_files: counts.mutable_files,
    mutable_lines: counts.mutable_lines,
    protected_files: counts.protected_files,
    protected_lines: counts.protected_lines,
    protected_paths: protectedPaths,
    protected_inventory_sha256: protectedDigest.digest('hex'),
  }
}

function scanEntries(root, entries, mode, baseRef) {
  const mutable = []
  const protectedFindings = []
  const protectedDigest = createHash('sha256')
  let protectedPaths = 0
  let protectedBytes = 0
  let retained = 0
  for (const [relative, lineNumbers] of entries) {
    const classification = classifyPath(relative)
    if (classification === 'localized' || classification === 'machine-policy-data') continue
    let buffer
    if (classification === 'protected' && mode === 'all') {
      buffer = readRepositoryBuffer(root, relative)
      protectedPaths++
      protectedBytes = addProtectedDigest(protectedDigest, relative, buffer, protectedBytes)
    }
    if (!scannablePath(relative)) continue
    let findings
    try {
      findings = scanBuffer(relative, buffer ?? readRepositoryBuffer(root, relative), lineNumbers)
    } catch (error) {
      if (opaqueProtectedInventoryError(error, classification, mode)) continue
      if (mode === 'diff' && classification === 'protected' &&
          error instanceof LanguageError && error.message === 'invalid-utf8' &&
          opaqueProtectedRootRedaction(root, baseRef, relative, readRepositoryBuffer(root, relative))) continue
      throw error
    }
    if (classification === 'protected') {
      retained = appendFindings(protectedFindings, findings, retained)
    } else {
      retained = appendFindings(mutable, findings, retained)
    }
  }
  const mutableCounts = countFindings(mutable, 'mutable')
  const protectedCounts = countFindings(protectedFindings, 'protected')
  const findings = [...mutable, ...protectedFindings].sort(compareFindings)
  const counts = summarize(mutableCounts, protectedCounts)
  return {
    findings,
    counts,
    snapshot: mode === 'all' ? snapshotFor(counts, protectedPaths, protectedDigest) : null,
  }
}

export function run({ root = REPO_ROOT, mode = 'all', baseRef } = {}) {
  if (mode !== 'all' && mode !== 'diff') throw new LanguageError('invalid-mode')
  const entries = mode === 'all'
    ? new Map(enumerateTrackedPaths(root).map((relative) => [relative, null]))
    : getDiffAddedLines(root, baseRef)
  const scanned = scanEntries(root, entries, mode, baseRef)
  return {
    ok: mode === 'all' ? scanned.counts.mutable_lines === 0 : scanned.findings.length === 0,
    findings: scanned.findings,
    counts: scanned.counts,
    snapshot: scanned.snapshot,
  }
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function hasExactKeys(value, keys) {
  if (!isRecord(value)) return false
  const actual = Object.keys(value).sort()
  const expected = [...keys].sort()
  return actual.length === expected.length && actual.every((key, index) => key === expected[index])
}

function nonNegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0
}

function validateSnapshot(value) {
  if (!hasExactKeys(value, SNAPSHOT_KEYS)) throw new LanguageError('invalid-ratchet-baseline')
  for (const key of SNAPSHOT_KEYS.slice(0, -1)) {
    if (!nonNegativeInteger(value[key])) throw new LanguageError('invalid-ratchet-baseline')
  }
  if (!/^[a-f0-9]{64}$/.test(value.protected_inventory_sha256)) {
    throw new LanguageError('invalid-ratchet-baseline')
  }
  return Object.fromEntries(SNAPSHOT_KEYS.map((key) => [key, value[key]]))
}

function validateApproval(value) {
  if (!hasExactKeys(value, APPROVAL_KEYS) ||
    value.approver_role !== RATCHET_APPROVER_ROLE ||
    value.channel !== RATCHET_REVIEW_CHANNEL) {
    throw new LanguageError('invalid-ratchet-baseline')
  }
  return {
    approver_role: value.approver_role,
    channel: value.channel,
  }
}

export function validateRatchetRecord(value) {
  if (!hasExactKeys(value, RATCHET_RECORD_KEYS) ||
    value.$schema !== RATCHET_SCHEMA_REFERENCE ||
    value.schema !== RATCHET_SCHEMA ||
    !nonNegativeInteger(value.revision)) {
    throw new LanguageError('invalid-ratchet-baseline')
  }
  return {
    $schema: value.$schema,
    schema: value.schema,
    revision: value.revision,
    approval: validateApproval(value.approval),
    initial: validateSnapshot(value.initial),
    current: validateSnapshot(value.current),
  }
}

function equalObject(left, right, keys) {
  return keys.every((key) => left[key] === right[key])
}

function equalSnapshot(left, right) {
  return equalObject(left, right, SNAPSHOT_KEYS)
}

function equalProtectedSnapshot(left, right) {
  return equalObject(left, right, SNAPSHOT_KEYS.slice(2))
}

function ratchetResult(ok, code, counts, revision) {
  return {
    ok,
    code,
    counts,
    revision,
    terminal: ok && counts.mutable_files === 0 && counts.mutable_lines === 0,
  }
}

function validateDecrease(base, current, counts, revision) {
  if (!equalProtectedSnapshot(base, current)) {
    return ratchetResult(false, 'ratchet-protected-drift', counts, revision)
  }
  if (current.mutable_files > base.mutable_files || current.mutable_lines > base.mutable_lines) {
    return ratchetResult(false, 'ratchet-mutable-growth', counts, revision)
  }
  const fileDecrease = base.mutable_files - current.mutable_files
  const lineDecrease = base.mutable_lines - current.mutable_lines
  if (fileDecrease === 0 && lineDecrease === 0) {
    return ratchetResult(false, 'ratchet-strict-decrease-required', counts, revision)
  }
  if (fileDecrease > MAX_RATCHET_BATCH_FILES) {
    return ratchetResult(false, 'ratchet-batch-file-limit', counts, revision)
  }
  if (lineDecrease > MAX_RATCHET_BATCH_LINES) {
    return ratchetResult(false, 'ratchet-batch-line-limit', counts, revision)
  }
  return ratchetResult(true, 'ratchet-pass', counts, revision)
}

export function evaluateRatchetTransition({
  baseRecord,
  currentRecord,
  observed,
  bootstrapSnapshot,
} = {}) {
  const current = validateRatchetRecord(currentRecord)
  const actual = validateSnapshot(observed)
  if (baseRecord === null) {
    if (bootstrapSnapshot === undefined) {
      throw new LanguageError('ratchet-bootstrap-not-authorized')
    }
    const bootstrap = validateSnapshot(bootstrapSnapshot)
    if (!equalSnapshot(current.initial, bootstrap)) {
      return ratchetResult(false, 'ratchet-bootstrap-snapshot-mismatch', actual, current.revision)
    }
    if (!equalSnapshot(current.current, actual)) {
      return ratchetResult(false, 'ratchet-snapshot-mismatch', actual, current.revision)
    }
    if (equalSnapshot(current.current, bootstrap)) {
      return current.revision === 0
        ? ratchetResult(true, 'ratchet-bootstrap-pass', actual, current.revision)
        : ratchetResult(false, 'ratchet-revision-mismatch', actual, current.revision)
    }
    if (current.revision !== 1) {
      return ratchetResult(false, 'ratchet-revision-mismatch', actual, current.revision)
    }
    return validateDecrease(bootstrap, current.current, actual, current.revision)
  }

  const base = validateRatchetRecord(baseRecord)
  if (!equalSnapshot(current.initial, base.initial)) {
    return ratchetResult(false, 'ratchet-initial-changed', actual, current.revision)
  }
  if (!equalObject(current.approval, base.approval, APPROVAL_KEYS)) {
    return ratchetResult(false, 'ratchet-approval-mismatch', actual, current.revision)
  }
  if (!equalSnapshot(current.current, actual)) {
    return ratchetResult(false, 'ratchet-snapshot-mismatch', actual, current.revision)
  }
  if (equalSnapshot(current.current, base.current)) {
    return current.revision === base.revision
      ? ratchetResult(true, 'ratchet-unchanged-pass', actual, current.revision)
      : ratchetResult(false, 'ratchet-revision-mismatch', actual, current.revision)
  }
  if (current.revision !== base.revision + 1) {
    return ratchetResult(false, 'ratchet-revision-mismatch', actual, current.revision)
  }
  return validateDecrease(base.current, current.current, actual, current.revision)
}

function parseRatchetRecord(buffer) {
  if (!Buffer.isBuffer(buffer) || buffer.length > MAX_FILE_BYTES) {
    throw new LanguageError('invalid-ratchet-baseline')
  }
  let text
  try {
    text = UTF8.decode(buffer)
  } catch {
    throw new LanguageError('invalid-ratchet-baseline')
  }
  try {
    return validateRatchetRecord(JSON.parse(text))
  } catch (error) {
    if (error instanceof LanguageError) throw error
    throw new LanguageError('invalid-ratchet-baseline')
  }
}

function readCurrentRatchetRecord(root) {
  return parseRatchetRecord(readRepositoryBuffer(root, RATCHET_BASELINE_PATH))
}

function readBaseRatchetRecord(root, baseRef) {
  verifyBaseRef(root, baseRef)
  const objectName = `${baseRef}:${RATCHET_BASELINE_PATH}`
  const probe = spawnSync('git', ['cat-file', '-e', '--end-of-options', objectName], {
    cwd: root,
    encoding: null,
    stdio: ['ignore', 'ignore', 'ignore'],
  })
  if (probe.error) throw new LanguageError('git-query-failed')
  if (probe.status === 128) return null
  if (probe.status !== 0) throw new LanguageError('git-query-failed')
  return parseRatchetRecord(gitBuffer(root, [
    'show', '--no-textconv', '--format=', '--end-of-options', objectName,
  ]))
}

export function runRatchet({ root = REPO_ROOT, baseRef } = {}) {
  const baseRecord = readBaseRatchetRecord(root, baseRef)
  if (baseRecord === null) throw new LanguageError('ratchet-baseline-missing')
  const currentRecord = readCurrentRatchetRecord(root)
  const observed = run({ root, mode: 'all' }).snapshot
  return evaluateRatchetTransition({ baseRecord, currentRecord, observed })
}

export function formatFinding(finding) {
  return `${finding.path}:${finding.line} — ${finding.rule} — ${finding.scope}`
}

function formatSummary(counts) {
  const findingCount = counts.finding_count ?? counts.mutable_lines + counts.protected_lines
  return [
    'SUMMARY',
    `mutable_files=${counts.mutable_files}`,
    `mutable_lines=${counts.mutable_lines}`,
    `protected_files=${counts.protected_files}`,
    `protected_lines=${counts.protected_lines}`,
    `finding_count=${findingCount}`,
  ].join(' ')
}

export function main(argv = process.argv.slice(2), root = REPO_ROOT, io = console) {
  let mode
  let baseRef
  if (argv.length === 1 && argv[0] === '--all') {
    mode = 'all'
  } else if (argv.length === 2 && argv[0] === '--diff') {
    mode = 'diff'
    baseRef = argv[1]
  } else if (argv.length === 2 && argv[0] === '--ratchet') {
    mode = 'ratchet'
    baseRef = argv[1]
  } else {
    io.error('usage: check-comment-language.mjs --all | --diff <base> | --ratchet <base>')
    return 2
  }
  try {
    const result = mode === 'ratchet'
      ? runRatchet({ root, baseRef })
      : run({ root, mode, baseRef })
    io.log('SCOPE=comment-language')
    io.log(`MODE=${mode}`)
    for (const finding of result.findings ?? []) io.error(formatFinding(finding))
    io.log(formatSummary(result.counts))
    if (mode === 'ratchet') {
      io.log(`RATCHET_REVISION=${result.revision}`)
      io.log(`RATCHET_RESULT=${result.code}`)
    }
    io.log(`COMMENT_LANGUAGE_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
    return result.ok ? 0 : 1
  } catch (error) {
    const reason = error instanceof LanguageError ? error.message : 'unexpected-error'
    io.error(`COMMENT_LANGUAGE_ERROR=${reason}`)
    return 2
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main()
}
