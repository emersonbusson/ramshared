import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { appendFileSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { run, validateTaskLog } from './check-task-log.mjs'

const RECORD = `## TASK-0001 — Fixture task
**Schema:** \`ramshared.task.v1\`.
**Status:** \`in_progress\`.
**Owner role:** \`governance\`.
**Registered date:** \`2026-08-11\`.
**Registered time:** \`12:00:00\`.
**Updated date:** \`2026-08-11\`.
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

test('rejects combined timestamps or missing separate date and time fields', () => {
  const combined = taskLog()
    .replace('**Registered date:** `2026-08-11`.\n**Registered time:** `12:00:00`.', '**Registered at:** `2026-08-11T12:00:00-03:00`.')

  assert.match(JSON.stringify(validateTaskLog(combined)), /Registered date/)
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
