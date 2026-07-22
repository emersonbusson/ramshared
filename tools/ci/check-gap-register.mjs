#!/usr/bin/env node
/**
 * Validates docs/reliability/GAP-REGISTER.md as a machine-checkable guardrail.
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  '..'
)

const REGISTER = 'docs/reliability/GAP-REGISTER.md'
const REQUIRED_LINKS = [
  'README.md',
  'docs/specs/no-milestone/memory-broker/IMPL.md',
]
const OPEN_STATUSES = new Set(['PARTIAL', 'DEFERRED', 'BLOCKED'])
const BAD_PLACEHOLDERS = /\b(?:TBD|TODO|PENDING|UNKNOWN|N\/A|none)\b/i

function read(rel) {
  return readFileSync(path.join(ROOT, rel), 'utf8')
}

function lineOf(text, needle) {
  const idx = text.indexOf(needle)
  if (idx === -1) return 0
  return text.slice(0, idx).split('\n').length
}

function section(text, heading) {
  const start = text.indexOf(`## ${heading}`)
  if (start === -1) return null
  const rest = text.slice(start)
  const next = rest.slice(1).search(/\n## /)
  return next === -1 ? rest : rest.slice(0, next + 1)
}

function tableRows(sectionText) {
  return sectionText
    .split('\n')
    .filter((line) => line.startsWith('|'))
    .filter((line) => !/^\|\s*-+/.test(line))
    .slice(1)
    .map((line) => line.split('|').slice(1, -1).map((cell) => cell.trim()))
}

function main() {
  const findings = []
  const register = read(REGISTER)

  const open = section(register, 'Current Open Gates')
  if (!open) {
    findings.push(`${REGISTER}:1 — missing Current Open Gates section`)
  } else {
    const rows = tableRows(open)
    if (rows.length === 0) {
      findings.push(`${REGISTER}:1 — Current Open Gates table is empty`)
    }
    for (const row of rows) {
      const [gate, status, why, evidence] = row
      const line = lineOf(register, `| ${gate} | ${status} |`)
      if (row.length !== 4) {
        findings.push(`${REGISTER}:${line} — open gate row must have 4 cells`)
        continue
      }
      if (!gate || !status || !why || !evidence) {
        findings.push(`${REGISTER}:${line} — open gate row has an empty cell`)
      }
      if (!OPEN_STATUSES.has(status)) {
        findings.push(
          `${REGISTER}:${line} — open gate status must be PARTIAL, DEFERRED, or BLOCKED`
        )
      }
      if (status === 'DONE' || status === 'PASS') {
        findings.push(`${REGISTER}:${line} — open gate cannot be marked ${status}`)
      }
      if (BAD_PLACEHOLDERS.test(evidence)) {
        findings.push(
          `${REGISTER}:${line} — close evidence must be concrete, not placeholder text`
        )
      }
      if (!/\b(PASS|proof|evidence|run|round|campaign|SPEC|BINARY_MATCH|KTEST|terminal)\b/i.test(evidence)) {
        findings.push(
          `${REGISTER}:${line} — close evidence must describe an observable proof`
        )
      }
    }
  }

  const closed = section(register, 'Closed In This Session')
  if (!closed) {
    findings.push(`${REGISTER}:1 — missing Closed In This Session section`)
  } else if (tableRows(closed).length === 0) {
    findings.push(`${REGISTER}:1 — Closed In This Session table is empty`)
  }

  for (const rel of REQUIRED_LINKS) {
    const text = read(rel)
    if (!text.includes('docs/reliability/GAP-REGISTER.md')) {
      findings.push(`${rel}:1 — missing link to ${REGISTER}`)
    }
  }

  if (findings.length > 0) {
    for (const f of findings) console.error(f)
    process.exit(1)
  }
  console.log('✓ gap register OK')
}

main()
