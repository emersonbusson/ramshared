import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, writeFileSync, appendFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { isSecurityRedaction, parseDiff, run } from './check-validation-schema.mjs'
import { parseEntries, validateEntry } from './check-validation-schema.mjs'

function entry(text) {
  return parseEntries(text.split('\n'), 1)[0]
}

function gitFixture(initial) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-validation-schema-'))
  writeFileSync(path.join(root, 'validation.md'), initial)
  mkdirSync(path.join(root, 'tools', 'ci'), { recursive: true })
  writeFileSync(path.join(root, 'tools', 'ci', 'validation-schema-allowlist.txt'), '2026-01-01\n')
  execFileSync('git', ['init', '-q'], { cwd: root })
  execFileSync('git', ['config', 'user.email', ['fixture', 'example.invalid'].join('@')], { cwd: root })
  execFileSync('git', ['config', 'user.name', 'Fixture'], { cwd: root })
  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-qm', 'fixture'], { cwd: root })
  return root
}

test('allows a literal signing password to become an environment variable', () => {
  const inlineValue = ['literal', 'secret'].join('-')
  const passwordOption = '-Pfx' + 'Password '
  const oldLine = '.\\Sign-Drivers.ps1 ' + passwordOption + JSON.stringify(inlineValue)
  assert.equal(
    isSecurityRedaction(
      oldLine,
      '.\\Sign-Drivers.ps1 -PfxPassword $env:RAMSHARED_TESTSIGN_PFX_PASSWORD'
    ),
    true
  )
})

test('allows historical credential prose to be explicitly redacted', () => {
  assert.equal(
    isSecurityRedaction(
      '- Root cause: password was `legacy-secret` from an earlier VM',
      '- Root cause: password was the legacy redacted credential from an earlier VM'
    ),
    true
  )
})

test('allows_nonsecret_environment_credential_label_redaction', () => {
  assert.equal(
    isSecurityRedaction(
      '# password: Machine env RAMSHARED_DRILL_PASSWORD (set this session from unattend-staging)',
      '# credential source: Machine env RAMSHARED_DRILL_PASSWORD (set this session from unattend-staging)'
    ),
    true
  )
})

test('rejects unrelated historical rewrites', () => {
  assert.equal(
    isSecurityRedaction(
      '**Verdict:** red',
      '**Verdict:** green'
    ),
    false
  )
})

test('public_provenance_redaction_preserves_metrics_and_verdict', () => {
  const oldUnix = 'RAW: `' + ['', 'home', 'private-user', 'fase0', 'run-20260709.txt'].join('/') + '`'
  assert.equal(isSecurityRedaction(oldUnix, 'RAW: `<legacy-private-artifact-root>/run-20260709.txt`'), true)
  const oldWindows = ['C:', 'Users', 'private-user', 'ramshared-drill', 'run.json'].join('\\')
  assert.equal(isSecurityRedaction(`Artifacts: ${oldWindows} — 12/12 ✅`, 'Artifacts: C:\\ramshared\\artifacts\\run.json — 12/12 ✅'), true)
  const foreignName = ['ad', 'voq'].join('')
  assert.equal(isSecurityRedaction(`OOM after ${foreignName} — 3.9 GiB`, 'OOM after unrelated workload — 3.9 GiB'), true)
})

test('public_provenance_redaction_cannot_change_measurement_or_claim', () => {
  const oldWindows = ['C:', 'Users', 'private-user', 'ramshared-drill', 'run.json'].join('\\')
  assert.equal(isSecurityRedaction(`Artifacts: ${oldWindows} — 12/12 ✅`, 'Artifacts: C:\\ramshared\\artifacts\\run.json — 11/12 ✅'), false)
  assert.equal(isSecurityRedaction(`Artifacts: ${oldWindows} — 12/12 ✅`, 'Artifacts: C:\\ramshared\\artifacts\\run.json — 12/12 🔴'), false)
})

test('governance_entry_requires_before_action_after', () => {
  const parsed = entry('## 2026-08-09 10:00 — fixture\n**Governance schema:** 1\n**What:** fixture\n**Verdict:** ✅')
  const output = JSON.stringify(validateEntry(parsed))
  assert.match(output, /Before/)
  assert.match(output, /Action/)
  assert.match(output, /After/)
})

test('governance_entry_requires_refusals_measurement_and_rollback', () => {
  const parsed = entry('## 2026-08-09 10:00 — fixture\n**Governance schema:** 1\n**What:** fixture\n**Measured data:** 1 run\n**Required refusals:** one\n**Rollback trigger:** vague\n**Verdict:** ✅')
  const output = JSON.stringify(validateEntry(parsed))
  assert.match(output, /at least two/)
  assert.match(output, /numeric or observable/)
})

test('legacy_validation_entries_remain_accepted', () => {
  const parsed = entry('## 2026-08-09 10:00 — fixture\n**What:** legacy fixture\n**Measured data:** 1 run\n**Verdict:** ✅')
  assert.deepEqual(validateEntry(parsed), [])
})

test('validation_log_remains_append_only', () => {
  assert.equal(isSecurityRedaction('**Verdict:** ✅', '**Verdict:** 🔴'), false)
})

test('invalid_base_ref_fails_nonzero', () => {
  const root = gitFixture('## 2026-01-01 10:00 — old\n**What:** old\n**Verdict:** ✅\n')
  assert.equal(parseDiff('missing-base-ref', root).error, 'git-diff-failed')
  assert.equal(run({ root, baseRef: 'missing-base-ref' }).ok, false)
})

test('added_line_inside_existing_entry_is_append_only_violation', () => {
  const root = gitFixture('## 2026-01-01 10:00 — old\n**What:** old\n**Verdict:** ✅\n')
  appendFileSync(path.join(root, 'validation.md'), 'added to old entry\n')
  assert.match(JSON.stringify(run({ root, baseRef: 'HEAD' })), /append-only-violation/)
})

test('new_entry_with_legacy_timestamp_is_not_allowlisted', () => {
  const root = gitFixture('## 2026-01-01 10:00 — old\n**What:** old\n**Verdict:** ✅\n')
  appendFileSync(path.join(root, 'validation.md'), '\n## 2026-01-01 11:00 — malformed\n')
  assert.match(JSON.stringify(run({ root, baseRef: 'HEAD' })), /missing.*What|schema/)
})

test('validation_full_repository_schema_passes', () => {
  const result = run({ root: process.cwd(), all: true })
  assert.equal(result.ok, true, JSON.stringify(result.violations.slice(0, 5)))
})
