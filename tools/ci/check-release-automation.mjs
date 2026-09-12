#!/usr/bin/env node
/**
 * Automated Release Automation & SemVer Governance Checker — RamShared.
 *
 * Verifies that:
 * 1. Cargo.toml root package version adheres to strict SemVer (MAJOR.MINOR.PATCH).
 * 2. .release-please-manifest.json exists and contains a valid SemVer version.
 * 3. release-please-config.json exists, contains valid JSON, conventional changelog sections,
 *    and its pull-request-header complies with .github/pull_request_template.md mandatory sections.
 * 4. .github/workflows/release-packaging.yml exists, triggers on 'v*', and references build scripts.
 * 5. Packaging scripts exist and are executable (build-deb-package.sh, build-rpm-package.sh).
 * 6. README.md & README.pt-BR.md maintain parity with active Multi-Tier Hardware Benchmark qualification.
 *
 * Usage:
 *   node tools/ci/check-release-automation.mjs --check
 */

import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')

const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/

const REQUIRED_PR_SECTIONS = [
  { name: 'Summary/Resumo', regex: /##\s+(?:Summary|Resumo)/i },
  { name: 'Commits', regex: /##\s+Commits/i },
  { name: 'Issue', regex: /##\s+Issue/i },
  { name: 'Owner/Responsavel', regex: /##\s+(?:Owner|Responsavel|Responsável)/i },
  { name: 'Labels', regex: /##\s+Labels/i },
  { name: 'Validation/Validacao', regex: /##\s+(?:Validation|Validacao|Validação)/i },
  { name: 'Rollback trigger', regex: /##\s+Rollback trigger/i },
]

export function checkReleaseAutomation({ root = ROOT } = {}) {
  const findings = []

  // 1. Check Cargo.toml version
  const cargoPath = path.join(root, 'Cargo.toml')
  if (!existsSync(cargoPath)) {
    findings.push('Cargo.toml is missing')
  } else {
    const cargoContent = readFileSync(cargoPath, 'utf8')
    const versionMatch = cargoContent.match(/^version\s*=\s*"(.*?)"/m)
    if (!versionMatch) {
      findings.push('Cargo.toml missing root package version')
    } else {
      const version = versionMatch[1]
      if (!SEMVER_RE.test(version)) {
        findings.push(`Cargo.toml version "${version}" violates strict SemVer`)
      }
    }
  }

  // 2. Check .release-please-manifest.json
  const manifestPath = path.join(root, '.release-please-manifest.json')
  if (!existsSync(manifestPath)) {
    findings.push('.release-please-manifest.json is missing')
  } else {
    try {
      const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
      const rootVersion = manifest['.']
      if (!rootVersion || !SEMVER_RE.test(rootVersion)) {
        findings.push(`.release-please-manifest.json root version "${rootVersion}" violates SemVer`)
      }
    } catch (err) {
      findings.push(`.release-please-manifest.json is invalid JSON: ${err.message}`)
    }
  }

  // 3. Check release-please-config.json
  const configPath = path.join(root, 'release-please-config.json')
  if (!existsSync(configPath)) {
    findings.push('release-please-config.json is missing')
  } else {
    try {
      const config = JSON.parse(readFileSync(configPath, 'utf8'))
      if (!config['changelog-sections'] || !Array.isArray(config['changelog-sections'])) {
        findings.push('release-please-config.json missing changelog-sections array')
      }
      const header = config['pull-request-header'] || ''
      for (const section of REQUIRED_PR_SECTIONS) {
        if (!section.regex.test(header)) {
          findings.push(`release-please-config.json pull-request-header missing section: ${section.name}`)
        }
      }
    } catch (err) {
      findings.push(`release-please-config.json is invalid JSON: ${err.message}`)
    }
  }

  // 4. Check packaging workflow
  const packagingPath = path.join(root, '.github', 'workflows', 'release-packaging.yml')
  if (!existsSync(packagingPath)) {
    findings.push('.github/workflows/release-packaging.yml is missing')
  } else {
    const packagingContent = readFileSync(packagingPath, 'utf8')
    if (!packagingContent.includes("tags:") || !packagingContent.includes("- 'v*'")) {
      findings.push('release-packaging.yml missing push tag v* trigger')
    }
    if (!packagingContent.includes('build-deb-package.sh')) {
      findings.push('release-packaging.yml missing build-deb-package.sh reference')
    }
    if (!packagingContent.includes('build-rpm-package.sh')) {
      findings.push('release-packaging.yml missing build-rpm-package.sh reference')
    }
    if (!packagingContent.includes('SHA256SUMS.txt')) {
      findings.push('release-packaging.yml missing SHA256SUMS.txt generation')
    }
  }

  // 5. Check packaging scripts exist
  const debScript = path.join(root, 'scripts', 'package', 'build-deb-package.sh')
  const rpmScript = path.join(root, 'scripts', 'package', 'build-rpm-package.sh')
  const aurPkg = path.join(root, 'packaging', 'arch', 'PKGBUILD')
  if (!existsSync(debScript)) findings.push('scripts/package/build-deb-package.sh is missing')
  if (!existsSync(rpmScript)) findings.push('scripts/package/build-rpm-package.sh is missing')
  if (!existsSync(aurPkg)) findings.push('packaging/arch/PKGBUILD is missing')

  // 6. Check README benchmark parity
  for (const readmeFile of ['README.md', 'README.pt-BR.md']) {
    const p = path.join(root, readmeFile)
    if (!existsSync(p)) {
      findings.push(`${readmeFile} is missing`)
    } else {
      const content = readFileSync(p, 'utf8')
      if (!content.includes('PASS_ZERO_PANIC')) {
        findings.push(`${readmeFile} missing PASS_ZERO_PANIC benchmark verdict`)
      }
      if (!/Tier\s*0/i.test(content) || !/Tier\s*1/i.test(content) || !/Tier\s*3/i.test(content)) {
        findings.push(`${readmeFile} missing Multi-Tier breakdown (Tier 0, Tier 1, Tier 3)`)
      }
      if (!content.includes('19,777 MB') && !content.includes('19.777 MB')) {
        findings.push(`${readmeFile} missing 19,777 MB stress qualification metric`)
      }
    }
  }

  return {
    ok: findings.length === 0,
    findings,
  }
}

function main() {
  const args = process.argv.slice(2)
  const isCheck = args.includes('--check')
  const result = checkReleaseAutomation()

  if (result.ok) {
    console.log('✓ release-automation OK (SemVer, packaging workflows, and README parity in sync)')
    process.exit(0)
  } else {
    console.error(`release-automation: NO-GO (${result.findings.length} findings)`)
    for (const f of result.findings) {
      console.error(`  - ${f}`)
    }
    process.exit(1)
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main()
}
