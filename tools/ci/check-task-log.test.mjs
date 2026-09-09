import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { appendFileSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { run, validateTaskLog, main } from './check-task-log.mjs'

const RECORD = `## TASK-0001 — Fixture task
**Schema:** \`ramshared.task.v1\`.
**Status:** \`in_progress\`.
**Owner role:** \`governance\`.
**Date:** \`2026-08-11\`.
**Registered time:** \`12:00:00\`.
**Updated time:** \`12:00:00\`.
**Source revision:** \`fd5cbf2d39a026bcf737a3082ef2497d3861b257\`.
**Destinations:** \`TASK.md\`.
**Scope:** Fixture coverage.
**Evidence / blockers:** None.
`

function taskLog(records = RECORD) {
  return `# TASK.md — RamShared

## Record schema

<!-- task-schema-v1 -->

${records}`
}

function gitFixture(initial = taskLog()) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-task-log-'))
  writeFileSync(path.join(root, 'TASK.md'), initial)
  execFileSync('git', ['init', '-q'], { cwd: root })
  execFileSync('git', ['config', 'user.email', 'fixture'], {
    cwd: root,
  })
  execFileSync('git', ['config', 'user.name', 'Fixture'], { cwd: root })
  execFileSync('git', ['add', 'TASK.md'], { cwd: root })
  execFileSync('git', ['commit', '-qm', 'fixture'], { cwd: root })
  return root
}

test('accepts a complete versioned task record', () => {
  assert.deepEqual(validateTaskLog(taskLog()), [])
})

test('uses one date when registration and update share a calendar day', () => {
  const redundant = taskLog(
    RECORD.replace(
      '**Date:** `2026-08-11`.',
      '**Registered date:** `2026-08-11`.\n**Updated date:** `2026-08-11`.'
    )
  )

  assert.deepEqual(validateTaskLog(taskLog()), [])
  assert.match(JSON.stringify(validateTaskLog(redundant)), /shared/)
})

test('rejects combined timestamps or missing separate date and time fields', () => {
  const combined = taskLog()
    .replace('**Date:** `2026-08-11`.\n**Registered time:** `12:00:00`.', '**Registered at:** `2026-08-11T12:00:00-03:00`.')

  assert.match(JSON.stringify(validateTaskLog(combined)), /Date/)
  assert.match(JSON.stringify(validateTaskLog(combined)), /Registered time/)
})

test('rejects a task record without temporal provenance', () => {
  const invalid = taskLog(RECORD.replace('**Updated time:** `12:00:00`.\n', ''))
  assert.match(JSON.stringify(validateTaskLog(invalid)), /Updated time/)
})

test('rejects duplicate task IDs', () => {
  assert.match(JSON.stringify(validateTaskLog(taskLog(`${RECORD}\n${RECORD}`))), /duplicate/)
})

test('accepts a new task appended after the marker', () => {
  const root = gitFixture()
  appendFileSync(
    path.join(root, 'TASK.md'),
    `\n${RECORD.replaceAll('TASK-0001', 'TASK-0002')}`
  )
  assert.deepEqual(run({ root, baseRef: 'HEAD' }), { ok: true, violations: [] })
})

test('accepts a new task log when the Git base did not contain TASK.md', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-task-log-new-file-'))
  writeFileSync(path.join(root, 'baseline.txt'), 'baseline\n')
  execFileSync('git', ['init', '-q'], { cwd: root })
  execFileSync('git', ['config', 'user.email', 'fixture'], {
    cwd: root,
  })
  execFileSync('git', ['config', 'user.name', 'Fixture'], { cwd: root })
  execFileSync('git', ['add', 'baseline.txt'], { cwd: root })
  execFileSync('git', ['commit', '-qm', 'baseline'], { cwd: root })
  writeFileSync(path.join(root, 'TASK.md'), taskLog())
  assert.deepEqual(run({ root, baseRef: 'HEAD' }), { ok: true, violations: [] })
})

test('requires updated date and time when an existing task record changes', () => {
  const root = gitFixture()
  const file = path.join(root, 'TASK.md')
  writeFileSync(
    file,
    taskLog(RECORD.replace('Fixture coverage.', 'Changed fixture coverage.'))
  )
  assert.match(JSON.stringify(run({ root, baseRef: 'HEAD' })), /Updated date/)
})

