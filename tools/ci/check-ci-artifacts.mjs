#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const ARTIFACT_POLICY = new Map([
  ['pr-diagnostic', { attachment: 'workflow', retention_days: 7 }],
  ['coverage-static', { attachment: 'workflow', retention_days: 14 }],
  ['lab-plan', { attachment: 'workflow', retention_days: 14 }],
  ['release-evidence', { attachment: 'release', retention_days: null }],
])
const TERMINAL_STATUSES = new Set(['PASS', 'FAIL', 'BLOCKED'])
const MAX_ARTIFACTS = 64
const MAX_SANITIZED_TEXT_BYTES = 64 * 1024

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function textArtifact(value) {
  return /\.(?:json|jsonl|log|md|txt)$/i.test(value)
}

export function validateRetention(record) {
  if (!isObject(record)) return false
  const expected = ARTIFACT_POLICY.get(record.class)
  return expected !== undefined && record.attachment === expected.attachment && record.retention_days === expected.retention_days
}

export function validateSanitizedText(value) {
  if (typeof value !== 'string') return { ok: false, errors: ['sanitizer-text-invalid'] }
  if (Buffer.byteLength(value) > MAX_SANITIZED_TEXT_BYTES) return { ok: false, errors: ['sanitizer-text-too-large'] }
  const forbidden = [
    /-----BEGIN [^-]*PRIVATE KEY-----/i,
    /(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})/,
    /\b(?:password|token|secret)\s*[:=]\s*\S+/i,
    /[A-Za-z]:\\Users\\/i,
    /\/home\/[^/\s]+/,
    /0x[fF]{4,}[0-9a-fA-F]{8,}/,
  ]
  return forbidden.some((pattern) => pattern.test(value))
    ? { ok: false, errors: ['sanitizer-sensitive-content'] }
    : { ok: true, errors: [] }
}

function add(errors, rule) {
  if (!errors.includes(rule)) errors.push(rule)
}

function validateArtifact(record, root, errors) {
  if (!isObject(record)) {
    add(errors, 'artifact-record-invalid')
    return
  }
  if (!safeRelative(record.path)) add(errors, 'artifact-path-unsafe')
  if (!ARTIFACT_POLICY.has(record.class)) add(errors, 'artifact-class-invalid')
  if (!validateRetention(record)) add(errors, 'artifact-retention-invalid')
  if (!Number.isInteger(record.bytes) || record.bytes < 0) add(errors, 'artifact-bytes-invalid')
  if (typeof record.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(record.sha256)) add(errors, 'artifact-sha256-invalid')
  if (record.sanitized !== true) add(errors, 'artifact-sanitization-invalid')
  if (record.summary !== undefined) {
    const sanitized = validateSanitizedText(record.summary)
    for (const rule of sanitized.errors) add(errors, rule)
  }
  if (!root || !safeRelative(record.path)) return

  const artifactPath = path.resolve(root, record.path)
  const rootPath = path.resolve(root)
  if (!artifactPath.startsWith(`${rootPath}${path.sep}`)) {
    add(errors, 'artifact-path-unsafe')
    return
  }
  if (!existsSync(artifactPath)) {
    add(errors, 'artifact-file-missing')
    return
  }
  if (lstatSync(artifactPath).isSymbolicLink() || !statSync(artifactPath).isFile()) {
    add(errors, 'artifact-file-invalid')
    return
  }
  const bytes = readFileSync(artifactPath)
  if (bytes.length !== record.bytes) add(errors, 'artifact-size-mismatch')
  if (sha256(bytes) !== record.sha256) add(errors, 'artifact-hash-mismatch')
  if (textArtifact(record.path)) {
    const sanitized = validateSanitizedText(bytes.toString('utf8'))
    for (const rule of sanitized.errors) add(errors, rule)
  }
}

export function validateArtifactManifest(manifest, { root } = {}) {
  const errors = []
  if (!isObject(manifest) || manifest.schema_version !== 1 ||
      typeof manifest.source_sha !== 'string' || !/^[0-9a-f]{40}$/.test(manifest.source_sha) ||
      !TERMINAL_STATUSES.has(manifest.terminal_status) || !Array.isArray(manifest.artifacts) ||
      manifest.artifacts.length === 0 || manifest.artifacts.length > MAX_ARTIFACTS) {
    add(errors, 'manifest-schema-invalid')
    return { ok: false, errors }
  }
  for (const artifact of manifest.artifacts) validateArtifact(artifact, root, errors)
  return { ok: errors.length === 0, errors: errors.sort() }
}

function parseArguments(argv) {
  if (argv[0] !== '--check' || typeof argv[1] !== 'string') return null
  if (argv.length === 2) return { manifestPath: argv[1], root: path.dirname(argv[1]) }
  if (argv.length === 4 && argv[2] === '--root' && typeof argv[3] === 'string') return { manifestPath: argv[1], root: argv[3] }
  return null
}

export function main(argv = process.argv.slice(2), { print = console.log, error = console.error } = {}) {
  const args = parseArguments(argv)
  if (!args) {
    error('usage: check-ci-artifacts.mjs --check <manifest> [--root <directory>]')
    return 2
  }
  let manifest
  try {
    manifest = JSON.parse(readFileSync(args.manifestPath, 'utf8'))
  } catch {
    error('CI_ARTIFACT_ERROR=manifest-read-failed')
    return 1
  }
  const result = validateArtifactManifest(manifest, { root: args.root })
  if (!result.ok) {
    for (const rule of result.errors) error(`CI_ARTIFACT_ERROR=${rule}`)
    print('CI_ARTIFACT_STATUS=FAIL')
    return 1
  }
  print('CI_ARTIFACT_STATUS=PASS')
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === new URL(import.meta.url).pathname) process.exitCode = main()
