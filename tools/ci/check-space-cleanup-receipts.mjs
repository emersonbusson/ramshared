#!/usr/bin/env node
/**
 * Read-only integrity gate for historical workstation-space receipts.
 *
 * This module deliberately validates documentation only. It never invokes a
 * host command and must not become a cleanup runner.
 */
import { existsSync, lstatSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const RECEIPTS_PATH = 'docs/governance/space-cleanup-receipts.jsonl'
const MAX_FILE_BYTES = 1024 * 1024
const MAX_RECORDS = 1000
const ISO_TIMESTAMP_RE = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?(?:Z|[+-]\d{2}:\d{2})$/
const RECEIPT_ID_RE = /^space-cleanup-\d{8}-historical-\d{3}$/
const OWNER_RE = /^[a-z][a-z0-9-]{2,63}$/
const REVISION_RE = /^[0-9a-f]{40}$/i
const SAFE_TARGET_CLASSES = new Set([
  'rebuildable-build-cache',
  'rebuildable-container-cache',
  'rebuildable-tool-cache',
  'temporary-test-output',
  'toolchain',
  'repository-source',
  'persistent-data',
])
const REMOVABLE_TARGET_CLASSES = new Set([
  'rebuildable-build-cache',
  'rebuildable-container-cache',
  'rebuildable-tool-cache',
  'temporary-test-output',
])
const DISPOSITIONS = new Set(['removed', 'retained', 'not-observed'])

function isObject(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function exactKeys(value, keys) {
  return isObject(value) && Object.keys(value).sort().join(',') === [...keys].sort().join(',')
}

function isExactTimestamp(value) {
  return typeof value === 'string' && ISO_TIMESTAMP_RE.test(value) && !Number.isNaN(Date.parse(value))
}

function isSafeDocumentPath(value) {
  if (typeof value !== 'string' || !(value === 'validation.md' || value.startsWith('docs/')) || !value.endsWith('.md')) return false
  if (value.includes('\\') || value.includes('\0')) return false
  const parts = value.split('/')
  return !parts.some((part) => !part || part === '.' || part === '..')
}

function containsSensitiveContent(value) {
  const text = JSON.stringify(value)
  return [
    /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/i,
    /(?:api[-_ ]?key|token|password|credential)\s*(?:=|:|\s)\s*["']?[^\s"',}]+/i,
    /(?:^|["'\s])\/home\/[A-Za-z0-9._-]+\//,
    /\b[A-Za-z]:\\Users\\[^\\\s"']+/i,
    /\\\\wsl(?:\.localhost|\$)\\[^\\]+\\(?:home\\)?[^\\]+/i,
    /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/i,
  ].some((rule) => rule.test(text))
}

function sourceExists(root, document) {
  if (!isSafeDocumentPath(document)) return false
  const rootPath = path.resolve(root)
  const fullPath = path.resolve(rootPath, document)
  if (!fullPath.startsWith(`${rootPath}${path.sep}`) || !existsSync(fullPath)) return false
  const stat = lstatSync(fullPath)
  return stat.isFile() && !stat.isSymbolicLink()
}

function validateBytes(container, prefix, findings) {
  const byteStatus = container.byte_status
  const before = Object.hasOwn(container, 'before_bytes') ? container.before_bytes : container.free_bytes
  const after = Object.hasOwn(container, 'after_bytes') ? container.after_bytes : container.total_bytes
  if (byteStatus === 'not-recorded') {
    if (before !== null || after !== null) findings.push(`${prefix}-bytes`)
    return
  }
  if (byteStatus !== 'measured' || !Number.isSafeInteger(before) || !Number.isSafeInteger(after) || before < 0 || after < 0) {
    findings.push(`${prefix}-bytes`)
  }
}

export function validateReceipt(receipt, { root = ROOT } = {}) {
  const findings = []
  if (!isObject(receipt)) return ['receipt-type']
  const required = [
    'schema_version', 'receipt_id', 'recorded_at', 'owner', 'classification',
    'promotion', 'source', 'i_volume_gate', 'targets', 'postcondition',
    'retention', 'safe_paths',
  ]
  if (!exactKeys(receipt, required)) findings.push('receipt-keys')
  if (receipt.schema_version !== 'ramshared-space-cleanup-receipt/v1') findings.push('schema-version')
  if (typeof receipt.receipt_id !== 'string' || !RECEIPT_ID_RE.test(receipt.receipt_id)) findings.push('receipt-id')
  if (!isExactTimestamp(receipt.recorded_at)) findings.push('recorded-at')
  if (typeof receipt.owner !== 'string' || !OWNER_RE.test(receipt.owner)) findings.push('owner')
  if (receipt.classification !== 'historical') findings.push('classification')

  if (!exactKeys(receipt.promotion, ['qualification', 'eligible']) ||
      receipt.promotion.qualification !== 'unqualified' || receipt.promotion.eligible !== false) {
    findings.push('historical-promotion')
  }

  const source = receipt.source
  if (!exactKeys(source, ['recorded_at', 'document', 'locator', 'revision', 'revision_status'])) {
    findings.push('source')
    if (!isExactTimestamp(source?.recorded_at)) findings.push('source-recorded-at')
    if (!['recorded', 'not-recorded'].includes(source?.revision_status) || (source?.revision_status === 'recorded' && (typeof source?.revision !== 'string' || !REVISION_RE.test(source.revision))) || (source?.revision_status === 'not-recorded' && source?.revision !== null)) findings.push('source-revision')
  } else {
    if (!isExactTimestamp(source.recorded_at)) findings.push('source-recorded-at')
    if (!['recorded', 'not-recorded'].includes(source.revision_status) || (source.revision_status === 'recorded' && !REVISION_RE.test(source.revision)) || (source.revision_status === 'not-recorded' && source.revision !== null)) findings.push('source-revision')
    if (!isSafeDocumentPath(source.document)) findings.push('source-document')
    else if (!sourceExists(root, source.document)) findings.push('source-document-missing')
    if (typeof source.locator !== 'string' || !/^#[a-z0-9][a-z0-9-]{1,127}$/i.test(source.locator)) findings.push('source-locator')
  }

  const gate = receipt.i_volume_gate
  if (!exactKeys(gate, ['volume', 'observed_at', 'free_bytes', 'total_bytes', 'byte_status', 'gate'])) {
    findings.push('i-volume-gate')
  } else {
    if (gate.volume !== 'I:' || !isExactTimestamp(gate.observed_at) || gate.gate !== 'not-qualified') findings.push('i-volume-gate')
    validateBytes(gate, 'i-volume-gate', findings)
    if (gate.byte_status === 'measured' && gate.free_bytes > gate.total_bytes) findings.push('i-volume-gate-bytes')
  }

  if (!Array.isArray(receipt.targets) || receipt.targets.length < 1 || receipt.targets.length > 32) {
    findings.push('targets')
  } else {
    const classes = new Set()
    for (const target of receipt.targets) {
      if (!exactKeys(target, ['class', 'disposition', 'before_bytes', 'after_bytes', 'byte_status'])) {
        findings.push('target-keys')
        continue
      }
      if (!SAFE_TARGET_CLASSES.has(target.class)) findings.push('unsafe-target-class')
      if (classes.has(target.class)) findings.push('duplicate-target-class')
      classes.add(target.class)
      if (!DISPOSITIONS.has(target.disposition)) findings.push('target-disposition')
      if (!REMOVABLE_TARGET_CLASSES.has(target.class) && target.disposition === 'removed') findings.push('unsafe-target-disposition')
      validateBytes(target, 'target', findings)
      if (target.byte_status === 'measured' && target.disposition === 'removed' && target.after_bytes > target.before_bytes) findings.push('target-bytes')
    }
  }

  const postcondition = receipt.postcondition
  if (!exactKeys(postcondition, ['status', 'statement']) || postcondition.status !== 'not-qualified' ||
      typeof postcondition.statement !== 'string' || postcondition.statement.trim().length < 10) {
    findings.push('postcondition')
  }
  if (!exactKeys(receipt.retention, ['class', 'disposition']) ||
      receipt.retention.class !== 'append-only' || receipt.retention.disposition !== 'retain') {
    findings.push('retention')
  }

  if (!Array.isArray(receipt.safe_paths) || receipt.safe_paths.length < 1 || receipt.safe_paths.length > 16) {
    findings.push('safe-paths')
  } else {
    const paths = new Set()
    for (const safePath of receipt.safe_paths) {
      if (!isSafeDocumentPath(safePath) || !sourceExists(root, safePath)) findings.push('safe-path')
      if (paths.has(safePath)) findings.push('duplicate-safe-path')
      paths.add(safePath)
    }
    if (isObject(source) && typeof source.document === 'string' && !paths.has(source.document)) findings.push('source-safe-path')
  }

  if (containsSensitiveContent(receipt)) findings.push('sensitive-content')
  return [...new Set(findings)]
}

function readBounded(file) {
  const stat = statSync(file)
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES) throw new Error('receipt-file-size')
  return readFileSync(file, 'utf8')
}

export function validateRepository({ root = ROOT } = {}) {
  const findings = []
  const file = path.join(root, RECEIPTS_PATH)
  if (!existsSync(file)) return { ok: false, receipts: 0, findings: ['missing-receipts-file'] }
  let lines
  try {
    lines = readBounded(file).split(/\r?\n/).filter((line) => line.trim())
  } catch (error) {
    return { ok: false, receipts: 0, findings: [error.message] }
  }
  if (lines.length > MAX_RECORDS) return { ok: false, receipts: lines.length, findings: ['receipt-count-limit'] }
  const ids = new Set()
  for (const [index, line] of lines.entries()) {
    let receipt
    try {
      receipt = JSON.parse(line)
    } catch {
      findings.push(`jsonl-parse:${index + 1}`)
      continue
    }
    const receiptFindings = validateReceipt(receipt, { root })
    for (const finding of receiptFindings) findings.push(`receipt:${index + 1}:${finding}`)
    if (typeof receipt?.receipt_id === 'string') {
      if (ids.has(receipt.receipt_id)) findings.push(`duplicate-receipt-id:${receipt.receipt_id}`)
      ids.add(receipt.receipt_id)
    }
  }
  return { ok: findings.length === 0, receipts: lines.length, findings }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  if (argv.length > 1 || (argv.length === 1 && argv[0] !== '--check')) {
    console.error('usage: check-space-cleanup-receipts.mjs [--check]')
    return 2
  }
  const result = validateRepository({ root: process.cwd() })
  console.log(`SPACE_CLEANUP_RECEIPTS=${result.receipts}`)
  for (const finding of result.findings) console.error(`space-cleanup-receipts — ${finding}`)
  console.log(`SPACE_CLEANUP_RECEIPTS_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
/* node:coverage enable */