test('accepts an existing task update with a newer updated time', () => {
  const root = gitFixture()
  writeFileSync(
    path.join(root, 'TASK.md'),
    taskLog(
      RECORD.replace('Fixture coverage.', 'Changed fixture coverage.').replace(
        '**Updated time:** `12:00:00`.',
        '**Updated time:** `12:01:00`.'
      )
    )
  )
  assert.deepEqual(run({ root, baseRef: 'HEAD' }), { ok: true, violations: [] })
})

test('test_cli_missing_args_fails', () => {
  let errOutput = ''
  const stderr = { write: (msg) => { errOutput += msg } }

  assert.equal(main([], { stderr }), 2)
  assert.match(errOutput, /usage/)
})

test('test_cli_diff_missing_base_ref_fails', () => {
  let errOutput = ''
  const stderr = { write: (msg) => { errOutput += msg } }

  assert.equal(main(['--diff'], { stderr }), 2)
  assert.match(errOutput, /usage/)
})

test('test_cli_diff_invalid_base_ref_fails', () => {
  let errOutput = ''
  const stderr = { write: (msg) => { errOutput += msg } }

  assert.equal(main(['--diff', '--all'], { stderr }), 2)
  assert.match(errOutput, /usage/)
})

test('test_cli_all_success', () => {
  let outOutput = ''
  const stdout = { write: (msg) => { outOutput += msg } }

  const root = gitFixture()
  assert.equal(main(['--all'], { stdout, root }), 0)
  assert.match(outOutput, /schema OK/)
})

test('test_cli_diff_violations_returns_1', () => {
  let outOutput = ''
  const stdout = { write: (msg) => { outOutput += msg } }

  const root = gitFixture()
  writeFileSync(path.join(root, 'TASK.md'), taskLog(RECORD.replace('**Updated time:** `12:00:00`.', '')))

  assert.equal(main(['--diff', 'HEAD'], { stdout, root }), 1)
  assert.match(outOutput, /missing or invalid \`\*\*Updated time:\*\*\`/)
})

test('test_cli_diff_error_returns_1', () => {
  let outOutput = ''
  const stdout = { write: (msg) => { outOutput += msg } }

  const root = gitFixture()

  assert.equal(main(['--diff', 'invalid-ref'], { stdout, root }), 1)
  assert.match(outOutput, /unable to read the requested Git base revision/)
})

test('test_validate_task_log_no_marker', () => {
  assert.equal(validateTaskLog('missing marker').length, 1)
})

test('test_validate_task_log_no_records', () => {
  assert.equal(validateTaskLog('<!-- task-schema-v1 -->\n').length, 1)
})

test('test_run_missing_task_md', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-task-log-missing-'))
  const result = run({ root })
  assert.equal(result.ok, false)
  assert.match(result.violations[0].message, /TASK.md does not exist/)
})

test('test_run_diff_missing_git_ref', () => {
  const root = gitFixture()
  const result = run({ root, baseRef: 'invalid-ref' })
  assert.equal(result.ok, false)
  assert.match(result.violations[0].message, /unable to read the requested Git base revision/)
})

test('test_run_diff_task_removed', () => {
  const root = gitFixture()
  writeFileSync(path.join(root, 'TASK.md'), taskLog(''))
  const result = run({ root, baseRef: 'HEAD' })
  assert.equal(result.ok, false)
  assert.match(JSON.stringify(result.violations), /was removed/)
})

test('test_validateRecord_missing_schema', () => {
  const violations = validateTaskLog(taskLog(RECORD.replace('**Schema:** `ramshared.task.v1`.', '')))
  assert.match(JSON.stringify(violations), /missing \`\*\*Schema:\*\* \`ramshared.task.v1\`\`/)
})

test('test_validateRecord_invalid_updated_precedes_registered', () => {
  const violations = validateTaskLog(taskLog(RECORD.replace('**Updated time:** `12:00:00`.', '**Updated time:** `11:00:00`.')))
  assert.match(JSON.stringify(violations), /must not precede/)
})

test('test_validateRecord_invalid_source_revision', () => {
  const violations = validateTaskLog(taskLog(RECORD.replace('**Source revision:** `fd5cbf2d39a026bcf737a3082ef2497d3861b257`.', '**Source revision:** `xyz`.')))
  assert.match(JSON.stringify(violations), /missing or invalid \`\*\*Source revision:\*\*\` Git revision/)
})
