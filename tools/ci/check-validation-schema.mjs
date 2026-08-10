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

  if (hasGovernanceSchema(entry.body)) {
    out.push(...validateGovernanceEntry(entry))
  }

  return out
}

export function hasGovernanceSchema(body) {
  const marker = firstLabel(body, 'Governance schema')
  return marker.present && /^\s*1\s*$/.test(marker.blockText)
}

export function validateGovernanceEntry(entry) {
  const out = []
  const required = [
    'Slug', 'Environment/commit', 'Scope', 'Before', 'Action', 'After',
    'Legitimate case', 'Required refusals', 'Tests/coverage', 'Platform gates',
    'Artifacts', 'Cleanup', 'Limitations', 'Rollback trigger',
  ]
  for (const label of required) {
    const value = getLabelBlock(entry.body, label)
    if (!value.present || !value.blockText.trim()) {
      out.push({ line: entry.headerLine, rule: 'governance-schema', message: `missing or empty \`**${label}:**\`` })
    }
  }
  const refusals = getLabelBlock(entry.body, 'Required refusals')
  if (refusals.present) {
    const named = refusals.blockText.split(/\r?\n|;/).map((item) => item.trim()).filter(Boolean)
    if (named.length < 2) out.push({ line: entry.headerLine, rule: 'governance-schema', message: '`**Required refusals:**` needs at least two named cases' })
  }
  const rollback = getLabelBlock(entry.body, 'Rollback trigger')
  if (rollback.present && !/\d|one|zero|missing|mismatch|non-zero|timeout/i.test(rollback.blockText)) {
    out.push({ line: entry.headerLine, rule: 'governance-schema', message: '`**Rollback trigger:**` must be numeric or observable' })
  }
  return out
}

export function isSecurityRedaction(oldLine, newLine) {
  const signingSecret =
    /-PfxPassword\s+["'][^"']+["']/.test(oldLine) &&
    /-PfxPassword\s+\$env:[A-Z0-9_]+/.test(newLine)
  const historicalCredential =
    /\b(password|credential)\b/i.test(oldLine) &&
    /`[^`]+`/.test(oldLine) &&
    /\b(password|credential)\b/i.test(newLine) &&
    /\bredacted\b/i.test(newLine)
  const environmentCredentialLabel =
    /RAMSHARED_[A-Z0-9_]+/.test(oldLine) &&
    /RAMSHARED_[A-Z0-9_]+/.test(newLine) &&
    oldLine.replace(/password:/i, 'credential source:') === newLine
  if (signingSecret || historicalCredential || environmentCredentialLabel) return true

  const unrelatedName = new RegExp(['ad', 'voq'].join(''), 'gi')
  const normalize = (line) => line
    .replace(/\/home\/[A-Za-z0-9._-]+\/fase0/gi, '<private-artifact-root>')
    .replace(/<legacy-private-artifact-root>/gi, '<private-artifact-root>')
    .replace(/\/home\/[A-Za-z0-9._-]+/gi, '<private-root>')
    .replace(/<legacy-private-root>/gi, '<private-root>')
    .replace(/[A-Za-z]:\\Users\\[^\\\s`]+\\ramshared-drill/gi, '<windows-artifact-root>')
    .replace(/C:\\ramshared\\artifacts/gi, '<windows-artifact-root>')
    .replace(/[A-Za-z]:\\Users\\[^\\\s`]+\\ramshared-src/gi, '<windows-source-root>')
    .replace(/C:\\ramshared\\src/gi, '<windows-source-root>')
    .replace(unrelatedName, 'unrelated workload')
  const normalizedOld = normalize(oldLine)
  const normalizedNew = normalize(newLine)
  const numbers = (line) => line.match(/\d+(?:\.\d+)?/g) ?? []
  const verdicts = (line) => line.match(/[✅🔴🟡]/g) ?? []
  if (JSON.stringify(numbers(normalizedOld)) !== JSON.stringify(numbers(normalizedNew)) ||
      JSON.stringify(verdicts(normalizedOld)) !== JSON.stringify(verdicts(normalizedNew))) {
    return false
  }
  return normalizedOld !== oldLine && normalizedOld === normalizedNew
}

export function parseDiff(baseRef, root = ROOT) {
  let diff
  try {
    diff = execFileSync('git', ['diff', '--unified=0', baseRef, '--', TARGET], {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    })
  } catch {
    return { added: new Set(), removedInEntries: [], error: 'git-diff-failed' }
  }
  let baseFirstEntry = Infinity
  try {
    const baseFile = execFileSync('git', ['show', `${baseRef}:${TARGET}`], {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    })
    baseFirstEntry = findFirstEntryLine(baseFile.split('\n'))
  } catch {
    baseFirstEntry = 1
  }

  const added = new Set()
  const removedInEntries = []
  let hunkRemoved = []
  let hunkAdded = []
  const flushHunk = () => {
    if (hunkRemoved.length === hunkAdded.length) {
      for (let i = 0; i < hunkRemoved.length; i++) {
        if (isSecurityRedaction(hunkRemoved[i].text, hunkAdded[i].text)) {
          added.delete(hunkAdded[i].line)
        } else {
          removedInEntries.push(hunkRemoved[i].line)
        }
      }
    } else {
      removedInEntries.push(...hunkRemoved.map((item) => item.line))
    }
    hunkRemoved = []
    hunkAdded = []
  }
  let newLine = 0
  let oldLine = 0
  for (const ln of diff.split('\n')) {
    const h = ln.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
    if (h) {
      flushHunk()
      oldLine = parseInt(h[1], 10)
      newLine = parseInt(h[2], 10)
      continue
    }
    if (ln.startsWith('+') && !ln.startsWith('+++')) {
      added.add(newLine)
      hunkAdded.push({ line: newLine, text: ln.slice(1) })
      newLine++
    } else if (ln.startsWith('-') && !ln.startsWith('---')) {
      if (oldLine >= baseFirstEntry)
        hunkRemoved.push({ line: oldLine, text: ln.slice(1) })
      oldLine++
    }
  }
  flushHunk()
  return { added, removedInEntries, error: null }
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

  const { added, removedInEntries, error } = parseDiff(baseRef, root)
  if (error) {
    violations.push({ line: 0, rule: 'diff-error', message: 'unable to read the requested Git base revision' })
    return { ok: false, violations }
  }
  for (const oldLine of removedInEntries) {
    violations.push({
      line: oldLine,
      rule: 'append-only-violation',
      message:
        'removal or modification of lines inside the entries region (log is append-only)',
    })
  }
  for (let index = 0; index < entries.length; index++) {
    const e = entries[index]
    const nextLine = index + 1 < entries.length ? entries[index + 1].headerLine : lines.length + 1
    const addedInside = [...added].filter((line) => line > e.headerLine && line < nextLine)
    if (added.has(e.headerLine)) {
      violations.push(...validateEntry(e))
    } else if (addedInside.length > 0 && !isAllowed(e)) {
      const nextEntryIsNew = index + 1 < entries.length && added.has(entries[index + 1].headerLine)
      const separatorOnly = nextEntryIsNew && addedInside.every((line) => lines[line - 1].trim() === '')
      if (separatorOnly) continue
      violations.push({ line: e.headerLine, rule: 'append-only-violation', message: 'added content inside an existing entry; append a new entry instead' })
    }
  }
  return { ok: violations.length === 0, violations }
}

/* node:coverage disable */
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
/* node:coverage enable */
