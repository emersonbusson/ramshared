#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { TextDecoder } from 'node:util'
import { fileURLToPath } from 'node:url'

const MAX_FILES = 20_000
const MAX_FILE_BYTES = 512 * 1024
const MODES = new Set(['candidate', 'tracked', 'staged'])
const UTF8 = new TextDecoder('utf-8', { fatal: true })

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
  return buffer.toString('utf8').split('\0').filter(Boolean)
}

export function enumerateFiles(root, mode = 'candidate') {
  if (!MODES.has(mode)) throw new HygieneError('invalid-mode')
  const args = mode === 'candidate'
    ? ['ls-files', '-co', '--exclude-standard', '-z']
    : ['ls-files', mode === 'staged' ? '--cached' : '-z', ...(mode === 'staged' ? ['-z'] : [])]
  const files = splitZero(git(root, args))
    .map((file) => file.replaceAll('\\', '/'))
    .sort((a, b) => a.localeCompare(b))
  if (files.length > MAX_FILES) throw new HygieneError('file-count-limit')
  return files
}

function readCandidate(root, file, mode) {
  try {
    const buffer = mode === 'staged'
      ? git(root, ['show', `:${file}`])
      : readFileSync(path.join(root, file))
    return buffer
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
    return !/[\u0001-\u0008\u000b\u000c\u000e-\u001f]/.test(text)
  } catch {
    return false
  }
}

function lineNumber(text, offset) {
  let line = 1
  for (let index = 0; index < offset; index++) if (text.charCodeAt(index) === 10) line++
  return line
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

function validAllowlist(registry, asOf = new Date()) {
  if (!registry || registry.schema_version !== 1 || !Array.isArray(registry.entries)) {
    throw new HygieneError('invalid-allowlist')
  }
  for (const entry of registry.entries) {
    const keys = Object.keys(entry).sort().join(',')
    if (keys !== 'expires,owner_role,pattern,reason,scope') throw new HygieneError('invalid-allowlist-entry')
    if (!entry.pattern || !Array.isArray(entry.scope) || entry.scope.length === 0 ||
        !entry.owner_role || !entry.reason || !/^\d{4}-\d{2}-\d{2}$/.test(entry.expires) ||
        Date.parse(`${entry.expires}T23:59:59Z`) < asOf.getTime()) {
      throw new HygieneError('invalid-allowlist-entry')
    }
  }
  return registry.entries
}

function emailAllowed(file, value, allowlist) {
  return allowlist.some((entry) => entry.pattern === value && entry.scope.some((scope) => file.startsWith(scope)))
}

export function scanText(file, text, registry = { schema_version: 1, entries: [] }, asOf = new Date()) {
  const findings = []
  const allowlist = validAllowlist(registry, asOf)
  for (const [rule, expression, reason] of rules) {
    expression.lastIndex = 0
    const match = expression.exec(text)
    if (!match) continue
    if (rule === 'EMAIL' && emailAllowed(file, match[0], allowlist)) continue
    findings.push({ path: file, line: lineNumber(text, match.index), rule, reason })
  }
  return findings
}

function loadAllowlist(root) {
  const file = path.join(root, 'docs/governance/public-hygiene-allowlist.json')
  try {
    return JSON.parse(readFileSync(file, 'utf8'))
  } catch (error) {
    if (error?.code === 'ENOENT') return { schema_version: 1, entries: [] }
    throw new HygieneError('allowlist-read-failed')
  }
}

export function run({ root, mode = 'candidate', asOf = new Date() }) {
  const files = enumerateFiles(root, mode)
  const allowlist = loadAllowlist(root)
  const findings = []
  for (const file of files) {
    if (/\r|\n|\0/.test(file)) {
      findings.push({ path: '<invalid-path>', line: 1, rule: 'UNSAFE_PATH', reason: 'control-character-in-path' })
      continue
    }
    const buffer = readCandidate(root, file, mode)
    if (!classifyText(buffer, file)) continue
    if (buffer.length > MAX_FILE_BYTES) throw new HygieneError('file-size-limit')
    const text = UTF8.decode(buffer)
    if (file !== 'docs/governance/public-hygiene-allowlist.json') {
      findings.push(...scanText(file, file, allowlist, asOf), ...scanText(file, text, allowlist, asOf))
    } else {
      validAllowlist(allowlist, asOf)
    }
  }
  findings.sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line || a.rule.localeCompare(b.rule))
  return { ok: findings.length === 0, files: files.length, findings }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  if (argv.length > 1 || (argv.length === 1 && !/^--(?:candidate|tracked|staged)$/.test(argv[0]))) {
    console.error('usage: check-public-hygiene.mjs [--candidate|--tracked|--staged]')
    return 2
  }
  const mode = argv.length === 0 ? 'candidate' : argv[0].slice(2)
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
