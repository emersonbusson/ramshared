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
