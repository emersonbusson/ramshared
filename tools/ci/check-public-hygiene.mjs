#!/usr/bin/env node
/**
 * Public hygiene gate for tracked repository content.
 *
 * This intentionally scans `git ls-files` instead of the working tree so local
 * VM notes, ignored artifacts, build output, and credential files stay outside
 * the public gate.
 */
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  '..'
)

const textExtensions = new Set([
  '.c',
  '.cc',
  '.cmd',
  '.cs',
  '.h',
  '.hpp',
  '.json',
  '.md',
  '.mjs',
  '.ps1',
  '.rs',
  '.sh',
  '.toml',
  '.txt',
  '.xml',
  '.yml',
  '.yaml',
])

const bannedAppSpecific = [
  {
    id: 'example-app-name-blender',
    re: /\bblender\b/i,
    reason:
      'example application names must not define generic product files, docs, or behavior',
  },
  {
    id: 'example-app-name-battlefield',
    re: /\bbattlefield\b/i,
    reason:
      'example application names must not define generic product files, docs, or behavior',
  },
  {
    id: 'example-app-name-after-effects',
    re: /\bafter[ -]?effects\b/i,
    reason:
      'example application names must not define generic product files, docs, or behavior',
  },
  {
    id: 'old-render-vram-script-name',
    re: /\b(?:measure-)?render-vram\b/i,
    reason: 'use app-agnostic gpu-workload/vram-reclaim naming',
  },
]

const bannedSecrets = [
  {
    id: 'known-test-signing-password',
    re: /TestSign!2026/,
    reason: 'historical signing password literal must not be tracked',
  },
  {
    id: 'inline-pfx-password',
    re: /-PfxPassword\s+["'][^"']+["']/i,
    reason: 'signing passwords must come from environment/local secret stores',
  },
  {
    id: 'inline-drill-password-env',
    re: /RAMSHARED_DRILL_PASSWORD\s*=\s*['"](?!…|<|REDACTED)[^'"]+['"]/i,
    reason: 'lab VM passwords must not be committed',
  },
  {
    id: 'private-key-material',
    re: /-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----/,
    reason: 'private key material must not be tracked',
  },
]

function trackedFiles() {
  const raw = execFileSync('git', ['ls-files', '-z'], {
    cwd: ROOT,
    encoding: 'buffer',
    maxBuffer: 64 * 1024 * 1024,
  })
  return raw
    .toString('utf8')
    .split('\0')
    .filter(Boolean)
    .sort()
}

function isTextFile(file) {
  return textExtensions.has(path.extname(file).toLowerCase())
}

function lineNumber(text, offset) {
  let line = 1
  for (let i = 0; i < offset; i++) {
    if (text.charCodeAt(i) === 10) line++
  }
  return line
}

function scanPatternList(file, text, patterns, findings) {
  for (const p of patterns) {
    p.re.lastIndex = 0
    const match = p.re.exec(text)
    if (!match) continue
    findings.push({
      file,
      line: lineNumber(text, match.index),
      id: p.id,
      reason: p.reason,
    })
  }
}

function main() {
  const findings = []
  for (const file of trackedFiles()) {
    const normalized = file.replaceAll('\\', '/')
    scanPatternList(normalized, normalized, bannedAppSpecific, findings)
    scanPatternList(normalized, normalized, bannedSecrets, findings)

    if (!isTextFile(file)) continue
    if (normalized === 'tools/ci/check-public-hygiene.mjs') continue
    const text = readFileSync(path.join(ROOT, file), 'utf8')
    scanPatternList(normalized, text, bannedAppSpecific, findings)
    scanPatternList(normalized, text, bannedSecrets, findings)
  }

  if (findings.length > 0) {
    for (const f of findings) {
      console.error(`${f.file}:${f.line} — ${f.id}: ${f.reason}`)
    }
    process.exit(1)
  }
  console.log('✓ public hygiene OK')
}

main()
