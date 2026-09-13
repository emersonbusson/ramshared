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

  const readmeContent = '## Multi-Tier Hardware Benchmark Comparison\nTier 0 ZRAM\nTier 1 GPU VRAM\nTier 3 SSD\n19,777 MB\nPASS_ZERO_PANIC\n'
  writeFileSync(path.join(root, 'README.md'), readmeContent)
  writeFileSync(path.join(root, 'README.pt-BR.md'), readmeContent)

  return root
}

test('release_automation_passes_on_valid_repository', () => {
  const root = createValidFixture()
  const result = checkReleaseAutomation({ root })
  assert.equal(result.ok, true)
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
