#!/usr/bin/env node
/**
 * Automated Doc-Code Drift Checker — RamShared.
 *
 * Verifies that all workspace crates, drivers, and major subsystems have
 * synchronized, up-to-date documentation matching the codebase topology.
 *
 * Usage:
 *   node tools/ci/check-doc-code-drift.mjs --check
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const REQUIRED_SECTIONS = [
  'Scope & Responsibility',
  'Workspace Dependencies',
  'Safety Invariants',
  'Testing',
]

export function checkDocCodeDrift({ root = ROOT } = {}) {
  const findings = []
  const cratesDir = path.join(root, 'crates')
  const driversBlockDir = path.join(root, 'drivers', 'block', 'ramshared')
  const driversWinDir = path.join(root, 'drivers', 'windows')
  const archPath = path.join(root, 'ARCHITECTURE.md')

  if (!existsSync(archPath)) {
    findings.push('ARCHITECTURE.md is missing')
    return { ok: false, findings }
  }

  const archContent = readFileSync(archPath, 'utf8')

  // 1. Verify all workspace crates have a compliant README.md
  if (existsSync(cratesDir)) {
    const entries = readdirSync(cratesDir).sort()
    for (const entry of entries) {
      const cratePath = path.join(cratesDir, entry)
      if (!statSync(cratePath).isDirectory()) continue
      const readmePath = path.join(cratePath, 'README.md')
      if (!existsSync(readmePath)) {
        findings.push(`missing-crate-readme:crates/${entry}/README.md`)
        continue
      }

      // Verify ARCHITECTURE.md references the crate
      if (!archContent.includes(entry)) {
        findings.push(`unreferenced-crate-in-architecture:crates/${entry}`)
      }

      // Verify required documentation sections
      const readmeContent = readFileSync(readmePath, 'utf8')
      for (const section of REQUIRED_SECTIONS) {
        if (!readmeContent.includes(`## ${section}`)) {
          findings.push(`missing-readme-section:crates/${entry}/README.md:${section}`)
        }
      }
    }
  }

  // 2. Verify driver directories have documentation
  if (existsSync(driversBlockDir)) {
    const blockReadme = path.join(driversBlockDir, 'README.md')
    if (!existsSync(blockReadme)) {
      findings.push('missing-driver-readme:drivers/block/ramshared/README.md')
    }
  }

  if (existsSync(driversWinDir)) {
    const winReadme = path.join(driversWinDir, 'README.md')
    if (!existsSync(winReadme)) {
      findings.push('missing-driver-readme:drivers/windows/README.md')
    }
  }

  return {
    ok: findings.length === 0,
    findings,
    checkedCrates: existsSync(cratesDir) ? readdirSync(cratesDir).length : 0,
  }
}

function main() {
  if (!process.argv.includes('--check')) {
    console.error('usage: check-doc-code-drift.mjs --check')
    process.exit(2)
  }

  const result = checkDocCodeDrift()
  if (!result.ok) {
    console.error('doc-code-drift: FAIL')
    for (const f of result.findings) {
      console.error(`  - ${f}`)
    }
    process.exit(1)
  }

  console.log(`doc-code-drift: PASS (checked ${result.checkedCrates} crates)`)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main()
}
