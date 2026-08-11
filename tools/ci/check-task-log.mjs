#!/usr/bin/env node
/**
 * Validates the RamShared task log and its temporal provenance.
 *
 * `TASK.md` is intentionally mutable: task state may change. A change to an
 * existing task must advance its separate updated date/time fields, so the log
 * remains reviewable without crowding every record with a timezone suffix.
 */
import { existsSync, readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const TARGET = 'TASK.md'
const MARKER = '<!-- task-schema-v1 -->'
const TASK_HEADER_RE = /^## (TASK-\d{4,})\s+—\s+(.+)$/
const LEGACY_TIMESTAMP_RE = /^(?<date>\d{4}-\d{2}-\d{2})T(?<time>\d{2}:\d{2}:\d{2}(?:\.\d+)?)(?:Z|[+-]\d{2}:\d{2})$/
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/
const TIME_RE = /^\d{2}:\d{2}:\d{2}(?:\.\d+)?$/
const SOURCE_REVISION_RE = /^[0-9a-f]{7,64}$/i
const STATUSES = new Set(['planned', 'in_progress', 'blocked', 'completed', 'cancelled'])

function value(body, label) {
  const line = body.find((item) => item.startsWith(`**${label}:**`))
  if (!line) return ''
  return line
    .slice(`**${label}:**`.length)
    .trim()
    .replace(/^`|`\.?$/g, '')
    .replace(/\.$/, '')
    .trim()
}

function isCalendarDate(value) {
  if (!DATE_RE.test(value)) return false
  const [year, month, day] = value.split('-').map(Number)
  const date = new Date(Date.UTC(year, month - 1, day))
  return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day
}

function isClockTime(value) {
  if (!TIME_RE.test(value)) return false
  const [hour, minute, second] = value.split(/[.:]/).map(Number)
  return hour < 24 && minute < 60 && second < 60
}

function temporalFields(record, prefix) {
  const sharedDate = value(record.body, 'Date')
  const specificDate = value(record.body, `${prefix} date`)
  const time = value(record.body, `${prefix} time`)
  if (sharedDate || specificDate || time) {
    return {
      date: specificDate || sharedDate,
      time,
      sharedDate: Boolean(sharedDate),
      specificDate: Boolean(specificDate),
      legacy: false,
    }
  }
  const legacy = value(record.body, `${prefix} at`).match(LEGACY_TIMESTAMP_RE)
  if (!legacy) return { date: '', time: '', legacy: false }
  return { date: legacy.groups.date, time: legacy.groups.time, legacy: true }
}

function temporalValue(temporal) {
  if (!isCalendarDate(temporal.date) || !isClockTime(temporal.time)) return null
  return `${temporal.date}T${temporal.time}`
}

function temporalLine(line) {
  return /^(?:\*\*Date:\*\*|\*\*(?:Registered|Updated) (?:at|date|time):\*\*)/u.test(line)
}

function parseTaskLog(text) {
  const lines = text.split(/\r?\n/)
  const markerLine = lines.findIndex((line) => line.trim() === MARKER)
  const records = []
  let current = null
  for (let index = Math.max(0, markerLine + 1); index < lines.length; index++) {
    const match = lines[index].match(TASK_HEADER_RE)
    if (match) {
      if (current) records.push(current)
      current = { id: match[1], title: match[2], headerLine: index + 1, body: [] }
    } else if (current) {
      current.body.push(lines[index])
    }
  }
  if (current) records.push(current)
  return { markerLine: markerLine + 1, records }
}

function validateRecord(record) {
  const violations = []
  const add = (message) => violations.push({ line: record.headerLine, rule: 'task-schema', message })
  if (!record.title.trim()) add('task title is empty')
  if (value(record.body, 'Schema') !== 'ramshared.task.v1') {
    add('missing `**Schema:** `ramshared.task.v1``')
  }
  const status = value(record.body, 'Status')
  if (!STATUSES.has(status)) add('`**Status:**` must be planned, in_progress, blocked, completed, or cancelled')
  if (!value(record.body, 'Owner role')) add('missing `**Owner role:**`')

  const registered = temporalFields(record, 'Registered')
  const updated = temporalFields(record, 'Updated')
  for (const [label, temporal] of [['Registered', registered], ['Updated', updated]]) {
    if (temporal.legacy) add(`use separate \`**Date:**\` or \`**${label} date:**\`, plus \`**${label} time:**\` fields`)
    if (!isCalendarDate(temporal.date)) add(`missing or invalid \`**Date:**\` / \`**${label} date:**\``)
    if (!isClockTime(temporal.time)) add(`missing or invalid \`**${label} time:**\``)
  }
  const registeredValue = temporalValue(registered)
  const updatedValue = temporalValue(updated)
  if (registeredValue && updatedValue && updatedValue < registeredValue) {
    add('`**Updated date/time:**` must not precede `**Registered date/time:**`')
  }
  if (
    (registered.sharedDate && (registered.specificDate || updated.specificDate)) ||
    (!registered.sharedDate &&
      registeredValue &&
      updatedValue &&
      registered.date === updated.date)
  ) {
    add('use one shared `**Date:**` when registration and update share a calendar day')
  }
  if (!SOURCE_REVISION_RE.test(value(record.body, 'Source revision'))) {
    add('missing or invalid `**Source revision:**` Git revision')
  }
  for (const label of ['Destinations', 'Scope', 'Evidence / blockers']) {
    if (!value(record.body, label)) add(`missing \`**${label}:**\``)
  }
  return violations
}

export function validateTaskLog(text) {
  const { markerLine, records } = parseTaskLog(text)
  const violations = []
  if (markerLine === 0) {
    violations.push({ line: 0, rule: 'marker', message: `missing ${MARKER}` })
    return violations
  }
  if (records.length === 0) {
    violations.push({ line: markerLine, rule: 'sentinel', message: 'task log has no task records after its schema marker' })
    return violations
  }
  const ids = new Set()
  for (const record of records) {
    if (ids.has(record.id)) {
      violations.push({ line: record.headerLine, rule: 'duplicate-id', message: `duplicate task ID ${record.id}` })
    }
    ids.add(record.id)
    violations.push(...validateRecord(record))
  }
  return violations
}

function readBaseTaskLog(root, baseRef) {
  try {
    execFileSync('git', ['rev-parse', '--verify', `${baseRef}^{commit}`], {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch {
    return { validBase: false, text: null }
  }
  try {
    return {
      validBase: true,
      text: execFileSync('git', ['show', `${baseRef}:${TARGET}`], {
        cwd: root,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      }),
    }
  } catch {
    return { validBase: true, text: null }
  }
}

function comparable(record) {
  return record.body.filter((line) => !temporalLine(line)).join('\n').trim()
}

export function run({ root = ROOT, baseRef = null, all = false } = {}) {
  const file = path.join(root, TARGET)
  if (!existsSync(file)) {
    return { ok: false, violations: [{ line: 0, rule: 'sentinel', message: `${TARGET} does not exist` }] }
  }
  const current = readFileSync(file, 'utf8')
  const violations = validateTaskLog(current)
  if (!all && baseRef) {
    const base = readBaseTaskLog(root, baseRef)
    if (!base.validBase) {
      violations.push({ line: 0, rule: 'diff-error', message: 'unable to read the requested Git base revision' })
    } else if (base.text !== null) {
      const before = new Map(parseTaskLog(base.text).records.map((record) => [record.id, record]))
      const after = new Map(parseTaskLog(current).records.map((record) => [record.id, record]))
      for (const [id, prior] of before) {
        const next = after.get(id)
        if (!next) {
          violations.push({ line: prior.headerLine, rule: 'task-removal', message: `existing task ${id} was removed` })
          continue
        }
        if (comparable(prior) === comparable(next)) continue
        const priorUpdated = temporalValue(temporalFields(prior, 'Updated'))
        const nextUpdated = temporalValue(temporalFields(next, 'Updated'))
        if (!nextUpdated || nextUpdated === priorUpdated || nextUpdated <= priorUpdated) {
          violations.push({ line: next.headerLine, rule: 'temporal-update', message: `changed task ${id} requires a newer \`**Updated date:**\` and \`**Updated time:**\`` })
        }
      }
    }
  }
  return { ok: violations.length === 0, violations }
}

function main(argv = process.argv.slice(2)) {
  const all = argv.includes('--all')
  const diffIndex = argv.indexOf('--diff')
  const baseRef = diffIndex === -1 ? null : argv[diffIndex + 1]
  if (!all && !baseRef) {
    process.stderr.write('usage: check-task-log.mjs (--all | --diff <baseRef>)\n')
    return 2
  }
  const result = run({ all, baseRef })
  if (result.ok) {
    process.stdout.write(`✓ ${TARGET} schema OK (${all ? 'all records' : `diff vs ${baseRef}`})\n`)
    return 0
  }
  for (const violation of result.violations) {
    process.stdout.write(`${TARGET}:${violation.line} — ${violation.rule}: ${violation.message}\n`)
  }
  return 1
}

if (import.meta.url === `file://${process.argv[1]}`) process.exitCode = main()
