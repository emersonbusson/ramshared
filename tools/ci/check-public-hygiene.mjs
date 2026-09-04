#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, readlinkSync, realpathSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { TextDecoder } from 'node:util'
import { fileURLToPath } from 'node:url'
import { inflateSync } from 'node:zlib'

const MAX_FILES = 20_000
const MAX_FILE_BYTES = 512 * 1024
const MAX_PUBLIC_BINARY_BYTES = 8 * 1024 * 1024
const MAX_PNG_DECODED_BYTES = 64 * 1024 * 1024
const MAX_PNG_TEXT_BYTES = 64 * 1024
const MAX_PNG_TEXT_TOTAL_BYTES = 256 * 1024
const MAX_JPEG_PIXELS = 64 * 1024 * 1024
const MAX_JPEG_BLOCKS = 2 * 1024 * 1024
const MAX_PUBLIC_JPEG_ENTRIES = 256
const MODES = new Set(['candidate', 'tracked', 'staged'])
// `ignoreBOM: true` deliberately preserves U+FEFF in decoded output. The
// default TextDecoder behavior consumes a leading BOM, which would otherwise
// hide a forbidden format character at the start of content or the first Git
// path returned by a NUL-delimited query.
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true })
const EMPTY_TREE = '4b825dc642cb6eb9a060e54bf8d69288fbee4904'
const ALLOWLIST_FILE = 'docs/governance/public-hygiene-allowlist.json'
const REDACTION_LEDGER_FILE = 'docs/governance/public-hygiene-redactions.jsonl'
const PUBLIC_BINARY_DIGESTS_FILE = 'docs/governance/public-binary-digests.json'
const PUBLIC_BINARY_DIGESTS_SCHEMA_FILE = 'docs/governance/public-binary-digests.schema.json'
const PUBLIC_BINARY_DIGEST_SCHEMA = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $id: 'public-binary-digests.schema.json',
  title: 'RamShared reviewed public JPEG digest manifest',
  type: 'object',
  additionalProperties: false,
  required: ['$schema', 'schema_version', 'entries'],
  properties: {
    $schema: { const: './public-binary-digests.schema.json' },
    schema_version: { const: 'ramshared-public-binary-digests/v1' },
    entries: {
      type: 'array',
      maxItems: MAX_PUBLIC_JPEG_ENTRIES,
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['path', 'size', 'sha256'],
        properties: {
          path: { type: 'string', pattern: '^docs/.+\\.jpe?g$' },
          size: { type: 'integer', minimum: 1, maximum: MAX_PUBLIC_BINARY_BYTES },
          sha256: { type: 'string', pattern: '^[0-9a-f]{64}$' },
        },
      },
    },
  },
}
// Public artifacts are text unless their extension and bytes satisfy one of
// these deliberately small, bounded contracts. SVG remains text. A new binary
// format must be reviewed and added here instead of inheriting a NUL/UTF-8
// heuristic that could let hostile public text bypass inspection.
const PUBLIC_BINARY_CONTRACTS = new Map([
  ['.jpg', {
    prefix: [0xff, 0xd8, 0xff],
    suffix: [0xff, 0xd9],
    validate: validateJpeg,
  }],
  ['.jpeg', {
    prefix: [0xff, 0xd8, 0xff],
    suffix: [0xff, 0xd9],
    validate: validateJpeg,
  }],
  ['.png', {
    prefix: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
    suffix: [0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82],
    validate: validatePng,
  }],
])
// Git names are opaque byte sequences. Refuse controls and format characters before
// any normalization, filesystem lookup, or diagnostic rendering. This makes bidi
// spoofing and path-separator ambiguity fail closed.
const CONTROL_OR_AMBIGUOUS_PATH = /[\p{Cc}\p{Cf}\\]/u

class HygieneError extends Error {}

function git(root, args, encoding = 'buffer') {
  try {
    return execFileSync('git', args, {
      cwd: root,
      encoding,
      maxBuffer: 64 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch {
    throw new HygieneError('git-query-failed')
  }
}

function splitZero(buffer) {
  try {
    return UTF8.decode(buffer).split('\0').filter(Boolean)
  } catch {
    throw new HygieneError('invalid-path-encoding')
  }
}

function hasSafeSegments(file) {
  return file.split('/').every((segment) => segment && segment !== '.' && segment !== '..')
}

/** Validate the exact Git path before normalization or filesystem use. */
export function isSafeRepoPath(root, file) {
  if (typeof root !== 'string' || typeof file !== 'string' || !file ||
      CONTROL_OR_AMBIGUOUS_PATH.test(file) || path.posix.isAbsolute(file) ||
      path.win32.isAbsolute(file) || file.startsWith('-') || file.startsWith(':') ||
      !hasSafeSegments(file)) return false
  const resolvedRoot = path.resolve(root)
  const resolvedFile = path.resolve(resolvedRoot, file)
  const relative = path.relative(resolvedRoot, resolvedFile)
  return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)
}

function parseIndexSnapshot(buffer) {
  let records
  try {
    records = UTF8.decode(buffer).split('\0').filter(Boolean)
  } catch {
    throw new HygieneError('invalid-path-encoding')
  }
  if (records.length > MAX_FILES) throw new HygieneError('file-count-limit')
  const entries = new Map()
  for (const record of records) {
    const match = record.match(/^(100644|100755|120000) ([0-9a-f]{40}|[0-9a-f]{64}) 0\t([\s\S]+)$/u)
    if (!match || entries.has(match?.[3])) throw new HygieneError('staged-entry-type-unsupported')
    entries.set(match[3], {
      mode: match[1],
      oid: match[2],
      kind: match[1] === '120000' ? 'symlink' : 'file',
    })
  }
  return entries
}

function parseTreeSnapshot(buffer) {
  let records
  try {
    records = UTF8.decode(buffer).split('\0').filter(Boolean)
  } catch {
    throw new HygieneError('invalid-path-encoding')
  }
  if (records.length > MAX_FILES) throw new HygieneError('file-count-limit')
  const entries = new Map()
  for (const record of records) {
    const match = record.match(/^(100644|100755|120000) blob ([0-9a-f]{40}|[0-9a-f]{64})\t([\s\S]+)$/u)
    if (!match || entries.has(match?.[3])) throw new HygieneError('authority-entry-type-unsupported')
    entries.set(match[3], {
      mode: match[1],
      oid: match[2],
      kind: match[1] === '120000' ? 'symlink' : 'file',
    })
  }
  return entries
}

function resolveGitSnapshot(root) {
  const headOid = git(root, ['rev-parse', '--verify', 'HEAD^{commit}'], 'utf8').trim()
  if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(headOid)) {
    throw new HygieneError('git-head-snapshot-invalid')
  }
  const indexRaw = git(root, ['ls-files', '--stage', '-z'])
  const headTreeRaw = git(root, ['ls-tree', '-r', '-z', headOid, '--'])
  return {
    headOid,
    indexRaw,
    indexEntries: parseIndexSnapshot(indexRaw),
    headEntries: parseTreeSnapshot(headTreeRaw),
  }
}

function assertIndexSnapshotUnchanged(root, snapshot) {
  const current = git(root, ['ls-files', '--stage', '-z'])
  if (!current.equals(snapshot.indexRaw)) throw new HygieneError('git-index-snapshot-changed')
}

export function enumerateFiles(root, mode = 'candidate', snapshot = null) {
  if (!MODES.has(mode)) throw new HygieneError('invalid-mode')
  if (snapshot !== null) {
    const indexed = [...snapshot.indexEntries.keys()]
    if (mode !== 'candidate') return indexed.sort((a, b) => a.localeCompare(b))
    const deleted = new Set(splitZero(git(root, ['ls-files', '--deleted', '-z'])))
    const untracked = splitZero(git(root, ['ls-files', '--others', '--exclude-standard', '-z']))
    const files = [...indexed.filter((file) => !deleted.has(file)), ...untracked]
      .sort((a, b) => a.localeCompare(b))
    if (files.length > MAX_FILES) throw new HygieneError('file-count-limit')
    return files
  }
  const args = mode === 'candidate'
    ? ['ls-files', '-co', '--exclude-standard', '-z']
    : mode === 'staged'
      ? ['ls-files', '--cached', '-z']
      : ['ls-files', '-z']
  const deleted = mode === 'candidate'
    ? new Set(splitZero(git(root, ['ls-files', '--deleted', '-z'])))
    : new Set()
  const files = splitZero(git(root, args))
    .filter((file) => !deleted.has(file))
    .sort((a, b) => a.localeCompare(b))
  if (files.length > MAX_FILES) throw new HygieneError('file-count-limit')
  return files
}

function realRoot(root) {
  try {
    return realpathSync.native(root)
  } catch {
    throw new HygieneError('root-realpath-failed')
  }
}

function isContained(root, candidate) {
  const relative = path.relative(root, candidate)
  return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)
}

function workingTreePath(root, file) {
  if (!isSafeRepoPath(root, file)) throw new HygieneError('unsafe-path')
  const canonicalRoot = realRoot(root)
  const lexical = path.resolve(canonicalRoot, file)
  if (!isContained(canonicalRoot, lexical)) throw new HygieneError('unsafe-path')
  try {
    const canonicalParent = realpathSync.native(path.dirname(lexical))
    if (canonicalParent !== canonicalRoot && !isContained(canonicalRoot, canonicalParent)) {
      throw new HygieneError('candidate-parent-symlink-outside-root')
    }
    return lexical
  } catch (error) {
    if (error instanceof HygieneError) throw error
    throw new HygieneError('candidate-path-unreadable')
  }
}

function revisionKind(root, revision, file) {
  const output = UTF8.decode(git(root, ['ls-tree', '-z', revision, '--', file]))
  const entries = output.split('\0').filter(Boolean)
  if (entries.length === 0) return null
  if (entries.length !== 1) throw new HygieneError('authority-entry-ambiguous')
  const match = entries[0].match(/^(100644|100755|120000) blob [0-9a-f]+\t/u)
  if (!match) throw new HygieneError('authority-entry-type-unsupported')
  return match[1] === '120000' ? 'symlink' : 'file'
}

