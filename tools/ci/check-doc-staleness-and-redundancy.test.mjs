import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { checkDocStalenessAndRedundancy } from './check-doc-staleness-and-redundancy.mjs'

test('checkDocStalenessAndRedundancy: detects forbidden jargon in public docs', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  mkdirSync(path.join(temp, 'docs'), { recursive: true })
  writeFileSync(
    path.join(temp, 'ARCHITECTURE.md'),
    '# Architecture\n\nThis follows Kahneman #16 discipline strictly.'
  )

  const result = checkDocStalenessAndRedundancy({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(
    result.findings.some((f) =>
      f.includes('forbidden-jargon-in-public-doc:ARCHITECTURE.md:kahneman-discipline')
    )
  )
})

test('checkDocStalenessAndRedundancy: detects broken relative links in docs', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  mkdirSync(path.join(temp, 'docs'), { recursive: true })
  writeFileSync(
    path.join(temp, 'docs', 'broken-guide.md'),
    '# Guide\n\nSee [Ghost Spec](nonexistent-spec.md) for details.'
  )

  const result = checkDocStalenessAndRedundancy({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(
    result.findings.some((f) =>
      f.includes('broken-doc-link:docs/broken-guide.md:nonexistent-spec.md')
    )
  )
})

test('checkDocStalenessAndRedundancy: passes on clean fixture', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  mkdirSync(path.join(temp, 'docs'), { recursive: true })
  writeFileSync(
    path.join(temp, 'docs', 'target.md'),
    '# Target\nContent.'
  )
  writeFileSync(
    path.join(temp, 'docs', 'clean-guide.md'),
    '# Clean Guide\n\nSee [Target](target.md) for details.'
  )
  writeFileSync(
    path.join(temp, 'ARCHITECTURE.md'),
    '# Architecture\n\nClear systems engineering without jargon.'
  )

  const result = checkDocStalenessAndRedundancy({ root: temp })
  assert.equal(result.ok, true, `Expected pass, got: ${result.findings.join(', ')}`)
  assert.equal(result.findings.length, 0)
})

test('checkDocStalenessAndRedundancy: detects raw Windows host drive paths and out-of-tree WSL mounts', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  mkdirSync(path.join(temp, '.claude', 'rules'), { recursive: true })
  writeFileSync(
    path.join(temp, '.claude', 'rules', 'test-rule.md'),
    '# Rule\nDeploy at C:\\private\\workstation\\path or mount /mnt/c/Users/dev/data.'
  )

  const result = checkDocStalenessAndRedundancy({
    root: temp,
    boundaryFiles: [],
    boundaryDirs: ['.claude/rules'],
    skipBrokenLinks: true,
  })
  assert.equal(result.ok, false)
  assert.ok(
    result.findings.some(
      (f) => f.includes('host-path-leak') && f.includes('C:\\private\\workstation\\path')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('host-path-leak') && f.includes('/mnt/c/Users/dev/data')
    )
  )
})

test('checkDocStalenessAndRedundancy: detects foreign project cross-contamination', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  writeFileSync(
    path.join(temp, 'AGENTS.md'),
    '# Agents\nRun tests on civm or deploy via jules-operator for advoq.'
  )

  const result = checkDocStalenessAndRedundancy({
    root: temp,
    boundaryFiles: ['AGENTS.md'],
    boundaryDirs: [],
    skipBrokenLinks: true,
  })
  assert.equal(result.ok, false)
  assert.ok(
    result.findings.some(
      (f) => f.includes('foreign-project-cross-contamination') && f.includes('civm')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('foreign-project-cross-contamination') && f.includes('jules-operator')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('foreign-project-cross-contamination') && f.includes('advoq')
    )
  )
})

test('checkDocStalenessAndRedundancy: detects unredacted private lab VM identities and private network IPs', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  writeFileSync(
    path.join(temp, 'CLAUDE.md'),
    '# Claude\nConnect to gha-ubuntu-2404 at 192.168.0.100 or 10.0.1.5 or Tailscale 100.123.10.20.'
  )

  const result = checkDocStalenessAndRedundancy({
    root: temp,
    boundaryFiles: ['CLAUDE.md'],
    boundaryDirs: [],
    skipBrokenLinks: true,
  })
  assert.equal(result.ok, false)
  assert.ok(
    result.findings.some(
      (f) => f.includes('unredacted-private-vm-identity') && f.includes('gha-ubuntu-2404')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('private-network-address') && f.includes('192.168.0.100')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('private-network-address') && f.includes('10.0.1.5')
    )
  )
  assert.ok(
    result.findings.some(
      (f) => f.includes('private-network-address') && f.includes('100.123.10.20')
    )
  )
})

test('checkDocStalenessAndRedundancy: allows canonical paths, placeholders, and sanitized generic terms', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-staleness-test-'))
  writeFileSync(
    path.join(temp, 'README.md'),
    '# RamShared\nConfig is in C:\\ProgramData\\RamShared\\config.toml.\nSystem file: C:\\Windows\\System32\\drivers.\nExample: C:\\path\\to\\dir.\nVirtual swap: X:\\pagefile.sys.\nRun in isolated VM or QEMU environment.'
  )

  const result = checkDocStalenessAndRedundancy({
    root: temp,
    boundaryFiles: ['README.md'],
    boundaryDirs: [],
    skipBrokenLinks: true,
  })
  assert.equal(result.ok, true, `Expected pass, got: ${result.findings.join(', ')}`)
  assert.equal(result.findings.length, 0)
})

test('checkDocStalenessAndRedundancy: passes on live repository tree', () => {
  const result = checkDocStalenessAndRedundancy()
  assert.equal(result.ok, true, `Expected repo to be clean, got: ${result.findings.join('\n')}`)
  assert.equal(result.findings.length, 0)
})
