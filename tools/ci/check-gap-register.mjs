#!/usr/bin/env node
/**
 * Validates docs/reliability/GAP-REGISTER.md as a machine-checkable guardrail.
 */
import { existsSync, readFileSync } from 'node:fs'
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

// Semantic anti-phantom-blocker patterns: expressions alleging defects or external
// blockers that have already been resolved and verified in active CI test suites.
const FORBIDDEN_OBSOLETE_BLOCKERS = [
  { name: 'obsolete-guard-repair', regex: /\b(?:await|pending|awaiting)\s+(?:the\s+)?external\s+guard\s+repair\b/i },
  { name: 'generic-guard-repair', regex: /\bguard\s+repair\b/i },
]

// Mandatory reliability milestones that must be recorded as closed evidence
const REQUIRED_CLOSED_MILESTONES = [
  { name: 'tier-3-ssd', regex: /Tier\s*3\s*(?:\(SSD\)|\bSSD\b)/i },
  { name: 'rust-ci-guardrails', regex: /Rust\s+(?:CI|slice\s+coverage)/i },
]

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

export function checkGapRegister({ root = ROOT } = {}) {
  const findings = []
  const registerPath = path.join(root, REGISTER)

  if (!existsSync(registerPath)) {
    findings.push(`${REGISTER}:1 — missing gap register file`)
    return { ok: false, findings }
  }

  const register = readFileSync(registerPath, 'utf8')

  // Semantic check: forbid obsolete / phantom blocker phrases anywhere in the register
  for (const { name, regex } of FORBIDDEN_OBSOLETE_BLOCKERS) {
    if (regex.test(register)) {
      const line = lineOf(register, register.match(regex)?.[0] ?? '')
      findings.push(`${REGISTER}:${line} — phantom/obsolete blocker detected: ${name}`)
    }
  }

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
  } else {
    const closedRows = tableRows(closed)
    if (closedRows.length === 0) {
      findings.push(`${REGISTER}:1 — Closed In This Session table is empty`)
    } else {
      // Check mandatory milestone coverage in closed table
      for (const { name, regex } of REQUIRED_CLOSED_MILESTONES) {
        const found = closedRows.some((r) => regex.test(r[0] ?? '') || regex.test(r[1] ?? ''))
        if (!found) {
          findings.push(`${REGISTER}:1 — missing closed milestone evidence for ${name}`)
        }
      }
    }
  }

  for (const rel of REQUIRED_LINKS) {
    const linkPath = path.join(root, rel)
    if (!existsSync(linkPath)) continue
    const text = readFileSync(linkPath, 'utf8')
    if (!text.includes('docs/reliability/GAP-REGISTER.md')) {
      findings.push(`${rel}:1 — missing link to ${REGISTER}`)
    }
  }

  return {
    ok: findings.length === 0,
    findings,
  }
}

function main() {
  const result = checkGapRegister({ root: ROOT })
  if (!result.ok) {
    for (const f of result.findings) console.error(f)
    process.exit(1)
  }
  console.log('✓ gap register OK')
}

// Support direct CLI execution
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