function revisionFiles(root, revision) {
  return splitZero(git(root, ['ls-tree', '-r', '--name-only', '-z', revision, '--']))
}

function revisionBuffer(root, revision, file) {
  return git(root, ['show', `${revision}:${file}`])
}

function readWorkingTree(root, file) {
  const candidate = workingTreePath(root, file)
  try {
    const stat = lstatSync(candidate)
    if (stat.isSymbolicLink()) {
      return { buffer: readlinkSync(candidate, { encoding: 'buffer' }), kind: 'symlink' }
    }
    if (!stat.isFile()) throw new HygieneError('candidate-entry-type-unsupported')
    return { buffer: readFileSync(candidate), kind: 'file' }
  } catch (error) {
    if (error instanceof HygieneError) throw error
    throw new HygieneError('file-read-failed')
  }
}

function readCandidate(root, file, mode, snapshot) {
  if (!isSafeRepoPath(root, file)) throw new HygieneError('unsafe-path')
  try {
    if (mode !== 'staged') return readWorkingTree(root, file)
    const entry = snapshot?.indexEntries.get(file)
    if (!entry) throw new HygieneError('staged-entry-ambiguous')
    return { buffer: git(root, ['cat-file', 'blob', entry.oid]), kind: entry.kind }
  } catch (error) {
    if (error instanceof HygieneError) throw error
    throw new HygieneError('file-read-failed')
  }
}

export function classifyText(buffer, file = '') {
  if (buffer.includes(0)) return false
  try {
    const text = UTF8.decode(buffer)
    if (text.startsWith('#!')) return true
    const basename = path.posix.basename(file)
    if (['Dockerfile', 'Makefile', 'Kconfig'].includes(basename)) return true
    return true
  } catch {
    return false
  }
}

function lineNumber(text, offset) {
  let line = 1
  for (let index = 0; index < offset; index++) if (text.charCodeAt(index) === 10) line++
  return line
}

function allMatches(expression, text) {
  const flags = expression.flags.includes('g') ? expression.flags : `${expression.flags}g`
  const matcher = new RegExp(expression.source, flags)
  const matches = []
  for (let match = matcher.exec(text); match; match = matcher.exec(text)) {
    matches.push(match)
    if (match[0] === '') matcher.lastIndex++
  }
  return matches
}

function unicodeControlReason(character) {
  return `unsafe-unicode-content-u+${character.codePointAt(0).toString(16).padStart(4, '0')}`
}

function isNormalizedTextControl(character) {
  return character === '\t' || character === '\n' || character === '\r'
}

/**
 * Report nonprinting public text without placing it in terminal diagnostics.
 * CR, LF, and tab are normal repository text separators; every other Cc and
 * every Cf (including bidi format characters) is refused.
 */
export function scanUnsafeUnicodeContent(file, text) {
  const findings = []
  for (const match of allMatches(/[\p{Cc}\p{Cf}]/u, text)) {
    if (isNormalizedTextControl(match[0])) continue
    findings.push({
      path: file,
      line: lineNumber(text, match.index),
      rule: 'UNSAFE_UNICODE_CONTENT',
      reason: unicodeControlReason(match[0]),
    })
  }
  return findings
}

