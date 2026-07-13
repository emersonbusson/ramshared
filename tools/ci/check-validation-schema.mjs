#!/usr/bin/env node
/**
 * Static schema validator for validation.md: validates schema format of new entries,
 * and enforces append-only log rules.
 *
 * Mode:
 *   --diff <baseRef>  validates only entries added in diff vs baseRef + append-only (CI).
 *   --all             validates all entries in the file.
 *
 * Output format: `validation.md:<line> — <rule>: <message>`
 */
import { readFileSync, existsSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  '..'
)
const TARGET = 'validation.md'
const ALLOWLIST_PATH = 'tools/ci/validation-schema-allowlist.txt'

function loadAllowlist(root) {
  const file = path.join(root, ALLOWLIST_PATH)
  if (!existsSync(file)) return new Set()
  return new Set(
    readFileSync(file, 'utf8')
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith('#'))
  )
}

function entryTimestamp(header) {
  const m = header.match(/^## (\d{4}-\d{2}-\d{2}(?:\s+\d{2}:\d{2})?)/)
  return m ? m[1] : null
}

const VERDICT_RE = /[✅🔴🟡]/
const ENTRY_HEADER_RE = /^## \d{4}-\d{2}-\d{2}/
const ENTRY_HEADER_FULL_RE = /^## \d{4}-\d{2}-\d{2} \d{2}:\d{2}\b/
const BOLD_LABEL_RE = /^\*\*[^*]+:\*\*/
const DIGIT_RE = /\d/

const EFFECT_CATEGORIES = new Set([
  'integration',
  'isolation',
  'e2e',
  'ci-gate',
])

function stripDiacritics(s) {
  return s.normalize('NFD').replace(/[̀-ͯ]/g, '')
}

export function findFirstEntryLine(lines) {
  for (let i = 0; i < lines.length; i++) {
    if (ENTRY_HEADER_RE.test(lines[i])) return i + 1
  }
  return lines.length + 1
}

export function parseEntries(lines, firstEntryLine) {
  const entries = []
  let cur = null
  for (let i = firstEntryLine - 1; i < lines.length; i++) {
    const ln = lines[i]
    if (ENTRY_HEADER_RE.test(ln)) {
      if (cur) entries.push(cur)
      cur = { headerLine: i + 1, header: ln, body: [] }
    } else if (cur) {
      cur.body.push(ln)
    }
  }
  if (cur) entries.push(cur)
  return entries
}

export function getLabelBlock(body, label) {
  const deLabel = stripDiacritics(label).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(
    '^\\*\\*' + deLabel + '(?:\\s*\\([^)]*\\))?:\\*\\*\\s*(.*)$'
  )
  for (let i = 0; i < body.length; i++) {
    const m = stripDiacritics(body[i]).match(re)
    if (!m) continue
    const parts = [m[1]]
    for (let j = i + 1; j < body.length; j++) {
      if (BOLD_LABEL_RE.test(stripDiacritics(body[j]))) break
      parts.push(body[j])
    }
    return { present: true, blockText: parts.join('\n') }
  }
  return { present: false, blockText: '' }
}

function categoryTokens(rawValue) {
  return new Set(
    stripDiacritics(rawValue)
      .toLowerCase()
      .split(/[\s/+(),]+/)
      .filter(Boolean)
  )
}

const REEXEC_HINT_RE =
  /\b(go (test|build|vet)|cargo (test|build)|golangci-lint|docker( compose)?|make |yarn |npm |pnpm |node |curl |psql |gh run|rg |\.\/scripts\/)|\S+\.(go|mjs|ts|sh|sql|rs|ps1)\b/

function hasLabel(body, ...labels) {
  return labels.some((l) => getLabelBlock(body, l).present)
}

function firstLabel(body, ...labels) {
  for (const l of labels) {
    const b = getLabelBlock(body, l)
    if (b.present) return b
  }
  return { present: false, blockText: '' }
}

function hasReexecPointer(body) {
  // English canonical + Portuguese legacy aliases
  if (hasLabel(body, 'How to measure', 'Como medir')) return true
  return REEXEC_HINT_RE.test(body.join('\n'))
}

export function validateEntry(entry) {
  const out = []
  const line = entry.headerLine
  const add = (rule, message) => out.push({ line, rule, message })

  if (!ENTRY_HEADER_FULL_RE.test(entry.header)) {
    add(
      'header',
      'malformed entry header (expected `## YYYY-MM-DD HH:MM — <title>`)'
    )
  }

  // Canonical English labels (validation.md schema). PT aliases accepted for legacy.
  if (!hasLabel(entry.body, 'What', 'O que')) {
    add('schema', 'missing `**What:**` (or legacy `**O que:**`)')
  }

  const verd = firstLabel(entry.body, 'Verdict', 'Veredito')
  if (!verd.present) {
    add('schema', 'missing `**Verdict:**` (or legacy `**Veredito:**`)')
  } else if (!VERDICT_RE.test(verd.blockText)) {
    add('schema', '`**Verdict:**` missing a valid emoji (use ✅/🔴/🟡)')
  }

  const cat = firstLabel(entry.body, 'Category', 'Categoria')
  if (cat.present) {
    const tokens = categoryTokens(cat.blockText)
    const isEffect = [...tokens].some((t) => EFFECT_CATEGORIES.has(t))
    if (isEffect && !hasReexecPointer(entry.body)) {
      add(
        'missing-pointer',
        'Effect category (integration/isolation/e2e/ci-gate) requires a re-executable pointer (`**How to measure:**` or a command/script path in the body)'
      )
    }
  }

  const dados = firstLabel(entry.body, 'Measured data', 'Dados medidos')
  if (
    dados.present &&
    !DIGIT_RE.test(dados.blockText) &&
    !/COUNT=|exit|0 match|schema sumiu|verde|passou|passa|OK/i.test(
      dados.blockText
    )
  ) {
    add(
      'adjective-before-number',
      '`**Measured data:**` missing raw numbers or measurable state'
    )
  }

  return out
}

function parseDiff(baseRef) {
  let diff
  try {
    diff = execFileSync('git', ['diff', '--unified=0', baseRef, '--', TARGET], {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    })
  } catch {
    return { added: new Set(), removedInEntries: [] }
  }
  let baseFirstEntry = Infinity
  try {
    const baseFile = execFileSync('git', ['show', `${baseRef}:${TARGET}`], {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    })
    baseFirstEntry = findFirstEntryLine(baseFile.split('\n'))
  } catch {
    baseFirstEntry = 1
  }

  const added = new Set()
  const removedInEntries = []
  let newLine = 0
  let oldLine = 0
  for (const ln of diff.split('\n')) {
    const h = ln.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
    if (h) {
      oldLine = parseInt(h[1], 10)
      newLine = parseInt(h[2], 10)
      continue
    }
    if (ln.startsWith('+') && !ln.startsWith('+++')) {
      added.add(newLine)
      newLine++
    } else if (ln.startsWith('-') && !ln.startsWith('---')) {
      if (oldLine >= baseFirstEntry) removedInEntries.push(oldLine)
      oldLine++
    }
  }
  return { added, removedInEntries }
}

export function run({ root = ROOT, baseRef = null, all = false } = {}) {
  const file = path.join(root, TARGET)

  if (!existsSync(file)) {
    return {
      ok: false,
      violations: [
        { line: 0, rule: 'sentinel', message: `${TARGET} does not exist` },
      ],
    }
  }
  const content = readFileSync(file, 'utf8')
  const lines = content.split('\n')
  const firstEntryLine = findFirstEntryLine(lines)
  const entries = parseEntries(lines, firstEntryLine)
  if (entries.length === 0) {
    return {
      ok: false,
      violations: [
        {
          line: 0,
          rule: 'sentinel',
          message: `${TARGET} has no parseable entries`,
        },
      ],
    }
  }

  const violations = []
  const allowed = loadAllowlist(root)
  const isAllowed = (e) => allowed.has(entryTimestamp(e.header))

  if (all) {
    for (const e of entries) {
      if (!isAllowed(e)) violations.push(...validateEntry(e))
    }
    return { ok: violations.length === 0, violations }
  }

  const { added, removedInEntries } = parseDiff(baseRef)
  for (const oldLine of removedInEntries) {
    violations.push({
      line: oldLine,
      rule: 'append-only-violation',
      message:
        'removal or modification of lines inside the entries region (log is append-only)',
    })
  }
  for (const e of entries) {
    if (added.has(e.headerLine) && !isAllowed(e))
      violations.push(...validateEntry(e))
  }
  return { ok: violations.length === 0, violations }
}

function main() {
  const argv = process.argv.slice(2)
  const all = argv.includes('--all')
  let baseRef = null
  const di = argv.indexOf('--diff')
  if (di !== -1 && argv[di + 1]) baseRef = argv[di + 1]
  if (!all && !baseRef) {
    process.stderr.write(
      'usage: check-validation-schema.mjs (--all | --diff <baseRef>)\n'
    )
    process.exit(2)
  }

  const { ok, violations } = run({ baseRef, all })
  if (ok) {
    process.stdout.write(
      `✓ ${TARGET} schema OK (${all ? 'all entries' : `diff vs ${baseRef}`})\n`
    )
    process.exit(0)
  }
  for (const v of violations) {
    process.stdout.write(`${TARGET}:${v.line} — ${v.rule}: ${v.message}\n`)
  }
  process.stderr.write(
    `\n${violations.length} schema violation(s) in ${TARGET}\n`
  )
  process.exit(1)
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main()
}
