import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  run,
  validateAdrIndex,
} from './check-adr-index.mjs'

function adr(number, slug, sections = true) {
  const headings = sections
    ? '## Status\nAccepted\n\n## Context\nContext.\n\n## Decision\nDecision.\n\n## Consequences\nConsequences.\n\n## Alternatives considered\nNone.\n\n## Kahneman\n#16.\n\n## Rollback trigger\nOne observable failure.'
    : '## Status\nAccepted\n\n## Context\nContext.'
  return { filename: `ADR-${number}-${slug}.md`, text: `# ADR-${number}: Fixture\n\n${headings}\n` }
}

function index({ canonical, historical = [] }) {
  const canonicalRows = canonical.map(({ number, filename, profile = 'legacy' }) =>
    `| ${number} | [${filename}](${filename}) | Fixture | Accepted | ${profile} |`).join('\n')
  const historicalRows = historical.length === 0
    ? '| None | — | — | — |'
    : historical.map(({ filename, collidesWith = '0007', disposition = 'historical-noncanonical' }) =>
      `| [${filename}](${filename}) | ${collidesWith} | ${disposition} | Preserved filename collision; no new numeric ID may use this exception. |`).join('\n')
  return `# Architecture Decision Records (ADRs)

## Record format

New records use the governed-v1 profile and retain the required sections.

## Canonical index

| ADR | Filename | Title | Status | Governance profile |
| --- | --- | --- | --- | --- |
${canonicalRows}

## Historical filename collision

| Filename | Collides with canonical ADR | Registry disposition | Preservation note |
| --- | --- | --- | --- |
${historicalRows}
`
}

test('governed ADR index accepts unique canonical records and one declared historical collision', () => {
  const canonical = adr('0007', 'kernel-native-language-c', false)
  const governed = adr('0008', 'evidence-and-document-lifecycle')
  const historical = adr('0007', 'windows-local-broker-ipc', false)
  const text = index({
    canonical: [
      { number: '0007', filename: canonical.filename },
      { number: '0008', filename: governed.filename, profile: 'governed-v1' },
    ],
    historical: [{ filename: historical.filename }],
  })
  assert.deepEqual(validateAdrIndex(text, [canonical, governed, historical]), [])
})

test('ADR index rejects an undeclared duplicate numeric identifier', () => {
  const first = adr('0008', 'first')
  const second = adr('0008', 'second')
  const text = index({ canonical: [{ number: '0008', filename: first.filename, profile: 'governed-v1' }] })
  assert.match(validateAdrIndex(text, [first, second]).join('\n'), /duplicate-numeric-id/)
})

test('ADR index rejects a malformed historical exception and an orphan record', () => {
  const canonical = adr('0008', 'canonical')
  const orphan = adr('0007', 'legacy-collision', false)
  const unlisted = adr('0009', 'unlisted')
  const malformed = index({
    canonical: [{ number: '0008', filename: canonical.filename, profile: 'governed-v1' }],
    historical: [{ filename: orphan.filename, disposition: 'temporary-exception' }],
  })
  const errors = validateAdrIndex(malformed, [canonical, orphan, unlisted]).join('\n')
  assert.match(errors, /historical-disposition/)
  assert.match(errors, /orphan-record/)
})

test('ADR index rejects governed records missing a required section', () => {
  const governed = adr('0008', 'incomplete', false)
  const text = index({ canonical: [{ number: '0008', filename: governed.filename, profile: 'governed-v1' }] })
  assert.match(validateAdrIndex(text, [governed]).join('\n'), /governed-section-missing/)
})

test('ADR checker rejects malformed canonical index headings and unsafe filenames', () => {
  const safe = adr('0008', 'safe')
  const unsafe = { filename: 'ADR-0009-../escape.md', text: safe.text }
  const text = index({ canonical: [{ number: '0008', filename: safe.filename, profile: 'governed-v1' }] })
    .replace('## Canonical index', '## Index')
  const errors = validateAdrIndex(text, [safe, unsafe]).join('\n')
  assert.match(errors, /canonical-index-heading/)
  assert.match(errors, /filename-invalid/)
})

test('repository ADR index is currently valid', () => {
  const result = run({ root: process.cwd() })
  assert.equal(result.ok, true, result.errors.join('\n'))
  assert.equal(result.records, 9)
})

test('ADR checker stays read-only', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-adr-check-'))
  mkdirSync(path.join(root, 'docs', 'decisions'), { recursive: true })
  const canonical = adr('0008', 'fixture')
  writeFileSync(path.join(root, 'docs', 'decisions', canonical.filename), canonical.text)
  writeFileSync(path.join(root, 'docs', 'decisions', 'README.md'), index({ canonical: [{ number: '0008', filename: canonical.filename, profile: 'governed-v1' }] }))
  const result = run({ root })
  assert.equal(result.ok, true)
})
