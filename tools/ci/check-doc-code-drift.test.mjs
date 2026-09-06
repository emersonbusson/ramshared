import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { checkDocCodeDrift } from './check-doc-code-drift.mjs'

test('checkDocCodeDrift: passes on current repository layout', () => {
  const result = checkDocCodeDrift()
  assert.equal(result.ok, true, `Expected pass, got findings: ${result.findings.join(', ')}`)
  assert.equal(result.findings.length, 0)
  assert.equal(result.checkedCrates, 15)
})

test('checkDocCodeDrift: detects missing crate readme', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-drift-test-'))
  mkdirSync(path.join(temp, 'crates', 'mock-crate'), { recursive: true })
  writeFileSync(path.join(temp, 'ARCHITECTURE.md'), '[`mock-crate`](crates/mock-crate/README.md)')

  const result = checkDocCodeDrift({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(result.findings.some((f) => f.includes('missing-crate-readme:crates/mock-crate/README.md')))
})

test('checkDocCodeDrift: detects unreferenced crate in architecture', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'doc-drift-test-'))
  const crateDir = path.join(temp, 'crates', 'orphan-crate')
  mkdirSync(crateDir, { recursive: true })
  writeFileSync(
    path.join(crateDir, 'README.md'),
    '# orphan-crate\n## Scope & Responsibility\n## Workspace Dependencies\n## Safety Invariants\n## Testing\n'
  )
  writeFileSync(path.join(temp, 'ARCHITECTURE.md'), 'Architecture without orphan crate.')

  const result = checkDocCodeDrift({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(result.findings.some((f) => f.includes('unreferenced-crate-in-architecture:crates/orphan-crate')))
})
