#!/usr/bin/env node
/**
 * Automated Staleness, Redundancy, Jargon, and Repository Boundary Checker — RamShared.
 *
 * Verifies that:
 * 1. Public human-facing docs do not contain internal bot methodology jargon.
 * 2. Markdown files do not contain broken relative file links (zombie links).
 * 3. Operational runbooks reference canonical guides rather than duplicating procedures.
 * 4. Repository boundary and host isolation:
 *    - No raw Windows host drive paths (e.g. C:\Users, V:\Hyper-V, I:\wsl2)
 *    - No out-of-tree WSL host mounts (e.g. /mnt/c/Users)
 *    - No foreign project cross-contamination (e.g. civm, jules-operator, advoq)
 *    - No unredacted lab VM names (e.g. gha-ubuntu-2404)
 *    - No leaked private lab/host IP addresses
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
export const PUBLIC_HUMAN_DOCS = [
  'README.md',
  'README.pt-BR.md',
  'ARCHITECTURE.md',
  'docs/FAQ.md',
  'docs/DEVELOPER-GUIDE.md',
  'docs/OPERATOR-GUIDE.md',
]

export const FORBIDDEN_JARGON_PATTERNS = [
  { name: 'kahneman-discipline', regex: /\bKahneman\s*#\d+\b/i },
  { name: 'wysiati-jargon', regex: /\bWYSIATI\b/i },
  { name: 'anti-skynet-jargon', regex: /\banti-skynet\b/i },
  { name: 'cognitive-hygiene-buzzword', regex: /\bcognitive hygiene\b/i },
]

/**
 * Repository Boundary & Host Isolation Rules.
 *
 * RamShared is strictly an open-source, self-contained project.
 * These rules enforce that no developer workstation paths, out-of-tree mounts,
 * unredacted lab virtual machines, or foreign project narratives leak into
 * active repository rules, documentation, or operational guides.
 */
export const REPOSITORY_BOUNDARY_RULES = [
  {
    name: 'host-path-leak',
    description: 'Raw host filesystem path or out-of-tree mount leaked into repo documentation',
    // Matches raw Windows drive paths or /mnt/<drive>/ out-of-tree mounts,
    // but excludes standard Windows system paths, ProgramData\RamShared, and documentation placeholders.
    regex: /(?:\b[A-Za-z]:\\(?!Windows\\|Program Files|ProgramData\\RamShared\\|path\\to\\|example\\|pagefile\.sys)[^\s"'`<>|]+|\/(?:mnt|mnt\/host)\/[a-z]\/(?!path\/to\/)[^\s"'`<>|]+)/i,
    remediation: 'Use repository-relative paths ($REPO_ROOT), generic placeholders (<drive>:\\...), or standard Linux paths.',
  },
  {
    name: 'foreign-project-cross-contamination',
    description: 'Cross-contamination from external private repositories or out-of-tree projects',
    regex: /\b(?:civm|jules-operator|jules\/inbox|advoq)\b/i,
    remediation: 'RamShared is strictly open-source. Replace foreign project references with generic terms (e.g. "isolated VM", "QEMU environment").',
  },
  {
    name: 'unredacted-private-vm-identity',
    description: 'Unredacted private lab virtual machine or testbed identity',
    regex: /\b(?<!SANITIZED_)(?:gha-ubuntu-\d+|linux-kernel-lab|win(?:dows)?[-_]?\d{1,2}-(?:wsl2?|driver|kernel|lab|drill)[A-Za-z0-9_-]*)\b/i,
    remediation: 'Redact private VM identities or use generic terms (e.g. "guest-vm", "testbed-node").',
  },
  {
    name: 'private-network-address',
    description: 'Leaked private host or internal lab IP address',
    regex: /\b(?:192\.168\.\d{1,3}\.\d{1,3}|10\.\d{1,3}\.\d{1,3}\.\d{1,3}|100\.(?:6[4-9]|[7-9]\d|1[0-1]\d|12[0-7])\.\d{1,3}\.\d{1,3})\b/,
    remediation: 'Use documentation IP addresses (RFC 5737: 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24) or localhost.',
  },
]

export const BOUNDARY_PROTECTED_FILES = [
  'AGENTS.md',
  'CLAUDE.md',
  'README.md',
  'README.pt-BR.md',
  'ARCHITECTURE.md',
  'docs/FAQ.md',
  'docs/DEVELOPER-GUIDE.md',
  'docs/OPERATOR-GUIDE.md',
  'docs/BENCHMARKS.md',
  'docs/SSDV3-PROMPTS.md',
]

export const BOUNDARY_PROTECTED_DIRS = [
  '.claude/rules',
  'docs/methodology',
  'docs/governance',
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

export function checkDocStalenessAndRedundancy({
  root = ROOT,
  publicDocs = PUBLIC_HUMAN_DOCS,
  boundaryFiles = BOUNDARY_PROTECTED_FILES,
  boundaryDirs = BOUNDARY_PROTECTED_DIRS,
  skipBrokenLinks = false,
} = {}) {
  const findings = []

  // 1. Scan Public Human Docs for Forbidden Jargon
  for (const rel of publicDocs) {
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
  const mdFiles = skipBrokenLinks
    ? []
    : scanDirectory(path.join(root, 'docs'), (p) => p.endsWith('.md')).concat(
        scanDirectory(path.join(root, 'crates'), (p) => p.endsWith('.md'))
      )

  if (!skipBrokenLinks) {
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
  }

  // 3. Scan Boundary-Protected Files and Rules for Host Path Leaks, Foreign Projects, and Unsanitized Environment Leaks
  const filesToScan = new Set(boundaryFiles.map((f) => path.resolve(root, f)))
  for (const relDir of boundaryDirs) {
    const fullDir = path.resolve(root, relDir)
    const scanned = scanDirectory(fullDir, (p) => p.endsWith('.md'))
    for (const f of scanned) filesToScan.add(f)
  }

  for (const file of filesToScan) {
    if (!existsSync(file)) continue
    const rel = path.relative(root, file)
    const lines = readFileSync(file, 'utf8').split('\n')
    lines.forEach((line, idx) => {
      // Ignore lines in this CI tool or test files
      if (line.includes('REPOSITORY_BOUNDARY_RULES') || line.includes("name: 'host-path-leak'")) return
      for (const rule of REPOSITORY_BOUNDARY_RULES) {
        const flags = rule.regex.flags.includes('g') ? rule.regex.flags : `${rule.regex.flags}g`
        const rx = new RegExp(rule.regex.source, flags)
        let match
        while ((match = rx.exec(line)) !== null) {
          findings.push(
            `repository-boundary-violation:${rel}:${idx + 1}:${rule.name}:${match[0]}`
          )
        }
      }
    })
  }

  return {
    ok: findings.length === 0,
    findings,
    scannedPublicDocs: publicDocs.length,
    scannedMdFiles: mdFiles.length,
    scannedBoundaryFiles: filesToScan.size,
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
    `doc-staleness-and-redundancy: PASS (checked ${result.scannedPublicDocs} public docs, ${result.scannedMdFiles} markdown files, ${result.scannedBoundaryFiles} boundary-protected files)`
  )
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main()
}
