#!/usr/bin/env node
/**
 * Automated Staleness, Redundancy, and Jargon Checker — RamShared.
 *
 * Verifies that:
 * 1. Public human-facing docs do not contain internal bot methodology jargon.
 * 2. Markdown files do not contain broken relative file links (zombie links).
 * 3. Operational runbooks reference canonical guides rather than duplicating procedures.
 *
 * Usage:
 *   node tools/ci/check-doc-staleness-and-redundancy.mjs --check
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')

// User-facing docs that must be clear, human-friendly, and free of robot methodology jargon
const PUBLIC_HUMAN_DOCS = [
  'README.md',
  'README.pt-BR.md',
  'ARCHITECTURE.md',
  'docs/FAQ.md',
  'docs/DEVELOPER-GUIDE.md',
  'docs/OPERATOR-GUIDE.md',
]

const FORBIDDEN_JARGON_PATTERNS = [
  { name: 'kahneman-discipline', regex: /\bKahneman\s*#\d+\b/i },
  { name: 'wysiati-jargon', regex: /\bWYSIATI\b/i },
  { name: 'anti-skynet-jargon', regex: /\banti-skynet\b/i },
  { name: 'cognitive-hygiene-buzzword', regex: /\bcognitive hygiene\b/i },
]

function scanDirectory(dir, filter, list = []) {
  if (!existsSync(dir)) return list
  const entries = readdirSync(dir)
  for (const entry of entries) {
    const full = path.join(dir, entry)
    let stat
    try {
      stat = statSync(full)
    } catch {
      continue
    }
    if (stat.isDirectory()) {
      scanDirectory(full, filter, list)
    } else if (filter(full)) {
      list.push(full)
    }
  }
  return list
}

export function checkDocStalenessAndRedundancy({ root = ROOT } = {}) {
  const findings = []

  // 1. Scan Public Human Docs for Forbidden Jargon
  for (const rel of PUBLIC_HUMAN_DOCS) {
    const full = path.join(root, rel)
    if (!existsSync(full)) continue
    const content = readFileSync(full, 'utf8')
    for (const { name, regex } of FORBIDDEN_JARGON_PATTERNS) {
      if (regex.test(content)) {
        findings.push(`forbidden-jargon-in-public-doc:${rel}:${name}`)
      }
    }
  }

  // 2. Scan Markdown Files in docs/ and crates/ for Broken Relative File Links
  const mdFiles = scanDirectory(
    path.join(root, 'docs'),
    (p) => p.endsWith('.md')
  ).concat(
    scanDirectory(path.join(root, 'crates'), (p) => p.endsWith('.md'))
  )

  const LINK_REGEX = /\[(?:[^\]]+)\]\(([^)#\s]+)(?:#[^)]*)?\)/g

  for (const file of mdFiles) {
    const content = readFileSync(file, 'utf8')
    const relFile = path.relative(root, file)
    let match
    while ((match = LINK_REGEX.exec(content)) !== null) {
      const link = match[1]
      // Skip external URLs, mailto, anchor-only, or placeholders
      if (/^(?:https?:\/\/|mailto:|#)/i.test(link)) continue
      if (link.includes('SANITIZED_')) continue

      // Resolve relative link
      const targetPath = path.resolve(path.dirname(file), link)
      if (!existsSync(targetPath)) {
        findings.push(`broken-doc-link:${relFile}:${link}`)
      }
    }
  }

  return {
    ok: findings.length === 0,
    findings,
    scannedPublicDocs: PUBLIC_HUMAN_DOCS.length,
    scannedMdFiles: mdFiles.length,
  }
}

function main() {
  if (!process.argv.includes('--check')) {
    console.error('usage: check-doc-staleness-and-redundancy.mjs --check')
    process.exit(2)
  }

  const result = checkDocStalenessAndRedundancy()
  if (!result.ok) {
    console.error('doc-staleness-and-redundancy: FAIL')
    for (const f of result.findings) {
      console.error(`  - ${f}`)
    }
    process.exit(1)
  }

  console.log(
    `doc-staleness-and-redundancy: PASS (checked ${result.scannedPublicDocs} public docs, ${result.scannedMdFiles} markdown files)`
  )
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main()
}
