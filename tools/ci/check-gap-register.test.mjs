import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { checkGapRegister } from './check-gap-register.mjs'

test('checkGapRegister: passes on current repository', () => {
  const result = checkGapRegister()
  assert.equal(result.ok, true, `Expected pass, got findings:\n${result.findings.join('\n')}`)
  assert.equal(result.findings.length, 0)
})

test('checkGapRegister: detects phantom/obsolete Guard repair blocker', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'gap-reg-test-'))
  mkdirSync(path.join(temp, 'docs', 'reliability'), { recursive: true })
  mkdirSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker'), { recursive: true })

  writeFileSync(path.join(temp, 'README.md'), '[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md)')
  writeFileSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker', 'IMPL.md'), '[`docs/reliability/GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md)')

  const badContent = `# RamShared Gap Register

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| Gate 1 | PARTIAL | We await the external Guard repair before testing | PASS run proof |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| Tier 3 SSD | PASS evidence |
| Rust CI | PASS evidence |
`
  writeFileSync(path.join(temp, 'docs', 'reliability', 'GAP-REGISTER.md'), badContent)

  const result = checkGapRegister({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(result.findings.some((f) => f.includes('phantom/obsolete blocker detected: obsolete-guard-repair')))
})

test('checkGapRegister: detects missing closed milestone', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'gap-reg-test-'))
  mkdirSync(path.join(temp, 'docs', 'reliability'), { recursive: true })
  mkdirSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker'), { recursive: true })

  writeFileSync(path.join(temp, 'README.md'), '[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md)')
  writeFileSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker', 'IMPL.md'), '[`docs/reliability/GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md)')

  const missingMilestoneContent = `# RamShared Gap Register

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| Gate 1 | PARTIAL | Genuine open reason | PASS run proof |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| Some Other Feature | PASS evidence |
`
  writeFileSync(path.join(temp, 'docs', 'reliability', 'GAP-REGISTER.md'), missingMilestoneContent)

  const result = checkGapRegister({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(result.findings.some((f) => f.includes('missing closed milestone evidence for tier-3-ssd')))
})

test('checkGapRegister: detects invalid status, bad row length, and placeholders', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'gap-reg-test-'))
  mkdirSync(path.join(temp, 'docs', 'reliability'), { recursive: true })
  mkdirSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker'), { recursive: true })

  writeFileSync(path.join(temp, 'README.md'), 'No link here')
  writeFileSync(path.join(temp, 'docs', 'specs', 'no-milestone', 'memory-broker', 'IMPL.md'), 'No link here either')

  const badStatusContent = `# RamShared Gap Register

## Current Open Gates

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| Gate 1 | DONE | Completed | PASS run proof |
| Gate 2 | INVALID | Still pending | TODO |
| Gate 3 | PARTIAL | Extra cell | extra | PASS proof |
| Gate 4 | PARTIAL | | PASS proof |

## Closed In This Session

| Gap | Close evidence |
| --- | --- |
| Tier 3 SSD | PASS evidence |
| Rust CI | PASS evidence |
`
  writeFileSync(path.join(temp, 'docs', 'reliability', 'GAP-REGISTER.md'), badStatusContent)

  const result = checkGapRegister({ root: temp })
  assert.equal(result.ok, false)
  assert.ok(result.findings.some((f) => f.includes('open gate cannot be marked DONE')))
  assert.ok(result.findings.some((f) => f.includes('open gate status must be PARTIAL, DEFERRED, or BLOCKED')))
  assert.ok(result.findings.some((f) => f.includes('open gate row must have 4 cells')))
  assert.ok(result.findings.some((f) => f.includes('open gate row has an empty cell')))
  assert.ok(result.findings.some((f) => f.includes('close evidence must be concrete, not placeholder text')))
  assert.ok(result.findings.some((f) => f.includes('missing link to docs/reliability/GAP-REGISTER.md')))
})

test('checkGapRegister: detects missing file, empty tables, and missing closed section', () => {
  const temp = mkdtempSync(path.join(tmpdir(), 'gap-reg-test-'))
  const resultMissingFile = checkGapRegister({ root: temp })
  assert.equal(resultMissingFile.ok, false)
  assert.ok(resultMissingFile.findings.some((f) => f.includes('missing gap register file')))

  mkdirSync(path.join(temp, 'docs', 'reliability'), { recursive: true })
  writeFileSync(path.join(temp, 'docs', 'reliability', 'GAP-REGISTER.md'), '# Empty File\n\n## Current Open Gates\n\nNo table here\n')
  const resultEmpty = checkGapRegister({ root: temp })
  assert.equal(resultEmpty.ok, false)
  assert.ok(resultEmpty.findings.some((f) => f.includes('Current Open Gates table is empty')))
  assert.ok(resultEmpty.findings.some((f) => f.includes('missing Closed In This Session section')))

  writeFileSync(path.join(temp, 'docs', 'reliability', 'GAP-REGISTER.md'), '# File\n\n## Current Open Gates\n\n| G | S | W | E |\n|---|---|---|---|\n| G | PARTIAL | W | PASS proof |\n\n## Closed In This Session\n\nNo table here\n')
  const resultClosedEmpty = checkGapRegister({ root: temp })
  assert.equal(resultClosedEmpty.ok, false)
  assert.ok(resultClosedEmpty.findings.some((f) => f.includes('Closed In This Session table is empty')))
})