const rules = [
  ['EXAMPLE_APP_NAME', new RegExp(`\\b(?:${['blen', 'der'].join('')}|${['battle', 'field'].join('')}|after[ -]?${['eff', 'ects'].join('')})\\b`, 'i'), 'example-application-name'],
  ['LEGACY_SCRIPT_NAME', new RegExp(`\\b(?:measure-)?${['render', 'vram'].join('-')}\\b`, 'i'), 'legacy-application-script-name'],
  ['PRIVATE_UNIX_PATH', /\/home\/(?!<|\$|user(?:\/|>))[A-Za-z0-9._-]+\//i, 'private-unix-path'],
  ['PRIVATE_WINDOWS_PATH', /\b[A-Za-z]:\\Users\\(?!<|Public\\|Default\\|%)[^\\\s"']+\\/i, 'private-windows-profile-path'],
  ['PRIVATE_WSL_PATH', /\\\\wsl(?:\.localhost)?\\[^\\\s]+\\(?:home\\)?[^\\\s]+\\/i, 'private-wsl-path'],
  ['EMAIL', /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i, 'personal-email-address'],
  ['TOKEN', /\b(?:gh[opusr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|AKIA[A-Z0-9]{16})\b/, 'credential-token'],
  ['PRIVATE_KEY', new RegExp(['-{5}BEGIN ', '(?:RSA |OPENSSH |EC |DSA )?', 'PRIVATE KEY-{5}'].join('')), 'private-key-material'],
  ['KERNEL_ADDRESS', /\bffff[0-9a-f]{8,}\b/i, 'raw-kernel-address'],
  ['INLINE_PFX_PASSWORD', /-PfxPassword\s+["'][^"']+["']/i, 'inline-signing-password'],
  ['INLINE_DRILL_PASSWORD', /RAMSHARED_DRILL_PASSWORD\s*=\s*["'](?!<|REDACTED|…)[^"']+["']/i, 'inline-drill-password'],
]

const IPV4_OCTET = '(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)'

// SANITIZED_* is explicitly non-identifying. Historical markers never suppress a hit.
const publicDocumentRules = [
  ['RAW_LAB_VM', /\b(?:linux-kernel-lab|gha-ubuntu-2404|win(?:dows)?[-_]?\d{1,2}-(?:wsl2?|driver|kernel|lab|drill)[A-Za-z0-9_-]*)\b/i, 'unredacted-lab-vm-identity'],
  ['PRIVATE_WINDOWS_ARTIFACT_RUN', /\b[A-Za-z]:\\(?:ramshared|artifacts?|hyper-v)(?:\\(?!SANITIZED_|<)[^\\\r\n`"']+)?/i, 'private-windows-artifact-or-host-path'],
  ['RAW_ARTIFACT_RUN_PATH', /\/(?:tmp|var\/tmp|opt)\/(?:ramshared|artifact|run)[A-Za-z0-9_./-]*/i, 'unredacted-artifact-run-path'],
  ['RAW_TIMESTAMP_RUN_ID', /\b(?!SANITIZED_)[A-Za-z][A-Za-z0-9_]*(?:-[A-Za-z0-9_]+)*-\d{8}(?:[-T_]\d{6})\b/i, 'unredacted-timestamped-run-identity'],
  ['RAW_UUID', /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/i, 'unredacted-run-identity'],
  ['RAW_DEVICE', /\/dev\/(?:sd[a-z]+\d*|nvme\d+n\d+(?:p\d+)?|mapper\/[A-Za-z0-9_.-]+)\b/i, 'unredacted-device-identity'],
  ['RAW_PRINCIPAL', /\b(?!(?:SANITIZED_))(?:win\d{1,2}[-_][A-Za-z0-9_.-]*|ramshared[-_][A-Za-z0-9_.-]*|[A-Za-z0-9_.-]*[-_]lab)\\[A-Za-z0-9_.-]+\b/i, 'unredacted-principal'],
  ['PRIVATE_IP', new RegExp(`\\b(?:10(?:\\.${IPV4_OCTET}){3}|192\\.168(?:\\.${IPV4_OCTET}){2}|172\\.(?:1[6-9]|2\\d|3[0-1])(?:\\.${IPV4_OCTET}){2})\\b`), 'private-ip-address'],
]
const PUBLIC_IDENTITY_RULES = new Set(publicDocumentRules.map(([rule]) => rule))
const REDACTION_KEYS = [
  'schema_version', 'redaction_id', 'applied_at', 'source_revision', 'path',
  'source_line', 'replacement_line', 'rule', 'replacement_class', 'supersedes_line_sha256',
  'replacement_line_sha256', 'reason',
]
const LOWER_SHA256 = /^[0-9a-f]{64}$/
const LOWER_REVISION = /^[0-9a-f]{40}$/

const LOCAL_SAFETY_MARKER = /(?:historical|dated).{0,140}(?:non[\s/-]*current|no[\s/-]*execution|do not execute|must not be repeated|disabled[\s/-]*staging|inert[\s/-]*only)|(?:disabled[\s/-]*staging|inert[\s/-]*only).{0,140}(?:no[\s/-]*execution|not a current (?:execution|activation|campaign)|do not execute)/i
const ACTIVATION_COMMAND = /(?:\b(?:mkswap|swapon|swapoff|mkfs(?:\.[A-Za-z0-9_-]+)?|mount(?:-vhd)?|umount|initialize-disk|format-(?:volume|disk)|new-(?:vhd|partition)|clear-disk|dismount-vhd|install-[A-Za-z0-9_-]+|start-vm|stop-vm|set-vm|restart-computer|enable-windowsoptionalfeature|modprobe|systemctl)\b|\bwsl(?:\.exe)?\s+(?:-d\b|--(?:terminate|shutdown|install|update))|\bwslconfig(?:-ctl\.sh)?\s+apply\b|\bramshared\s+(?:up|down)\b|--run-(?:isolated|shared-daily-host)\b|\b(?:invoke-)?(?:shared)?wsl(?:2)?(?:freeze|pressure)[A-Za-z0-9_.-]*\b|\bcascade-pressure-probe\b|\b(?:origin|pressure)[-_ ]?(?:route|probe|campaign)\b|\bdd\s+[^\n]*\bof=\/dev\/)/i

function hasPrecedingSafetyMarker(lines, line) {
  const start = Math.max(0, line - 3)
  for (let index = line - 1; index >= start; index--) {
    if (/^\s*```/.test(lines[index])) break
    if (LOCAL_SAFETY_MARKER.test(lines[index])) return true
  }
  // A marker directly before a fenced transcript governs that one closed block,
  // not subsequent prose or another fence. A marker after a command never counts.
  let openingFence = -1
  for (let index = 0; index < line; index++) {
    if (!/^\s*```/.test(lines[index])) continue
    openingFence = openingFence === -1 ? index : -1
  }
  if (openingFence !== -1) return lines.slice(Math.max(0, openingFence - 3), openingFence).some((candidate) => LOCAL_SAFETY_MARKER.test(candidate))
  return false
}

function isPublicArtifact(file) {
  return file === 'README.md' || file === 'README.pt-BR.md' || file === 'validation.md' || file.startsWith('docs/')
}

function publicBinaryContract(file) {
  return PUBLIC_BINARY_CONTRACTS.get(path.posix.extname(file).toLowerCase()) ?? null
}

function matchesPublicBinarySignature(buffer, contract) {
  const minimumLength = contract.prefix.length + contract.suffix.length
  const suffixOffset = buffer.length - contract.suffix.length
  return buffer.length >= minimumLength &&
    contract.prefix.every((byte, index) => buffer[index] === byte) &&
    contract.suffix.every((byte, index) => buffer[suffixOffset + index] === byte)
}

function crc32(buffer, start, end) {
  let crc = 0xffffffff
  for (let index = start; index < end; index++) {
    crc ^= buffer[index]
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1))
  }
  return (crc ^ 0xffffffff) >>> 0
}

function parsePngHeader(buffer, dataStart, length) {
  if (length !== 13) return { reason: 'public-png-ihdr-invalid' }
  const width = buffer.readUInt32BE(dataStart)
  const height = buffer.readUInt32BE(dataStart + 4)
  const bitDepth = buffer[dataStart + 8]
  const colorType = buffer[dataStart + 9]
  const channels = new Map([[0, 1], [2, 3], [3, 1], [4, 2], [6, 4]])
  const validDepths = new Map([
    [0, new Set([1, 2, 4, 8, 16])],
    [2, new Set([8, 16])],
    [3, new Set([1, 2, 4, 8])],
    [4, new Set([8, 16])],
    [6, new Set([8, 16])],
  ])
  if (width === 0 || height === 0 || validDepths.get(colorType)?.has(bitDepth) !== true ||
      buffer[dataStart + 10] !== 0 || buffer[dataStart + 11] !== 0 ||
      buffer[dataStart + 12] !== 0) {
    return { reason: 'public-png-ihdr-invalid' }
  }
  const bitsPerPixel = BigInt(channels.get(colorType) * bitDepth)
  const rowBytes = ((BigInt(width) * bitsPerPixel) + 7n) / 8n
  const expectedBytes = (rowBytes + 1n) * BigInt(height)
  if (expectedBytes > BigInt(MAX_PNG_DECODED_BYTES) || expectedBytes > BigInt(Number.MAX_SAFE_INTEGER)) {
    return { reason: 'public-png-decoded-size-limit' }
  }
  return {
    width,
    height,
    bitDepth,
    colorType,
    rowBytes: Number(rowBytes),
    expectedBytes: Number(expectedBytes),
  }
}

function binaryMetadataTexts(buffer) {
  const texts = [buffer.toString('latin1')]
  for (const littleEndian of [true, false]) {
    for (const alignment of [0, 1]) {
      let text = ''
      for (let offset = alignment; offset + 1 < buffer.length; offset += 2) {
        const code = littleEndian
          ? buffer[offset] | (buffer[offset + 1] << 8)
          : (buffer[offset] << 8) | buffer[offset + 1]
        text += code >= 0x20 && code <= 0x7e ? String.fromCharCode(code) : '\n'
      }
      texts.push(text)
    }
  }
  return texts
}

function binaryMetadataHasSensitiveValue(buffer, allowContentCredentialUuid = false) {
  const documentRules = publicDocumentRules.filter(([rule]) =>
    !allowContentCredentialUuid || rule !== 'RAW_UUID')
  return binaryMetadataTexts(buffer).some((text) =>
    scanRuleMatches('<binary-metadata>', text, [], rules).length > 0 ||
    scanRuleMatches('<binary-metadata>', text, [], documentRules).length > 0)
}

const JUMBF_UUID_TAIL = Buffer.from('00110010800000aa00389b71', 'hex')
const C2PA_CLAIM_LABEL = /^urn:c2pa:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const C2PA_JUMBF_PROFILE = {
  contentType: 'c2pa',
  label: 'c2pa',
  children: [{
    contentType: 'c2ma',
    label: C2PA_CLAIM_LABEL,
    children: [
      { contentType: 'c2cs', label: 'c2pa.signature', leaf: 'cbor' },
      { contentType: 'c2cl', label: 'c2pa.claim.v2', leaf: 'cbor' },
      {
        contentType: 'c2as',
        label: 'c2pa.assertions',
        children: [
          { contentType: 'cbor', label: 'c2pa.hash.data', leaf: 'cbor' },
          { contentType: 'cbor', label: 'c2pa.actions.v2', leaf: 'cbor' },
        ],
      },
    ],
  }],
}

function parseJumbfBoxes(buffer, start, end) {
  const boxes = []
  let offset = start
  while (offset < end) {
    if (offset + 8 > end) return null
    const size = buffer.readUInt32BE(offset)
    const typeBytes = buffer.subarray(offset + 4, offset + 8)
    if (size < 8 || offset + size > end ||
        !typeBytes.every((byte) => (byte >= 0x61 && byte <= 0x7a) || (byte >= 0x30 && byte <= 0x39))) {
      return null
    }
    boxes.push({ type: typeBytes.toString('ascii'), start: offset + 8, end: offset + size })
    offset += size
  }
  return offset === end ? boxes : null
}

function validJumbfDescription(buffer, box, profile) {
  const body = buffer.subarray(box.start, box.end)
  if (box.type !== 'jumd' || body.length < 19 ||
      body.subarray(0, 4).toString('ascii') !== profile.contentType ||
      !body.subarray(4, 16).equals(JUMBF_UUID_TAIL) || body[16] !== 0x03 ||
      body[body.length - 1] !== 0x00 || body.subarray(17, -1).includes(0) ||
      !body.subarray(17, -1).every((byte) => byte >= 0x21 && byte <= 0x7e)) return false
  const label = body.subarray(17, -1).toString('ascii')
  return typeof profile.label === 'string' ? label === profile.label : profile.label.test(label)
}

function validateC2paJumbfNode(buffer, box, profile) {
  if (box.type !== 'jumb') return 'invalid'
  const children = parseJumbfBoxes(buffer, box.start, box.end)
  const expectedChildren = profile.children ?? []
  const expectedCount = 1 + (profile.leaf ? 1 : expectedChildren.length)
  if (children === null || children.length !== expectedCount ||
      !validJumbfDescription(buffer, children[0], profile)) return 'invalid'
  if (profile.leaf) {
    const leaf = children[1]
    if (leaf.type !== profile.leaf || leaf.start === leaf.end) return 'invalid'
    return binaryMetadataHasSensitiveValue(buffer.subarray(leaf.start, leaf.end), true)
      ? 'sensitive'
      : 'valid'
  }
  for (let index = 0; index < expectedChildren.length; index++) {
    const result = validateC2paJumbfNode(buffer, children[index + 1], expectedChildren[index])
    if (result !== 'valid') return result
  }
  return 'valid'
}

function validateC2paApp11(buffer) {
  if (buffer.length < 16 || !buffer.subarray(0, 4).equals(Buffer.from([0x4a, 0x50, 0x00, 0x01])) ||
      buffer.readUInt32BE(4) !== 1) return 'invalid'
  const boxes = parseJumbfBoxes(buffer, 8, buffer.length)
  return boxes?.length === 1
    ? validateC2paJumbfNode(buffer, boxes[0], C2PA_JUMBF_PROFILE)
    : 'invalid'
}

function validPngKeyword(buffer) {
  if (buffer.length < 1 || buffer.length > 79 || buffer[0] === 0x20 ||
      buffer[buffer.length - 1] === 0x20) return false
  let previousSpace = false
  for (const byte of buffer) {
    const space = byte === 0x20
    if (previousSpace && space) return false
    if (byte === 0 || byte < 0x20 || (byte >= 0x7f && byte <= 0xa0)) return false
    previousSpace = space
  }
  return true
}

function inflatePngText(buffer) {
  try {
    const inflated = inflateSync(buffer, {
      info: true,
      maxOutputLength: MAX_PNG_TEXT_BYTES + 1,
    })
    if (!Number.isSafeInteger(inflated.engine?.bytesWritten) ||
        inflated.engine.bytesWritten !== buffer.length) return { reason: 'public-png-ancillary-invalid' }
    if (inflated.buffer.length > MAX_PNG_TEXT_BYTES) return { reason: 'public-png-ancillary-size-limit' }
    return { buffer: inflated.buffer, reason: null }
  } catch (error) {
    return { reason: error?.code === 'ERR_BUFFER_TOO_LARGE'
      ? 'public-png-ancillary-size-limit'
      : 'public-png-ancillary-invalid' }
  }
}

function pngTextIsSensitive(parts) {
  return parts.some((part) => binaryMetadataHasSensitiveValue(part) || (() => {
    let text
    try {
      text = UTF8.decode(part)
    } catch {
      return false
    }
    return scanUnsafeUnicodeContent('<png-metadata>', text).length > 0
  })())
}

function parsePngTextChunk(type, data) {
  const keywordEnd = data.indexOf(0)
  if (keywordEnd < 1 || !validPngKeyword(data.subarray(0, keywordEnd))) {
    return { reason: 'public-png-ancillary-invalid' }
  }
  const keyword = data.subarray(0, keywordEnd)
  if (type === 'tEXt') {
    const text = data.subarray(keywordEnd + 1)
    if (text.length > MAX_PNG_TEXT_BYTES || text.includes(0)) {
      return { reason: text.length > MAX_PNG_TEXT_BYTES
        ? 'public-png-ancillary-size-limit'
        : 'public-png-ancillary-invalid' }
    }
    return { reason: pngTextIsSensitive([keyword, text]) ? 'public-png-ancillary-sensitive' : null, bytes: text.length }
  }
  if (type === 'zTXt') {
    if (data[keywordEnd + 1] !== 0 || keywordEnd + 2 >= data.length) {
      return { reason: 'public-png-ancillary-invalid' }
    }
    const inflated = inflatePngText(data.subarray(keywordEnd + 2))
    if (inflated.reason !== null) return inflated
    return {
      reason: pngTextIsSensitive([keyword, inflated.buffer]) ? 'public-png-ancillary-sensitive' : null,
      bytes: inflated.buffer.length,
    }
  }
  if (type !== 'iTXt' || keywordEnd + 3 > data.length) {
    return { reason: 'public-png-ancillary-invalid' }
  }
  const compressed = data[keywordEnd + 1]
  const method = data[keywordEnd + 2]
  if (![0, 1].includes(compressed) || method !== 0) return { reason: 'public-png-ancillary-invalid' }
  const languageStart = keywordEnd + 3
  const languageEnd = data.indexOf(0, languageStart)
  const translatedStart = languageEnd + 1
  const translatedEnd = languageEnd < 0 ? -1 : data.indexOf(0, translatedStart)
  if (languageEnd < 0 || translatedEnd < 0) return { reason: 'public-png-ancillary-invalid' }
  const language = data.subarray(languageStart, languageEnd)
  if (!language.every((byte) =>
    (byte >= 0x41 && byte <= 0x5a) || (byte >= 0x61 && byte <= 0x7a) ||
    (byte >= 0x30 && byte <= 0x39) || byte === 0x2d)) {
    return { reason: 'public-png-ancillary-invalid' }
  }
  const translated = data.subarray(translatedStart, translatedEnd)
  let text = data.subarray(translatedEnd + 1)
  if (compressed === 1) {
    const inflated = inflatePngText(text)
    if (inflated.reason !== null) return inflated
    text = inflated.buffer
  } else if (text.length > MAX_PNG_TEXT_BYTES) {
    return { reason: 'public-png-ancillary-size-limit' }
  }
  try {
    UTF8.decode(translated)
    UTF8.decode(text)
  } catch {
    return { reason: 'public-png-ancillary-invalid' }
  }
  return {
    reason: pngTextIsSensitive([keyword, language, translated, text]) ? 'public-png-ancillary-sensitive' : null,
    bytes: translated.length + text.length,
  }
}

function validatePng(buffer) {
  let offset = 8
  let chunks = 0
  let header = null
  let paletteEntries = null
  let sawData = false
  let dataClosed = false
  let sawEnd = false
  let textBytes = 0
  const singletonAncillary = new Set()
  const dataParts = []
  while (offset + 12 <= buffer.length) {
    if (++chunks > 10_000) return 'public-png-chunk-invalid'
    const length = buffer.readUInt32BE(offset)
    const typeStart = offset + 4
    const dataStart = typeStart + 4
    const dataEnd = dataStart + length
    const crcEnd = dataEnd + 4
    if (!Number.isSafeInteger(dataEnd) || dataEnd < dataStart || crcEnd > buffer.length) {
      return 'public-png-chunk-invalid'
    }
    const typeBytes = buffer.subarray(typeStart, dataStart)
    if (typeBytes.length !== 4 || !typeBytes.every((byte) =>
      (byte >= 0x41 && byte <= 0x5a) || (byte >= 0x61 && byte <= 0x7a)) ||
      typeBytes[2] < 0x41 || typeBytes[2] > 0x5a) return 'public-png-chunk-invalid'
    const type = typeBytes.toString('ascii')
    if (crc32(buffer, typeStart, dataEnd) !== buffer.readUInt32BE(dataEnd)) return 'public-png-crc-invalid'
    if (chunks === 1 && type !== 'IHDR') return 'public-png-chunk-order-invalid'
    if (type === 'IHDR') {
      if (header !== null || chunks !== 1) return 'public-png-chunk-order-invalid'
      const parsed = parsePngHeader(buffer, dataStart, length)
      if (parsed.reason) return parsed.reason
      header = parsed
    } else if (type === 'PLTE') {
      if (header === null || sawData || paletteEntries !== null) return 'public-png-chunk-order-invalid'
      if (length === 0 || length > 768 || length % 3 !== 0 || [0, 4].includes(header.colorType)) {
        return 'public-png-palette-invalid'
      }
      paletteEntries = length / 3
      if (header.colorType === 3 && paletteEntries > 2 ** header.bitDepth) return 'public-png-palette-invalid'
    } else if (type === 'IDAT') {
      if (header === null || dataClosed) return 'public-png-chunk-order-invalid'
      sawData = true
      dataParts.push(buffer.subarray(dataStart, dataEnd))
    } else if (type === 'IEND') {
      if (header === null || !sawData || sawEnd || length !== 0 || crcEnd !== buffer.length) {
        return 'public-png-chunk-order-invalid'
      }
      sawEnd = true
    } else if (['tEXt', 'zTXt', 'iTXt'].includes(type)) {
      if (header === null || sawEnd) return 'public-png-chunk-order-invalid'
      const parsed = parsePngTextChunk(type, buffer.subarray(dataStart, dataEnd))
      if (parsed.reason !== null) return parsed.reason
      textBytes += parsed.bytes
      if (textBytes > MAX_PNG_TEXT_TOTAL_BYTES) return 'public-png-ancillary-size-limit'
      if (sawData) dataClosed = true
    } else if (type === 'cHRM' || type === 'bKGD') {
      if (header === null || sawData || sawEnd || singletonAncillary.has(type)) {
        return 'public-png-chunk-order-invalid'
      }
      const expectedLength = type === 'cHRM'
        ? 32
        : header.colorType === 3 ? 1 : [0, 4].includes(header.colorType) ? 2 : 6
      if (length !== expectedLength || (type === 'bKGD' && header.colorType === 3 &&
          (paletteEntries === null || buffer[dataStart] >= paletteEntries))) {
        return 'public-png-ancillary-invalid'
      }
      singletonAncillary.add(type)
    } else {
      if (typeBytes[0] >= 0x41 && typeBytes[0] <= 0x5a) return 'public-png-chunk-invalid'
      if (header === null || sawEnd) return 'public-png-chunk-order-invalid'
      return 'public-png-ancillary-unsupported'
    }
    offset = crcEnd
    if (sawEnd) break
  }
  if (!sawEnd || offset !== buffer.length || header === null || !sawData) {
    return 'public-png-chunk-order-invalid'
  }
  if (header.colorType === 3 && paletteEntries === null) return 'public-png-palette-invalid'
  const compressed = Buffer.concat(dataParts)
  let decoded
  try {
    const inflated = inflateSync(compressed, {
      info: true,
      maxOutputLength: header.expectedBytes + 1,
    })
    if (!Number.isSafeInteger(inflated.engine?.bytesWritten) ||
        inflated.engine.bytesWritten !== compressed.length) return 'public-png-zlib-invalid'
    decoded = inflated.buffer
  } catch (error) {
    return error?.code === 'ERR_BUFFER_TOO_LARGE'
      ? 'public-png-decompression-limit'
      : 'public-png-zlib-invalid'
  }
  if (decoded.length > header.expectedBytes) return 'public-png-decompression-limit'
  if (decoded.length !== header.expectedBytes) return 'public-png-decoded-size-invalid'
  const stride = header.rowBytes + 1
  for (let row = 0; row < header.height; row++) {
    if (decoded[row * stride] > 4) return 'public-png-filter-invalid'
  }
  return null
}

const JPEG_FRAME_MARKERS = new Set([
  0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7,
  0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
])

function validJfifSegment(buffer, start, length) {
  if (length !== 14 || buffer.subarray(start, start + 5).toString('latin1') !== 'JFIF\0' ||
      buffer[start + 5] !== 1 || buffer[start + 6] > 2 || buffer[start + 7] > 2 ||
      buffer.readUInt16BE(start + 8) === 0 || buffer.readUInt16BE(start + 10) === 0) return false
  return buffer[start + 12] === 0 && buffer[start + 13] === 0
}

function parseJpegQuantizationTables(buffer, start, end, tables) {
  let offset = start
  while (offset < end) {
    const descriptor = buffer[offset++]
    const precision = descriptor >>> 4
    const id = descriptor & 0x0f
    if (precision !== 0 || id > 3 || tables.has(id) || offset + 64 > end) return false
    const values = buffer.subarray(offset, offset + 64)
    if (values.includes(0)) return false
    tables.add(id)
    offset += 64
  }
  return offset === end
}

function buildJpegHuffmanTable(counts, symbols, tableClass) {
  const codes = Array.from({ length: 17 }, () => new Map())
  const unique = new Set()
  let code = 0
  let symbolIndex = 0
  for (let length = 1; length <= 16; length++) {
    const count = counts[length - 1]
    if (code + count > 2 ** length) return null
    for (let index = 0; index < count; index++) {
      const symbol = symbols[symbolIndex++]
      if (unique.has(symbol) ||
          (tableClass === 0 && symbol > 11) ||
          (tableClass === 1 && (symbol & 0x0f) === 0 && symbol !== 0x00 && symbol !== 0xf0) ||
          (tableClass === 1 && (symbol & 0x0f) > 10)) return null
      unique.add(symbol)
      codes[length].set(code + index, symbol)
    }
    code = (code + count) * 2
  }
  return symbolIndex === symbols.length ? { codes } : null
}

function parseJpegHuffmanTables(buffer, start, end, tables) {
  let offset = start
  while (offset < end) {
    if (offset + 17 > end) return false
    const descriptor = buffer[offset++]
    const tableClass = descriptor >>> 4
    const id = descriptor & 0x0f
    if (tableClass > 1 || id > 3) return false
    const key = `${tableClass}:${id}`
    if (tables.has(key)) return false
    const counts = buffer.subarray(offset, offset + 16)
    offset += 16
    const symbolCount = counts.reduce((sum, count) => sum + count, 0)
    if (symbolCount === 0 || symbolCount > 256 || offset + symbolCount > end) return false
    const table = buildJpegHuffmanTable(counts, buffer.subarray(offset, offset + symbolCount), tableClass)
    if (table === null) return false
    tables.set(key, table)
    offset += symbolCount
  }
  return offset === end
}

function parseJpegFrame(buffer, start, length, quantizationTables) {
  if (length < 9 || buffer[start] !== 8) return { reason: 'public-jpeg-frame-invalid' }
  const height = buffer.readUInt16BE(start + 1)
  const width = buffer.readUInt16BE(start + 3)
  const count = buffer[start + 5]
  if (height === 0 || width === 0 || ![1, 3].includes(count) || length !== 6 + (3 * count) ||
      BigInt(width) * BigInt(height) > BigInt(MAX_JPEG_PIXELS)) {
    return { reason: 'public-jpeg-frame-invalid' }
  }
  const components = []
  const ids = new Set()
  let samplingBlocks = 0
  for (let index = 0; index < count; index++) {
    const offset = start + 6 + (index * 3)
    const id = buffer[offset]
    const horizontal = buffer[offset + 1] >>> 4
    const vertical = buffer[offset + 1] & 0x0f
    const quantization = buffer[offset + 2]
    if (id !== index + 1 || ids.has(id) || horizontal < 1 || horizontal > 4 ||
        vertical < 1 || vertical > 4 || !quantizationTables.has(quantization)) {
      return { reason: quantizationTables.has(quantization)
        ? 'public-jpeg-frame-invalid'
        : 'public-jpeg-table-invalid' }
    }
    ids.add(id)
    samplingBlocks += horizontal * vertical
    components.push({ id, horizontal, vertical, quantization })
  }
  if (samplingBlocks > 10 || (count === 1 &&
      (components[0].horizontal !== 1 || components[0].vertical !== 1))) {
    return { reason: 'public-jpeg-frame-invalid' }
  }
  const maxHorizontal = Math.max(...components.map((component) => component.horizontal))
  const maxVertical = Math.max(...components.map((component) => component.vertical))
  const mcuColumns = Math.ceil(width / (8 * maxHorizontal))
  const mcuRows = Math.ceil(height / (8 * maxVertical))
  const mcuCount = mcuColumns * mcuRows
  const blockCount = mcuCount * samplingBlocks
  if (!Number.isSafeInteger(blockCount) || blockCount > MAX_JPEG_BLOCKS) {
    return { reason: 'public-jpeg-frame-invalid' }
  }
  return { width, height, components, mcuCount, samplingBlocks }
}

function parseJpegScan(buffer, start, length, frame, huffmanTables) {
  if (frame === null || length < 6) return { reason: 'public-jpeg-scan-invalid' }
  const count = buffer[start]
  if (count !== frame.components.length || length !== 1 + (2 * count) + 3) {
    return { reason: 'public-jpeg-scan-invalid' }
  }
  const components = []
  for (let index = 0; index < count; index++) {
    const frameComponent = frame.components[index]
    const id = buffer[start + 1 + (2 * index)]
    const selectors = buffer[start + 2 + (2 * index)]
    const dc = selectors >>> 4
    const ac = selectors & 0x0f
    if (id !== frameComponent.id || dc > 3 || ac > 3 ||
        !huffmanTables.has(`0:${dc}`) || !huffmanTables.has(`1:${ac}`)) {
      return { reason: huffmanTables.has(`0:${dc}`) || huffmanTables.has(`1:${ac}`)
        ? 'public-jpeg-scan-invalid'
        : 'public-jpeg-table-invalid' }
    }
    components.push({ ...frameComponent, dc, ac })
  }
  const trailer = start + 1 + (2 * count)
  if (buffer[trailer] !== 0 || buffer[trailer + 1] !== 63 || buffer[trailer + 2] !== 0) {
    return { reason: 'public-jpeg-scan-invalid' }
  }
  return { components }
}

function splitJpegEntropy(buffer, start) {
  const segments = []
  const restarts = []
  let current = []
  let offset = start
  while (offset < buffer.length) {
    const byte = buffer[offset++]
    if (byte !== 0xff) {
      current.push(byte)
      continue
    }
    let fill = 0
    while (offset < buffer.length && buffer[offset] === 0xff) {
      fill++
      offset++
    }
    if (offset >= buffer.length) return null
    const marker = buffer[offset++]
    if (marker === 0x00) {
      if (fill !== 0) return null
      current.push(0xff)
    } else if (marker >= 0xd0 && marker <= 0xd7) {
      segments.push(Buffer.from(current))
      current = []
      restarts.push(marker)
    } else if (marker === 0xd9) {
      if (offset !== buffer.length) return null
      segments.push(Buffer.from(current))
      return { segments, restarts }
    } else {
      return null
    }
  }
  return null
}

function jpegBitReader(buffer) {
  let bit = 0
  return {
    readBit() {
      if (bit >= buffer.length * 8) return null
      const value = (buffer[Math.floor(bit / 8)] >>> (7 - (bit % 8))) & 1
      bit++
      return value
    },
    readBits(count) {
      for (let index = 0; index < count; index++) if (this.readBit() === null) return false
      return true
    },
    hasOnlyPaddingOnes() {
      for (let value = this.readBit(); value !== null; value = this.readBit()) if (value !== 1) return false
      return true
    },
  }
}

function decodeJpegHuffmanSymbol(reader, table) {
  let code = 0
  for (let length = 1; length <= 16; length++) {
    const bit = reader.readBit()
    if (bit === null) return null
    code = (code * 2) + bit
    if (table.codes[length].has(code)) return table.codes[length].get(code)
  }
  return null
}

function decodeJpegBlock(reader, dcTable, acTable) {
  const dcLength = decodeJpegHuffmanSymbol(reader, dcTable)
  if (dcLength === null || !reader.readBits(dcLength)) return false
  let coefficient = 1
  while (coefficient < 64) {
    const symbol = decodeJpegHuffmanSymbol(reader, acTable)
    if (symbol === null) return false
    if (symbol === 0x00) return true
    if (symbol === 0xf0) {
      coefficient += 16
      if (coefficient > 64) return false
      continue
    }
    const run = symbol >>> 4
    const size = symbol & 0x0f
    coefficient += run
    if (coefficient >= 64 || !reader.readBits(size)) return false
    coefficient++
  }
  return true
}

function validateJpegEntropy(buffer, start, frame, scan, huffmanTables, restartInterval) {
  const split = splitJpegEntropy(buffer, start)
  if (split === null) return false
  const expectedRestarts = restartInterval === null ? 0 : Math.floor((frame.mcuCount - 1) / restartInterval)
  if (split.restarts.length !== expectedRestarts || split.segments.length !== expectedRestarts + 1) return false
  for (let index = 0; index < split.restarts.length; index++) {
    if (split.restarts[index] !== 0xd0 + (index % 8)) return false
  }
  let remainingMcus = frame.mcuCount
  for (const segment of split.segments) {
    const mcus = restartInterval === null ? remainingMcus : Math.min(restartInterval, remainingMcus)
    if (mcus <= 0) return false
    const reader = jpegBitReader(segment)
    for (let mcu = 0; mcu < mcus; mcu++) {
      for (const component of scan.components) {
        const dcTable = huffmanTables.get(`0:${component.dc}`)
        const acTable = huffmanTables.get(`1:${component.ac}`)
        for (let block = 0; block < component.horizontal * component.vertical; block++) {
          if (!decodeJpegBlock(reader, dcTable, acTable)) return false
        }
      }
    }
    if (!reader.hasOnlyPaddingOnes()) return false
    remainingMcus -= mcus
  }
  return remainingMcus === 0
}

function validateJpeg(buffer) {
  let offset = 2
  let markerCount = 0
  let sawC2pa = false
  let frame = null
  let restartInterval = null
  const quantizationTables = new Set()
  const huffmanTables = new Map()
  while (offset < buffer.length) {
    if (buffer[offset++] !== 0xff) return 'public-jpeg-structure-invalid'
    while (offset < buffer.length && buffer[offset] === 0xff) offset++
    if (offset >= buffer.length) return 'public-jpeg-structure-invalid'
    const marker = buffer[offset++]
    if (marker === 0xd8 || marker === 0xd9 || marker === 0x00 || marker === 0x01 ||
        (marker >= 0xd0 && marker <= 0xd7)) return 'public-jpeg-structure-invalid'
    if (offset + 2 > buffer.length) return 'public-jpeg-structure-invalid'
    const segmentLength = buffer.readUInt16BE(offset)
    if (segmentLength < 2 || offset + segmentLength > buffer.length) return 'public-jpeg-structure-invalid'
    const dataStart = offset + 2
    const dataLength = segmentLength - 2
    const dataEnd = dataStart + dataLength
    if (markerCount++ === 0 && (marker !== 0xe0 || !validJfifSegment(buffer, dataStart, dataLength))) {
      return 'public-jpeg-jfif-invalid'
    }
    if (marker === 0xe0 && markerCount > 1 &&
        buffer.subarray(dataStart, Math.min(dataStart + 5, dataEnd)).toString('latin1') === 'JFIF\0') {
      return 'public-jpeg-jfif-invalid'
    }
    let acceptedMetadata = marker === 0xe0 && markerCount === 1
    if (!acceptedMetadata && ((marker >= 0xe0 && marker <= 0xef) || marker === 0xfe)) {
      const metadata = buffer.subarray(dataStart, dataEnd)
      if (marker === 0xeb && !sawC2pa) {
        const c2pa = validateC2paApp11(metadata)
        if (c2pa === 'sensitive') return 'public-jpeg-metadata-sensitive'
        if (c2pa === 'valid') {
          acceptedMetadata = true
          sawC2pa = true
        }
      }
      if (!acceptedMetadata) {
        if (binaryMetadataHasSensitiveValue(metadata)) return 'public-jpeg-metadata-sensitive'
        return 'public-jpeg-metadata-unsupported'
      }
    }
    if (marker === 0xdb) {
      if (frame !== null || !parseJpegQuantizationTables(buffer, dataStart, dataEnd, quantizationTables)) {
        return 'public-jpeg-table-invalid'
      }
    } else if (marker === 0xc4) {
      if (!parseJpegHuffmanTables(buffer, dataStart, dataEnd, huffmanTables)) {
        return 'public-jpeg-table-invalid'
      }
    } else if (marker === 0xc0) {
      if (frame !== null) return 'public-jpeg-frame-invalid'
      const parsed = parseJpegFrame(buffer, dataStart, dataLength, quantizationTables)
      if (parsed.reason) return parsed.reason
      frame = parsed
    } else if (JPEG_FRAME_MARKERS.has(marker)) {
      return 'public-jpeg-frame-invalid'
    } else if (marker === 0xdd) {
      if (restartInterval !== null || dataLength !== 2) return 'public-jpeg-structure-invalid'
      restartInterval = buffer.readUInt16BE(dataStart)
      if (restartInterval === 0) return 'public-jpeg-structure-invalid'
    } else if (marker === 0xda) {
      const scan = parseJpegScan(buffer, dataStart, dataLength, frame, huffmanTables)
      if (scan.reason) return scan.reason
      return validateJpegEntropy(buffer, dataEnd, frame, scan, huffmanTables, restartInterval)
        ? null
        : 'public-jpeg-entropy-invalid'
    } else if (!acceptedMetadata) {
      return 'public-jpeg-structure-invalid'
    }
    offset += segmentLength
  }
  return 'public-jpeg-structure-invalid'
}

function publicBinaryFinding(file, reason) {
  return { path: file, line: 1, rule: 'UNSAFE_PUBLIC_BINARY_ENCODING', reason }
}

function canonicalJson(value) {
  if (Array.isArray(value)) return value.map(canonicalJson)
  if (value && typeof value === 'object' && Object.getPrototypeOf(value) === Object.prototype) {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalJson(value[key])]))
  }
  return value
}

function sameCanonicalJson(left, right) {
  return JSON.stringify(canonicalJson(left)) === JSON.stringify(canonicalJson(right))
}

function exactObjectKeys(value, keys) {
  return value && typeof value === 'object' && Object.getPrototypeOf(value) === Object.prototype &&
    Object.keys(value).sort().join(',') === [...keys].sort().join(',')
}

function publicJpegPath(file) {
  return isPublicArtifact(file) && /^\.jpe?g$/i.test(path.posix.extname(file))
}

function publicJpegManifestFinding(reason) {
  return {
    path: PUBLIC_BINARY_DIGESTS_FILE,
    line: 1,
    rule: 'UNSAFE_PUBLIC_BINARY_ENCODING',
    reason,
  }
}

function strictJsonCandidate(root, mode, files, snapshot, file, missingReason, notRegularReason, invalidReason) {
  if (!files.includes(file)) return { value: null, reason: missingReason }
  let candidate
  try {
    candidate = readCandidate(root, file, mode, snapshot)
  } catch {
    return { value: null, reason: invalidReason }
  }
  if (candidate.kind !== 'file') return { value: null, reason: notRegularReason }
  if (candidate.buffer.length > MAX_FILE_BYTES) return { value: null, reason: invalidReason }
  let text
  try {
    text = UTF8.decode(candidate.buffer)
  } catch {
    return { value: null, reason: invalidReason }
  }
  if (scanUnsafeUnicodeContent(file, text).length > 0) return { value: null, reason: invalidReason }
  try {
    return { value: JSON.parse(text), reason: null }
  } catch {
    return { value: null, reason: invalidReason }
  }
}

function canonicalManifestPath(file) {
  return typeof file === 'string' && file.normalize('NFC') === file && path.posix.normalize(file) === file
    ? file.toLowerCase()
    : null
}

function validatePublicJpegDigestContract(root, mode, files, snapshot, authority = null) {
  const jpegFiles = files.filter(publicJpegPath).sort((left, right) => left.localeCompare(right))
  const hasContract = files.includes(PUBLIC_BINARY_DIGESTS_FILE) || files.includes(PUBLIC_BINARY_DIGESTS_SCHEMA_FILE)
  if (jpegFiles.length === 0 && !hasContract) return { entries: new Map(), findings: [] }
  const schema = strictJsonCandidate(
    root,
    mode,
    files,
    snapshot,
    PUBLIC_BINARY_DIGESTS_SCHEMA_FILE,
    'public-jpeg-schema-missing',
    'public-jpeg-schema-not-regular',
    'public-jpeg-schema-invalid',
  )
  if (schema.reason !== null || !sameCanonicalJson(schema.value, PUBLIC_BINARY_DIGEST_SCHEMA)) {
    return { entries: new Map(), findings: [publicJpegManifestFinding(schema.reason ?? 'public-jpeg-schema-invalid')] }
  }
  const manifest = strictJsonCandidate(
    root,
    mode,
    files,
    snapshot,
    PUBLIC_BINARY_DIGESTS_FILE,
    'public-jpeg-manifest-missing',
    'public-jpeg-manifest-not-regular',
    'public-jpeg-manifest-invalid',
  )
  if (manifest.reason !== null) {
    return { entries: new Map(), findings: [publicJpegManifestFinding(manifest.reason)] }
  }
  if (!exactObjectKeys(manifest.value, ['$schema', 'schema_version', 'entries']) ||
      manifest.value.$schema !== './public-binary-digests.schema.json' ||
      manifest.value.schema_version !== 'ramshared-public-binary-digests/v1' ||
      !Array.isArray(manifest.value.entries) ||
      manifest.value.entries.length > MAX_PUBLIC_JPEG_ENTRIES) {
    return { entries: new Map(), findings: [publicJpegManifestFinding('public-jpeg-manifest-invalid')] }
  }

  const entries = new Map()
  const canonicalPaths = new Set()
  const digests = new Set()
  let previousPath = null
  for (const entry of manifest.value.entries) {
    const canonicalPath = canonicalManifestPath(entry?.path)
    if (!exactObjectKeys(entry, ['path', 'size', 'sha256']) || canonicalPath === null ||
        !isSafeRepoPath(root, entry.path) || !/^docs\/.+\.jpe?g$/.test(entry.path) ||
        !Number.isSafeInteger(entry.size) || entry.size < 1 || entry.size > MAX_PUBLIC_BINARY_BYTES ||
        typeof entry.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(entry.sha256) ||
        canonicalPaths.has(canonicalPath) || digests.has(entry.sha256) ||
        (previousPath !== null && previousPath.localeCompare(entry.path) >= 0)) {
      return { entries: new Map(), findings: [publicJpegManifestFinding('public-jpeg-manifest-entry-invalid')] }
    }
    previousPath = entry.path
    canonicalPaths.add(canonicalPath)
    digests.add(entry.sha256)
    entries.set(entry.path, entry)
  }
  if (JSON.stringify([...entries.keys()]) !== JSON.stringify(jpegFiles)) {
    return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-path-set-mismatch')] }
  }
  const selectedAssets = new Map()
  for (const [file, entry] of entries) {
    const indexEntry = snapshot.indexEntries.get(file)
    if (!indexEntry) {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-asset-not-tracked')] }
    }
    if (indexEntry.kind !== 'file') {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-asset-not-regular')] }
    }
    let candidate
    try {
      candidate = readCandidate(root, file, mode, snapshot)
    } catch {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-asset-unreadable')] }
    }
    if (candidate.kind !== 'file') {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-asset-not-regular')] }
    }
    const digest = createHash('sha256').update(candidate.buffer).digest('hex')
    if (candidate.buffer.length !== entry.size || digest !== entry.sha256) {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-digest-mismatch')] }
    }
    selectedAssets.set(file, candidate.buffer)
  }
  if (authority !== null) {
    let authorityJpegs
    try {
      authorityJpegs = revisionFiles(root, authority).filter(publicJpegPath)
        .sort((left, right) => left.localeCompare(right))
    } catch {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-authority-query-failed')] }
    }
    if (JSON.stringify(authorityJpegs) !== JSON.stringify(jpegFiles)) {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-authority-path-set-mismatch')] }
    }
    for (const [file, entry] of entries) {
      try {
        if (revisionKind(root, authority, file) !== 'file') {
          return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-mismatch')] }
        }
        const baseline = revisionBuffer(root, authority, file)
        const digest = createHash('sha256').update(baseline).digest('hex')
        if (baseline.length !== entry.size || digest !== entry.sha256) {
          return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-mismatch')] }
        }
      } catch {
        return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-mismatch')] }
      }
    }
    for (const file of [PUBLIC_BINARY_DIGESTS_SCHEMA_FILE, PUBLIC_BINARY_DIGESTS_FILE]) {
      try {
        const kind = revisionKind(root, authority, file)
        if (kind === null) continue
        if (kind !== 'file') {
          return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-changed')] }
        }
        const current = readCandidate(root, file, mode, snapshot)
        if (current.kind !== 'file' || !current.buffer.equals(revisionBuffer(root, authority, file))) {
          return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-changed')] }
        }
      } catch {
        return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-authority-changed')] }
      }
    }
  }
  for (const [file, buffer] of selectedAssets) {
    const contract = publicBinaryContract(file)
    if (buffer.length > MAX_PUBLIC_BINARY_BYTES || contract === null ||
        !matchesPublicBinarySignature(buffer, contract) || validateJpeg(buffer) !== null) {
      return { entries, findings: [publicJpegManifestFinding('public-jpeg-manifest-asset-invalid')] }
    }
  }
  return { entries, findings: [] }
}

function isActivationInstruction(line) {
  if (!ACTIVATION_COMMAND.test(line)) return false
  // Markdown bullets and emphasis can contain current instructions; strip only
  // genuinely non-executable comment syntax. A same-line negation is not a local
  // boundary: it can neither guard this command nor later commands.
  if (/^\s*(?:#|\/\/|;)/.test(line)) return false
  const inlineCommand = /`\s*(?:sudo\s+)?(?:mkswap|swapon|swapoff|mkfs(?:\.[A-Za-z0-9_-]+)?|mount(?:-vhd)?|umount|initialize-disk|format-(?:volume|disk)|new-(?:vhd|partition)|clear-disk|dismount-vhd|start-vm|stop-vm|set-vm|restart-computer|modprobe|systemctl|wsl(?:\.exe)?\s+(?:-d|--)|wslconfig(?:-ctl\.sh)?\s+apply|ramshared\s+(?:up|down)|origin[-_ ]?route|pressure[-_ ]?(?:route|probe|campaign)|[^`]*--run-(?:isolated|shared-daily-host))\b[^`]*`/i.test(line)
  const firstCode = line.indexOf('`')
  const inlineActionIntent = firstCode !== -1 && /(?:\bnext action:\*{0,2}\s*|\b(?:(?:re-)?run|invoke|execute|apply|start|mount|format|restart)\s*)$/i.test(line.slice(0, firstCode))
  const inlineImperative = inlineActionIntent &&
    /`[^`]*(?:wsl(?:2)?(?:freeze|pressure)|cascade-pressure|sharedwsl|--run-|origin|pressure)[^`]*`/i.test(line)
  const prose = line.replace(/`[^`]*`/g, '')
  const proseSubject = prose.replace(/^\s*(?:[-*+]\s+)?(?:\*{1,2}[^*]+\*{1,2}:?\s*)?/, '')
  const proseImperative = /^(?:(?:re-)?run|invoke|execute|apply|start|mount|format|restart)\b.{0,100}\b(?:mkswap|swapon|swapoff|mkfs|mount|umount|initialize-disk|format-|new-(?:vhd|partition)|clear-disk|dismount-vhd|start-vm|stop-vm|set-vm|restart-computer|modprobe|systemctl|wsl|wslconfig|ramshared|origin|pressure)\b/i.test(proseSubject) ||
    /^do not execute\b.{0,100}\b(?:mkswap|swapon|swapoff|mkfs|mount|umount|initialize-disk|format-|new-(?:vhd|partition)|clear-disk|dismount-vhd|start-vm|stop-vm|set-vm|restart-computer|modprobe|systemctl|wsl|wslconfig|ramshared|origin|pressure)\b/i.test(proseSubject)
  const normalized = line.replace(/^\s*[-*+]\s+/, '').replace(/^\*{1,2}[^*]+\*{1,2}:?\s*/, '')
  const commandLike = /^(?:sudo\s+)?(?:mkswap|swapon|swapoff|mkfs|mount|umount|initialize-disk|format-|new-(?:vhd|partition)|clear-disk|dismount-vhd|install-|start-vm|stop-vm|set-vm|restart-computer|enable-windowsoptionalfeature|modprobe|systemctl|wsl|wslconfig|ramshared)(?=\s|$)/i.test(normalized)
  return (inlineCommand && (inlineActionIntent || /^\s*`[^`]+`\s*$/.test(line))) || inlineImperative || proseImperative || commandLike
}

function validScope(scope) {
  if (typeof scope !== 'string' || !scope || CONTROL_OR_AMBIGUOUS_PATH.test(scope) ||
      path.posix.isAbsolute(scope) || path.win32.isAbsolute(scope) || scope.startsWith('-') ||
      scope.startsWith(':')) return false
  const trimmed = scope.endsWith('/') ? scope.slice(0, -1) : scope
  return Boolean(trimmed) && hasSafeSegments(trimmed)
}

function validCalendarDate(value, asOf) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false
  const [year, month, day] = value.split('-').map(Number)
  const instant = new Date(Date.UTC(year, month - 1, day, 23, 59, 59, 999))
  return instant.getUTCFullYear() === year && instant.getUTCMonth() === month - 1 &&
    instant.getUTCDate() === day && instant.getTime() >= asOf.getTime()
}

function scopeContains(scope, file) {
  const normalizedScope = scope.endsWith('/') ? scope.slice(0, -1) : scope
  return file === normalizedScope || file.startsWith(`${normalizedScope}/`)
}

function validAllowlist(registry, asOf = new Date()) {
  if (!registry || registry.schema_version !== 1 || !Array.isArray(registry.entries)) throw new HygieneError('invalid-allowlist')
  for (const entry of registry.entries) {
    if (!entry || typeof entry !== 'object' || Object.getPrototypeOf(entry) !== Object.prototype ||
        Object.keys(entry).sort().join(',') !== 'expires,owner_role,pattern,reason,scope' ||
        typeof entry.pattern !== 'string' || !entry.pattern || !Array.isArray(entry.scope) ||
        entry.scope.length === 0 || !entry.scope.every(validScope) ||
        typeof entry.owner_role !== 'string' || !entry.owner_role.trim() ||
        typeof entry.reason !== 'string' || !entry.reason.trim() || !validCalendarDate(entry.expires, asOf)) {
      throw new HygieneError('invalid-allowlist-entry')
    }
  }
  return registry.entries
}

function emailAllowed(file, value, allowlist) {
  return allowlist.some((entry) => entry.pattern === value && entry.scope.some((scope) => scopeContains(scope, file)))
}

function scanRuleMatches(file, text, allowlist, enabledRules) {
  const findings = []
  for (const [rule, expression, reason] of enabledRules) {
    for (const match of allMatches(expression, text)) {
      if (rule === 'EMAIL' && emailAllowed(file, match[0], allowlist)) continue
      findings.push({ path: file, line: lineNumber(text, match.index), rule, reason })
    }
  }
  return findings
}

export function scanText(file, text, registry = { schema_version: 1, entries: [] }, asOf = new Date(), enabledRules = [...rules, ...publicDocumentRules]) {
  const allowlist = validAllowlist(registry, asOf)
  return [...scanUnsafeUnicodeContent(file, text), ...scanRuleMatches(file, text, allowlist, enabledRules)]
}

export function scanDocumentActivation(file, text) {
  if (!/\.md$/i.test(file)) return []
  const findings = []
  const lines = text.split('\n')
  for (let index = 0; index < lines.length; index++) {
    if (isActivationInstruction(lines[index]) && !hasPrecedingSafetyMarker(lines, index)) {
      findings.push({ path: file, line: index + 1, rule: 'UNGUARDED_ACTIVATION', reason: 'unguarded-current-activation-instruction' })
    }
  }
  return findings
}

function headBase(root, headOid) {
  const parts = git(root, ['rev-list', '--parents', '-n', '1', headOid], 'utf8').trim().split(/\s+/)
  return parts[1] ?? EMPTY_TREE
}

function changedPublicArtifacts(root, mode, snapshot) {
  if (mode === 'tracked') return null
  if (mode === 'staged') {
    const files = new Set()
    const allLines = new Set()
    const objectDiffs = new Map()
    const paths = new Set([...snapshot.headEntries.keys(), ...snapshot.indexEntries.keys()])
    if (paths.size > MAX_FILES) throw new HygieneError('file-count-limit')
    for (const file of paths) {
      const before = snapshot.headEntries.get(file) ?? null
      const after = snapshot.indexEntries.get(file) ?? null
      if (before?.mode === after?.mode && before?.oid === after?.oid) continue
      files.add(file)
      if (before === null) allLines.add(file)
      objectDiffs.set(file, { before: before?.oid ?? null, after: after?.oid ?? null })
    }
    return {
      authority: snapshot.headOid,
      files,
      allLines,
      objectDiffs,
      diffPrefix: null,
    }
  }

  const untracked = new Set(splitZero(git(root, ['ls-files', '--others', '--exclude-standard', '-z'])))
  const workingChanges = new Set(splitZero(git(root, [
    'diff', '--no-renames', '--name-only', '-z', snapshot.headOid, '--',
  ])))
  if (untracked.size === 0 && workingChanges.size === 0) {
    const base = headBase(root, snapshot.headOid)
    return {
      authority: base,
      files: new Set(splitZero(git(root, [
        'diff', '--no-renames', '--name-only', '-z', base, snapshot.headOid, '--',
      ]))),
      allLines: new Set(),
      diffPrefix: [
        'diff', '--no-renames', '--unified=0', '--no-color', '--no-ext-diff', base, snapshot.headOid,
      ],
    }
  }

  const files = workingChanges
  for (const file of untracked) files.add(file)
  return {
    authority: snapshot.headOid,
    files,
    allLines: untracked,
    diffPrefix: [
      'diff', '--no-renames', '--unified=0', '--no-color', '--no-ext-diff', snapshot.headOid,
    ],
  }
}

function candidateLineFilter(root, changes, file) {
  if (changes === null || changes.allLines.has(file)) return null
  const objects = changes.objectDiffs?.get(file)
  if (objects?.before === null || objects?.after === null) return null
  const diffArgs = objects
    ? [
        'diff', '--no-renames', '--unified=0', '--no-color', '--no-ext-diff',
        objects.before, objects.after,
      ]
    : [...changes.diffPrefix, '--', file]
  const diff = git(root, diffArgs).toString('utf8')
  const lines = new Set()
  for (const match of diff.matchAll(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@/gm)) {
    const start = Number(match[1])
    const count = match[2] === undefined ? 1 : Number(match[2])
    for (let line = start; line < start + count; line++) lines.add(line)
  }
  return lines
}

function filterCandidateLines(findings, lines) {
  return lines === null ? findings : findings.filter((finding) => lines.has(finding.line))
}

function symlinkTargetEscapes(file, text) {
  if (!text || path.posix.isAbsolute(text) || path.win32.isAbsolute(text)) return true
  const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(file), text))
  return resolved === '..' || resolved.startsWith('../') || path.posix.isAbsolute(resolved)
}

function loadAllowlist(root, mode, files, snapshot) {
  if (!files.includes(ALLOWLIST_FILE)) return { schema_version: 1, entries: [] }
  try {
    return JSON.parse(UTF8.decode(readCandidate(root, ALLOWLIST_FILE, mode, snapshot).buffer))
  } catch (error) {
    if (error instanceof HygieneError) throw error
    throw new HygieneError('allowlist-read-failed')
  }
}

function hashLine(text, line) {
  const lines = text.split(/\r?\n/)
  if (!Number.isSafeInteger(line) || line < 1 || line > lines.length) return null
  return createHash('sha256').update(`${lines[line - 1]}\n`).digest('hex')
}

function redactionFinding(line, reason) {
  return { path: REDACTION_LEDGER_FILE, line, rule: 'REDACTION_AUDIT', reason }
}

function validateRedactionLedger(root, mode, files, snapshot) {
  if (!files.includes(REDACTION_LEDGER_FILE)) return []
  let ledgerText
  try { ledgerText = UTF8.decode(readCandidate(root, REDACTION_LEDGER_FILE, mode, snapshot).buffer) } catch { return [redactionFinding(1, 'ledger-read-failed')] }
  const lines = ledgerText.split(/\r?\n/).filter((line) => line.trim())
  if (lines.length > 1000) return [redactionFinding(1, 'ledger-entry-limit')]
  const findings = []
  const ids = new Set()
  for (const [index, line] of lines.entries()) {
    let entry
    try { entry = JSON.parse(line) } catch { findings.push(redactionFinding(index + 1, 'ledger-json')); continue }
    if (!entry || typeof entry !== 'object' || Array.isArray(entry) ||
        Object.keys(entry).sort().join(',') !== [...REDACTION_KEYS].sort().join(',') ||
        entry.schema_version !== 'ramshared-public-redaction/v1' ||
        !/^PHR-\d{4,}$/.test(entry.redaction_id ?? '') || ids.has(entry.redaction_id) ||
        !Number.isFinite(Date.parse(entry.applied_at ?? '')) || !LOWER_REVISION.test(entry.source_revision ?? '') ||
        !isSafeRepoPath(root, entry.path) || !Number.isSafeInteger(entry.source_line) || entry.source_line < 1 ||
        !Number.isSafeInteger(entry.replacement_line) || entry.replacement_line < 1 ||
        !PUBLIC_IDENTITY_RULES.has(entry.rule) || !/^SANITIZED_[A-Z0-9_]+$/.test(entry.replacement_class ?? '') ||
        !LOWER_SHA256.test(entry.supersedes_line_sha256 ?? '') || !LOWER_SHA256.test(entry.replacement_line_sha256 ?? '') ||
        typeof entry.reason !== 'string' || entry.reason.length < 20) {
      findings.push(redactionFinding(index + 1, 'ledger-entry-shape'))
      continue
    }
    ids.add(entry.redaction_id)
    let sourceText
    let replacementText
    try {
      sourceText = UTF8.decode(git(root, ['show', `${entry.source_revision}:${entry.path}`]))
    } catch {
      try {
        git(root, ['fetch', 'origin', entry.source_revision])
        sourceText = UTF8.decode(git(root, ['show', `${entry.source_revision}:${entry.path}`]))
      } catch {
        findings.push(redactionFinding(index + 1, 'superseded-source-unavailable'))
        continue
      }
    }
    try { replacementText = UTF8.decode(readCandidate(root, entry.path, mode, snapshot).buffer) } catch { findings.push(redactionFinding(index + 1, 'replacement-source-unavailable')); continue }
    if (hashLine(sourceText, entry.source_line) !== entry.supersedes_line_sha256) findings.push(redactionFinding(index + 1, 'superseded-line-digest-mismatch'))
    if (hashLine(replacementText, entry.replacement_line) !== entry.replacement_line_sha256 ||
        !replacementText.split(/\r?\n/)[entry.replacement_line - 1]?.includes(entry.replacement_class)) {
      findings.push(redactionFinding(index + 1, 'replacement-line-digest-mismatch'))
    }
  }
  return findings
}

function pathFinding(error) {
  const reason = error instanceof HygieneError ? error.message : 'candidate-path-unreadable'
  return {
    path: '<invalid-path>', line: 1,
    rule: 'UNSAFE_PATH',
    reason,
  }
}

function scanSnapshot(root, mode, asOf, snapshot, files, changedArtifacts) {
  const findings = []
  const unsafeChangedPaths = new Set()
  if (changedArtifacts !== null) {
    for (const file of changedArtifacts.files) {
      if (isSafeRepoPath(root, file)) continue
      unsafeChangedPaths.add(file)
      findings.push({ path: '<invalid-path>', line: 1, rule: 'UNSAFE_PATH', reason: 'unsafe-git-path' })
    }
  }
  let allowlist
  try {
    allowlist = loadAllowlist(root, mode, files, snapshot)
    allowlist = validAllowlist(allowlist, asOf)
  } catch (error) {
    findings.push(pathFinding(error))
    allowlist = []
  }
  const jpegDigestContract = validatePublicJpegDigestContract(
    root,
    mode,
    files,
    snapshot,
    changedArtifacts?.authority ?? null,
  )
  findings.push(...jpegDigestContract.findings)
  for (const file of files) {
    if (!isSafeRepoPath(root, file)) {
      if (!unsafeChangedPaths.has(file)) {
        findings.push({ path: '<invalid-path>', line: 1, rule: 'UNSAFE_PATH', reason: 'unsafe-git-path' })
      }
      continue
    }
    let candidate
    try {
      candidate = readCandidate(root, file, mode, snapshot)
    } catch (error) {
      findings.push(pathFinding(error))
      continue
    }
    const { buffer, kind } = candidate
    const changedPublicArtifact = isPublicArtifact(file) &&
      (changedArtifacts === null || changedArtifacts.files.has(file))
    if (changedPublicArtifact) {
      // Filename checks apply to text and binary artifacts alike.
      findings.push(...scanRuleMatches(file, file, allowlist, rules))
      findings.push(...scanRuleMatches(file, file, allowlist, publicDocumentRules))
      const binaryContract = kind === 'file' ? publicBinaryContract(file) : null
      if (binaryContract) {
        if (buffer.length > MAX_PUBLIC_BINARY_BYTES) {
          findings.push(publicBinaryFinding(file, 'public-binary-size-limit'))
        } else if (!matchesPublicBinarySignature(buffer, binaryContract)) {
          findings.push(publicBinaryFinding(file, 'public-binary-signature-mismatch'))
        } else {
          const invalidReason = binaryContract.validate(buffer)
          if (invalidReason !== null) {
            findings.push(publicBinaryFinding(file, invalidReason))
          } else if (publicJpegPath(file)) {
            const digestEntry = jpegDigestContract.entries.get(file)
            const digest = createHash('sha256').update(buffer).digest('hex')
            if (!digestEntry) {
              findings.push(publicBinaryFinding(file, 'public-jpeg-digest-unlisted'))
            } else if (digestEntry.size !== buffer.length || digestEntry.sha256 !== digest) {
              findings.push(publicBinaryFinding(file, 'public-jpeg-digest-mismatch'))
            }
          }
        }
        continue
      }
      if (buffer.length > MAX_FILE_BYTES) throw new HygieneError('file-size-limit')
      let text
      try {
        text = UTF8.decode(buffer)
      } catch {
        findings.push({
          path: file,
          line: 1,
          rule: 'UNSAFE_PUBLIC_TEXT_ENCODING',
          reason: 'public-text-invalid-utf8',
        })
        continue
      }
      if (kind === 'symlink' && symlinkTargetEscapes(file, text)) {
        findings.push({
          path: file,
          line: 1,
          rule: 'PUBLIC_SYMLINK_ESCAPE',
          reason: 'public-symlink-target-outside-repository',
        })
      }
      const candidateLines = candidateLineFilter(root, changedArtifacts, file)
      // Content controls are checked before identity/activation matching, even
      // when an unsafe Cc would otherwise resemble a binary delimiter.
      findings.push(...scanUnsafeUnicodeContent(file, text))
      if (file === ALLOWLIST_FILE) continue
      findings.push(...filterCandidateLines(scanRuleMatches(file, text, allowlist, rules), candidateLines))
      findings.push(...filterCandidateLines(scanRuleMatches(file, text, allowlist, publicDocumentRules), candidateLines))
      findings.push(...filterCandidateLines(scanDocumentActivation(file, text), candidateLines))
      continue
    }
    if (!classifyText(buffer, file)) continue
    if (buffer.length > MAX_FILE_BYTES) throw new HygieneError('file-size-limit')
    const text = UTF8.decode(buffer)
    if (file === ALLOWLIST_FILE) continue
    findings.push(...scanRuleMatches(file, file, allowlist, rules), ...scanRuleMatches(file, text, allowlist, rules))
  }
  findings.push(...validateRedactionLedger(root, mode, files, snapshot))
  findings.sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line || a.rule.localeCompare(b.rule) || a.reason.localeCompare(b.reason))
  return { ok: findings.length === 0, files: files.length, findings }
}

export function run({ root, mode = 'candidate', asOf = new Date(), afterGitSnapshot = null }) {
  const snapshot = resolveGitSnapshot(root)
  const files = enumerateFiles(root, mode, snapshot)
  const changedArtifacts = changedPublicArtifacts(root, mode, snapshot)
  try {
    if (afterGitSnapshot !== null) {
      if (typeof afterGitSnapshot !== 'function') throw new HygieneError('git-snapshot-hook-invalid')
      afterGitSnapshot()
    }
    return scanSnapshot(root, mode, asOf, snapshot, files, changedArtifacts)
  } finally {
    assertIndexSnapshotUnchanged(root, snapshot)
  }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  if (argv.length > 1 || (argv.length === 1 && !/^--(?:candidate|tracked|staged|check)$/.test(argv[0]))) {
    console.error('usage: check-public-hygiene.mjs [--candidate|--tracked|--staged|--check]')
    return 2
  }
  const mode = argv.length === 0 || argv[0] === '--check' ? 'candidate' : argv[0].slice(2)
  try {
    const result = run({ root: process.cwd(), mode })
    console.log(`MODE=${mode}`)
    console.log(`FILES=${result.files}`)
    for (const item of result.findings) console.error(`${item.path}:${item.line} — ${item.rule}: ${item.reason}`)
    console.log(`PUBLIC_HYGIENE_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
    return result.ok ? 0 : 1
  } catch (error) {
    const reason = error instanceof HygieneError ? error.message : 'unexpected-error'
    console.error(`PUBLIC_HYGIENE_ERROR=${reason}`)
    return 2
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
/* node:coverage enable */
