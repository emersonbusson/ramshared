import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { checkReleaseAutomation } from './check-release-automation.mjs'

function createValidFixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-auto-'))
  mkdirSync(path.join(root, '.github', 'workflows'), { recursive: true })
  mkdirSync(path.join(root, 'scripts', 'package'), { recursive: true })
  mkdirSync(path.join(root, 'packaging', 'arch'), { recursive: true })
  mkdirSync(path.join(root, '.claude', 'rules'), { recursive: true })
  mkdirSync(path.join(root, 'docs'), { recursive: true })

  writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "ramshared"\nversion = "0.11.0"\n')
  writeFileSync(path.join(root, '.release-please-manifest.json'), JSON.stringify({ ".": "0.11.0" }))
  writeFileSync(path.join(root, 'release-please-config.json'), JSON.stringify({
    "changelog-sections": [{ "type": "feat", "section": "Features" }],
    "pull-request-header": "## Summary\n\n## Commits\n\n## Issue\n\n## Owner\n\n## Labels\n\n## Validation\n\n## Rollback trigger\n"
  }))
  writeFileSync(path.join(root, '.github', 'workflows', 'release-packaging.yml'), 'on:\n  push:\n    tags:\n      - \'v*\'\njobs:\n  build:\n    run: ./scripts/package/build-deb-package.sh && ./scripts/package/build-rpm-package.sh && sha256sum > SHA256SUMS.txt\n')
  writeFileSync(path.join(root, 'scripts', 'package', 'build-deb-package.sh'), '#!/bin/bash\n')
  writeFileSync(path.join(root, 'scripts', 'package', 'build-rpm-package.sh'), '#!/bin/bash\n')
  writeFileSync(path.join(root, 'packaging', 'arch', 'PKGBUILD'), 'pkgname=ramshared\n')

  writeFileSync(path.join(root, 'CHANGELOG.md'), '# Changelog\n\n## [0.11.0] - 2026-01-01\n')
  writeFileSync(path.join(root, '.claude', 'rules', 'governance.md'), 'Production posture is strictly stable (`v0.11.0`).\n')
  writeFileSync(path.join(root, 'ROADMAP.md'), 'Current release: **v0.11.0**.\n\n## Next (v0.12.0)\n')

  const supportText = [
    'Standard WSL2 uses NBD as the baseline transport.',
    'ublk/io_uring is qualified on native Linux or WSL2 with a compatible custom kernel.',
    'EVD-0039 records ublk/io_uring qualification.',
    'EVD-0040 records zero-copy CUDA host mapping.',
  ].join('\n')
  const readmeContent = `Release v0.11.0\n## Multi-Tier Hardware Benchmark Comparison\nTier 0 ZRAM\nTier 1 GPU VRAM\nTier 3 SSD\n19,777 MB\nPASS_ZERO_PANIC\n${supportText}\n`
  writeFileSync(path.join(root, 'README.md'), readmeContent)
  writeFileSync(path.join(root, 'README.pt-BR.md'), readmeContent)
  writeFileSync(path.join(root, 'ARCHITECTURE.md'), supportText)
  writeFileSync(path.join(root, 'docs', 'FAQ.md'), supportText)
  writeFileSync(
    path.join(root, 'validation.md'),
    '## ublk/io_uring qualification\nEvidence ID: `EVD-0039`.\nublk/io_uring on native Linux and compatible WSL2 custom kernel.\n\n## zero-copy CUDA host mapping\nEvidence ID: `EVD-0040`.\ncuMemHostRegister and PinnedHostMapping.\n',
  )

  return root
}

test('release_automation_passes_on_valid_repository', () => {
  const root = createValidFixture()
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, true, result.findings.join('\n'))
  assert.equal(result.findings.length, 0)
})

test('release_automation_detects_invalid_semver_in_cargo', () => {
  const root = createValidFixture()
  writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname = "ramshared"\nversion = "invalid-version"\n')
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /violates strict SemVer/)
})

test('release_automation_detects_missing_pr_header_sections', () => {
  const root = createValidFixture()
  writeFileSync(path.join(root, 'release-please-config.json'), JSON.stringify({
    "changelog-sections": [{ "type": "feat", "section": "Features" }],
    "pull-request-header": "## Summary\n"
  }))
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /pull-request-header missing section/)
})

test('release_automation_detects_missing_packaging_scripts', () => {
  const root = createValidFixture()
  writeFileSync(path.join(root, '.github', 'workflows', 'release-packaging.yml'), 'on:\n  push:\n    tags:\n      - \'beta*\'\n')
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /missing push tag v\* trigger/)
})

test('release_automation_detects_missing_readme_benchmark_verdict', () => {
  const root = createValidFixture()
  writeFileSync(path.join(root, 'README.md'), '# RamShared\n')
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /missing PASS_ZERO_PANIC/)
})

test('release_automation_detects_version_drift_across_release_sources', () => {
  const root = createValidFixture()
  writeFileSync(path.join(root, 'README.md'), 'Release v0.10.9\nTier 0\nTier 1\nTier 3\n19,777 MB\nPASS_ZERO_PANIC\n')
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /README\.md.*0\.10\.9.*Cargo\.toml.*0\.11\.0/i)
})

test('release_automation_rejects_universal_ublk_claims_for_standard_wsl2', () => {
  const root = createValidFixture()
  writeFileSync(
    path.join(root, 'docs', 'FAQ.md'),
    'Standard WSL2 uses ublk/io_uring as the universal default transport.\n',
  )
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /standard WSL2.*NBD.*baseline/i)
})

test('release_automation_rejects_swapped_evidence_assignments', () => {
  const root = createValidFixture()
  writeFileSync(
    path.join(root, 'ARCHITECTURE.md'),
    'EVD-0040 qualifies the ublk/io_uring transport. EVD-0039 proves zero-copy CUDA host mapping.\n',
  )
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, false)
  assert.match(result.findings.join('\n'), /EVD-0039.*ublk\/io_uring/i)
  assert.match(result.findings.join('\n'), /EVD-0040.*zero-copy CUDA host mapping/i)
})
