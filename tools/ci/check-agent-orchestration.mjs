#!/usr/bin/env node
/**
 * Validates the repository-local agent orchestration contract. Authority is
 * taken only from rendered CommonMark prose and canonical YAML record fences.
 */
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const RULE_RELATIVE = '.claude/rules/agent-orchestration.md'
const POINTER_TARGET = '.claude/rules/agent-orchestration.md'
export const REQUIRED_POINTER = `- Agent orchestration and dispatch: [\`${POINTER_TARGET}\`](${POINTER_TARGET}).`
const POINTER_REPRESENTATION = '- Its rendered policy and canonical typed records are the machine-checked source.'
const MARKER = '<!-- agent-orchestration-schema-v1 -->'
const ROUTES = ['R0', 'R1', 'R2', 'R3', 'R4']
const TIERS = new Set(['low', 'medium', 'high', 'xhigh', 'max', 'ultra'])
const APPROVALS = new Set(['current-user-request', 'fresh-explicit-approval', 'none'])
const HANDOFF_STATUSES = new Set(['GREEN', 'PARTIAL', 'BLOCKED', 'NO-GO'])
const TEST_RESULTS = new Set(['PASS', 'FAIL', 'SKIP'])
const ROUTE_MODEL_TIERS = new Map([
  ['R0', new Map([['gpt-5.6-luna', new Set(['low'])]])],
  ['R1', new Map([['gpt-5.6-luna', new Set(['medium'])]])],
  ['R2', new Map([['gpt-5.6-luna', new Set(['high', 'max'])]])],
  ['R3', new Map([['gpt-5.6-terra', new Set(['low', 'medium', 'high', 'xhigh', 'max'])]])],
  ['R4', new Map([
    ['gpt-5.6-sol', new Set(['low', 'medium', 'high', 'xhigh', 'max'])],
    ['gpt-5.6-terra', new Set(['low', 'medium', 'high', 'xhigh', 'max'])],
  ])],
])
export const REQUIRED_MODEL_TIERS = [
  ['gpt-5.6-luna', 'low'],
  ['gpt-5.6-luna', 'medium'],
  ['gpt-5.6-luna', 'high'],
  ['gpt-5.6-luna', 'max'],
  ['gpt-5.6-luna', 'ultra'],
  ['gpt-5.6-terra', 'low'],
  ['gpt-5.6-terra', 'medium'],
  ['gpt-5.6-terra', 'high'],
  ['gpt-5.6-terra', 'xhigh'],
  ['gpt-5.6-terra', 'max'],
  ['gpt-5.6-sol', 'low'],
  ['gpt-5.6-sol', 'medium'],
  ['gpt-5.6-sol', 'high'],
  ['gpt-5.6-sol', 'xhigh'],
  ['gpt-5.6-sol', 'max'],
]
const HEADINGS = [
  'Checker-visible representation',
  'Checker-visible safety invariants',
  'R0–R4 routing',
  'Luna/Terra/Sol tier matrix',
  'Dispatch card',
  'Ownership, fork, and context rules',
  'Current approvals',
  'Mandatory typed handoff',
  'Two independent Sol gates',
]
const REQUIRED_INVARIANTS = [
  'Root Sol is read-only and must not edit, self-approve, commit, push, merge, or run host or destructive actions.',
  'A worker must not spawn agents or workers.',
  'Every approval is explicit, current, and scoped; a stale or inherited approval is invalid.',
  'The two Sol gates require separate independent verdicts; one Sol verdict cannot satisfy both gates.',
]
const DISPATCH_KEYS = [
  'schema', 'dispatch_id', 'route', 'model', 'tier', 'objective', 'owner', 'parent',
  'scope', 'read_only', 'approval', 'inputs', 'outputs', 'tests', 'coverage', 'rollback_trigger',
]
const HANDOFF_KEYS = [
  'schema', 'dispatch_id', 'route', 'model', 'tier', 'owner', 'status', 'changed_files',
  'tests', 'metrics', 'gates', 'residuals', 'next_action',
]

function finding(rule, message, value = undefined) {
  return value === undefined ? { rule, message } : { rule, value, message }
}

function normalize(value) {
  return String(value).replace(/\s+/gu, ' ').trim()
}

function indentColumns(line) {
  let columns = 0
  for (const character of line) {
    if (character === ' ') columns += 1
    else if (character === '\t') columns += 4 - (columns % 4)
    else break
  }
  return columns
}

function removeHtmlComments(line, inComment) {
  let output = ''
  let index = 0
  let open = inComment
  while (index < line.length) {
    if (open) {
      const end = line.indexOf('-->', index)
      if (end < 0) return { text: output, inComment: true }
      index = end + 3
      open = false
      continue
    }
    const start = line.indexOf('<!--', index)
    if (start < 0) {
      output += line.slice(index)
      break
    }
    output += line.slice(index, start)
    index = start + 4
    open = true
  }
  return { text: output, inComment: open }
}

function fenceOpener(line) {
  const match = line.match(/^( {0,3})(`{3,}|~{3,})(.*)$/u)
  if (!match) return null
  const marker = match[2]
  const info = match[3].trim()
  if (marker[0] === '`' && info.includes('`')) return null
  return { marker: marker[0], length: marker.length, info }
}

function fenceCloser(line, fence) {
  const match = line.match(/^ {0,3}(`+|~+)[ \t]*$/u)
  return Boolean(match && match[1][0] === fence.marker && match[1].length >= fence.length)
}

function rawHtmlOpen(line) {
  const match = line.match(/^ {0,3}<(pre|script|style|textarea)(?=[\t\f\r />]|$)/iu)
  return match ? { tag: match[1].toLowerCase(), end: match[0].length } : null
}

function rawHtmlClose(line, tag) {
  return new RegExp(`</${tag}>`, 'iu').test(line)
}

/**
 * Extracts only CommonMark-rendered prose and complete fenced code blocks.
 * It intentionally supports the narrow constructs used by this contract and
 * fails closed by treating unfinished hidden blocks as hidden until EOF.
 */
export function extractCommonMark(text) {
  const prose = []
  const fences = []
  if (typeof text !== 'string') return { prose, fences }
  let comment = false
  let rawTag = null
  let fence = null
  for (const sourceLine of text.split(/\r?\n/u)) {
    if (fence) {
      if (fenceCloser(sourceLine, fence)) {
        fences.push({ info: fence.info, content: fence.lines.join('\n') })
        fence = null
      } else {
        fence.lines.push(sourceLine)
      }
      continue
    }
    if (rawTag) {
      if (rawHtmlClose(sourceLine, rawTag)) rawTag = null
      continue
    }
    const withoutComments = removeHtmlComments(sourceLine, comment)
    comment = withoutComments.inComment
    const line = withoutComments.text
    if (indentColumns(line) >= 4) continue
    const opener = fenceOpener(line)
    if (opener) {
      fence = { ...opener, lines: [] }
      continue
    }
    const raw = rawHtmlOpen(line)
    if (raw) {
      if (!rawHtmlClose(line.slice(raw.end), raw.tag)) rawTag = raw.tag
      continue
    }
    prose.push(line)
  }
  return { prose, fences }
}

function section(text, heading) {
  const lines = text.split(/\r?\n/u)
  const start = lines.findIndex((line) => line.trim().toLowerCase() === `## ${heading}`.toLowerCase())
  if (start < 0) return ''
  const end = lines.slice(start + 1).findIndex((line) => /^##\s+/u.test(line))
  const stop = end < 0 ? lines.length : start + 1 + end
  return lines.slice(start + 1, stop).join('\n')
}

function hasHeading(text, heading) {
  return new RegExp(`^##\\s+${heading.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*$`, 'imu').test(text)
}

function hasRoute(text, route) {
  return new RegExp(`^\\|\\s*${route}\\s*\\|`, 'mu').test(text)
}

function routeLine(text, route) {
  return text.split(/\r?\n/u).find((line) => new RegExp(`^\\|\\s*${route}\\s*\\|`, 'u').test(line)) ?? ''
}

function validateCostFirstRouting(text, findings) {
  const required = [
    ['R0', ['read-only', 'deterministic', 'gpt-5.6-luna / low', 'root sol', 'orchestration-only']],
    ['R1', ['small closed mutation', 'gpt-5.6-luna / medium']],
    ['R2', ['multi-file', 'known contract', 'gpt-5.6-luna / high or max']],
    ['R3', ['structural', 'security', 'concurrency', 'kernel', 'driver', 'host', 'gpt-5.6-terra']],
    ['R4', ['critical', 'release', 'final audit', 'gpt-5.6-sol', 'gpt-5.6-terra']],
  ]
  for (const [route, terms] of required) {
    const line = routeLine(text, route).toLowerCase()
    if (!line || terms.some((term) => !line.includes(term))) {
      findings.push(finding('routing-cost-first', `cost-first routing contract is incomplete for ${route}`, route))
    }
  }
}

function splitTopLevel(value, delimiter = ',') {
  const parts = []
  let start = 0
  let quote = ''
  let depth = 0
  for (let index = 0; index < value.length; index++) {
    const character = value[index]
    if (quote) {
      if (character === quote && value[index - 1] !== '\\') quote = ''
      continue
    }
    if (character === '"' || character === "'") quote = character
    else if (character === '[' || character === '{') depth += 1
    else if (character === ']' || character === '}') {
      depth -= 1
      if (depth < 0) return null
    } else if (character === delimiter && depth === 0) {
      parts.push(value.slice(start, index).trim())
      start = index + 1
    }
  }
  if (quote || depth !== 0) return null
  parts.push(value.slice(start).trim())
  return parts
}

function topLevelColon(value) {
  const parts = splitTopLevel(value, ':')
  if (!parts || parts.length !== 2 || !parts[0]) return null
  return parts
}

function parseInlineValue(value) {
  const trimmed = value.trim()
  if (!trimmed) return { ok: false }
  if ((trimmed.startsWith('[') && !trimmed.endsWith(']')) || (trimmed.startsWith('{') && !trimmed.endsWith('}'))) return { ok: false }
  if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
    const body = trimmed.slice(1, -1).trim()
    if (!body) return { ok: true, value: [] }
    const items = splitTopLevel(body)
    if (!items || items.some((item) => !item)) return { ok: false }
    const values = items.map(parseInlineValue)
    return values.every((item) => item.ok) ? { ok: true, value: values.map((item) => item.value) } : { ok: false }
  }
  if (trimmed.startsWith('{') && trimmed.endsWith('}')) {
    const body = trimmed.slice(1, -1).trim()
    if (!body) return { ok: true, value: {} }
    const entries = splitTopLevel(body)
    if (!entries || entries.some((item) => !item)) return { ok: false }
    const object = {}
    for (const entry of entries) {
      const pair = topLevelColon(entry)
      if (!pair || !/^[A-Za-z][A-Za-z0-9_]*$/u.test(pair[0]) || Object.hasOwn(object, pair[0])) return { ok: false }
      const parsed = parseInlineValue(pair[1])
      if (!parsed.ok) return { ok: false }
      object[pair[0]] = parsed.value
    }
    return { ok: true, value: object }
  }
  if ((trimmed.startsWith('"') || trimmed.startsWith("'")) && trimmed.length >= 2 && trimmed.at(-1) === trimmed[0]) {
    return { ok: true, value: trimmed.slice(1, -1) }
  }
  if (trimmed === 'true') return { ok: true, value: true }
  if (trimmed === 'false') return { ok: true, value: false }
  if (/^-?\d+$/u.test(trimmed)) return { ok: true, value: Number.parseInt(trimmed, 10) }
  if (/^[\[\]{}]/u.test(trimmed)) return { ok: false }
  return { ok: true, value: trimmed }
}

function parseYamlRecord(content) {
  const record = {}
  let nested = null
  for (const sourceLine of content.split(/\r?\n/u)) {
    if (sourceLine.trim() === '') continue
    const top = sourceLine.match(/^([A-Za-z][A-Za-z0-9_]*):(?:\s+(.*)|\s*)$/u)
    if (top) {
      const key = top[1]
      if (Object.hasOwn(record, key)) return { ok: false }
      if (top[2] === undefined || top[2] === '') {
        record[key] = {}
        nested = key
      } else {
        const parsed = parseInlineValue(top[2])
        if (!parsed.ok) return { ok: false }
        record[key] = parsed.value
        nested = null
      }
      continue
    }
    const child = sourceLine.match(/^  ([A-Za-z][A-Za-z0-9_]*):\s+(.+)$/u)
    if (!child || !nested || !record[nested] || typeof record[nested] !== 'object' || Array.isArray(record[nested]) || Object.hasOwn(record[nested], child[1])) {
      return { ok: false }
    }
    const parsed = parseInlineValue(child[2])
    if (!parsed.ok) return { ok: false }
    record[nested][child[1]] = parsed.value
  }
  return { ok: true, value: record }
}

function hasExactKeys(value, expected) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const actual = Object.keys(value).sort()
  return actual.length === expected.length && actual.every((key, index) => key === [...expected].sort()[index])
}

function safeRelativePath(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/u.test(value) && !value.split(/[\\/]/u).includes('..') &&
    !/[?*{}\[\]]/u.test(value) && !value.endsWith('/') && !['.', '..', '*', '**'].includes(value)
}

function isBoundedPathList(value) {
  return Array.isArray(value) && value.length > 0 && new Set(value).size === value.length && value.every(safeRelativePath)
}

function isAgentIdentifier(value) {
  return typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9._/-]{2,127}$/u.test(value) && !value.includes('..')
}

function validRouteModelTier(record) {
  if (!ROUTES.includes(record.route) || !TIERS.has(record.tier)) return false
  return ROUTE_MODEL_TIERS.get(record.route)?.get(record.model)?.has(record.tier) === true
}

function coverageFloors(value) {
  if (typeof value !== 'string') return null
  const floors = {}
  for (const metric of ['lines', 'branches', 'functions']) {
    const match = value.match(new RegExp(`\\b${metric}\\s*>=\\s*(\\d+)\\b`, 'u'))
    if (!match) return null
    floors[metric] = Number.parseInt(match[1], 10)
  }
  return floors
}

function commandList(value) {
  return Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === 'string' && item.trim() !== '')
}

function boundedTestCommand(value) {
  return typeof value === 'string' && value.trim() !== '' && value.length <= 512 && !/[\r\n]/u.test(value) &&
    !/(?:^|[;&|]\s*)(?:while\s+(?:true|:)|until\s+false|for\s*\(\(\s*;\s*;\s*\)|yes)(?:\s|$)/iu.test(value)
}

function testCommandList(value) {
  return Array.isArray(value) && value.length > 0 && new Set(value).size === value.length && value.every(boundedTestCommand)
}

function validateDispatchRecord(record) {
  if (!hasExactKeys(record, DISPATCH_KEYS) || record.schema !== 'ramshared.dispatch.v1' || !isAgentIdentifier(record.dispatch_id) ||
      !validRouteModelTier(record) || typeof record.objective !== 'string' || !record.objective.trim() ||
      !isAgentIdentifier(record.owner) || !isAgentIdentifier(record.parent) || !/^root(?:[-_.]|$)/iu.test(record.parent) ||
      record.owner === record.parent || (!record.read_only && /^root(?:[-_.]|$)/iu.test(record.owner)) || !record.scope || !hasExactKeys(record.scope, ['include', 'exclude']) ||
      !isBoundedPathList(record.scope.include) || !Array.isArray(record.scope.exclude) || new Set(record.scope.exclude).size !== record.scope.exclude.length || !record.scope.exclude.every(safeRelativePath) ||
      typeof record.read_only !== 'boolean' || !APPROVALS.has(record.approval) || (!record.read_only && record.approval === 'none') ||
      !commandList(record.inputs) || !commandList(record.outputs) || !record.outputs.includes('ramshared.handoff.v1') ||
      !testCommandList(record.tests) || !coverageFloors(record.coverage) || Object.values(coverageFloors(record.coverage)).some((floor) => floor < 80) ||
      typeof record.rollback_trigger !== 'string' || !record.rollback_trigger.trim() || record.rollback_trigger === 'none') {
    return false
  }
  return true
}

function validateHandoffRecord(record) {
  if (!hasExactKeys(record, HANDOFF_KEYS) || record.schema !== 'ramshared.handoff.v1' || !isAgentIdentifier(record.dispatch_id) ||
      !validRouteModelTier(record) || !isAgentIdentifier(record.owner) || !HANDOFF_STATUSES.has(record.status) ||
      !Array.isArray(record.changed_files) || !record.changed_files.every(safeRelativePath) || !Array.isArray(record.tests) ||
      record.tests.length === 0 || !record.tests.every((item) => hasExactKeys(item, ['command', 'result']) &&
        boundedTestCommand(item.command) && TEST_RESULTS.has(item.result)) ||
      new Set(record.tests.map((item) => item.command)).size !== record.tests.length ||
      !hasExactKeys(record.metrics, ['lines', 'branches', 'functions']) ||
      !Object.values(record.metrics).every((value) => Number.isInteger(value) && value >= 0) ||
      !commandList(record.gates) || !commandList(record.residuals) || typeof record.next_action !== 'string' || !record.next_action.trim()) {
    return false
  }
  if (record.status === 'GREEN' && (!record.tests.every((item) => item.result === 'PASS') ||
      Object.values(record.metrics).some((value) => value < 80) || !record.residuals.every((item) => item === 'none') || record.next_action !== 'none')) {
    return false
  }
  return record.status === 'GREEN' || record.residuals.every((item) => item !== 'none')
}

function pathInScope(file, scope) {
  return scope.includes(file)
}

function reconcileRecords(dispatch, handoff) {
  const floors = coverageFloors(dispatch.coverage)
  if (dispatch.dispatch_id !== handoff.dispatch_id || dispatch.route !== handoff.route || dispatch.model !== handoff.model ||
      dispatch.tier !== handoff.tier || dispatch.owner !== handoff.owner ||
      (dispatch.read_only && handoff.changed_files.length > 0) ||
      !handoff.changed_files.every((file) => pathInScope(file, dispatch.scope.include)) ||
      !handoff.tests.every((test) => dispatch.tests.includes(test.command)) ||
      !dispatch.tests.every((command) => handoff.tests.some((test) => test.command === command)) ||
      Object.entries(floors).some(([metric, floor]) => handoff.metrics[metric] < floor)) {
    return false
  }
  return handoff.status !== 'GREEN' || handoff.tests.every((test) => test.result === 'PASS')
}

function validateTypedRecords(fences, findings) {
  const typed = []
  for (const fence of fences) {
    if (!/^(yaml|yml)$/iu.test(fence.info)) continue
    const parsed = parseYamlRecord(fence.content)
    if (!parsed.ok) {
      if (/\bschema\s*:\s*ramshared\.(?:dispatch|handoff)/u.test(fence.content)) findings.push(finding('typed-record-invalid', 'canonical YAML record is malformed'))
      continue
    }
    const schema = parsed.value.schema
    if (schema === 'ramshared.dispatch.v1') typed.push({ kind: 'dispatch', record: parsed.value })
    else if (schema === 'ramshared.handoff.v1') typed.push({ kind: 'handoff', record: parsed.value })
    else if (typeof schema === 'string' && schema.startsWith('ramshared.dispatch')) findings.push(finding('dispatch-record-invalid', 'dispatch schema must be ramshared.dispatch.v1'))
    else if (typeof schema === 'string' && schema.startsWith('ramshared.handoff')) findings.push(finding('handoff-record-invalid', 'handoff schema must be ramshared.handoff.v1'))
  }
  const dispatches = typed.filter((item) => item.kind === 'dispatch')
  const handoffs = typed.filter((item) => item.kind === 'handoff')
  if (dispatches.length !== 1 || handoffs.length !== 1) {
    findings.push(finding('typed-record-count', 'exactly one dispatch and one handoff record are required'))
    return
  }
  if (!validateDispatchRecord(dispatches[0].record)) findings.push(finding('dispatch-record-invalid', 'dispatch record violates bounded card semantics'))
  if (!validateHandoffRecord(handoffs[0].record)) findings.push(finding('handoff-record-invalid', 'handoff record violates typed result semantics'))
  if (validateDispatchRecord(dispatches[0].record) && validateHandoffRecord(handoffs[0].record) && !reconcileRecords(dispatches[0].record, handoffs[0].record)) {
    findings.push(finding('handoff-reconciliation', 'handoff does not reconcile with its dispatch card'))
  }
}

function contradictionFindings(text, findings) {
  const sentences = normalize(text).split(/[.!?]+/u)
  const grant = '(?:may|can|will|is\\s+allowed\\s+to|is\\s+permitted\\s+to|has\\s+authority\\s+to)'
  const positiveGrant = (sentence, subject, action) => {
    const expression = new RegExp(`${subject}[\\s\\S]{0,120}?\\b(${grant})\\b[\\s\\S]{0,80}?\\b(?:${action})\\b`, 'giu')
    return [...sentence.matchAll(expression)].some((match) => !/\b(?:not|never|no longer|cannot|can't|must not|do not|does not|did not)\b/iu.test(match[0]))
  }
  const rootAction = '(?:edit|self[- ]?approve|approve (?:itself|its own|an? own)|commit|push|merge|(?:run|perform|take|reboot|access|use|control|operate) (?:the )?(?:host(?:[- ]bound)?|destructive)(?: actions?| commands?| operations?)?)'
  if (sentences.some((sentence) => positiveGrant(sentence, '\\bRoot Sol\\b', rootAction))) {
    findings.push(finding('root-sol-authority-grant', 'rendered prose grants Root Sol a prohibited action'))
  }
  const worker = '(?:\\b(?:a|an|the)\\s+)?\\bworkers?\\b'
  if (sentences.some((sentence) => positiveGrant(sentence, worker, '(?:spawn|dispatch|create|fork)\\b[\\s\\S]{0,80}?\\b(?:agents?|workers?)\\b'))) {
    findings.push(finding('worker-spawn-grant', 'rendered prose grants a worker spawn authority'))
  }
  const staleGrant = (sentence) => positiveGrant(sentence, worker, '(?:inherit|reuse|use)\\b[\\s\\S]{0,80}?\\b(?:stale|inherited) approvals?\\b') ||
    positiveGrant(sentence, '\\b(?:stale|inherited) approvals?\\b', '(?:be )?(?:used|valid|reused)\\b')
  if (sentences.some(staleGrant)) findings.push(finding('stale-approval-grant', 'rendered prose grants stale or inherited approval'))
  if (sentences.some((sentence) => positiveGrant(sentence, '\\b(?:one|a single|single) Sol (?:result|verdict)\\b', '(?:satisfy|cover|pass)\\b[\\s\\S]{0,100}?\\bboth gates\\b'))) {
    findings.push(finding('sol-gates-reused', 'rendered prose allows one Sol result to satisfy both gates'))
  }
}

function unsafeAuthorityFindings(text, findings) {
  const sentences = normalize(text).split(/[.!?]+/u)
  const grant = '(?:may|can|will|is\\s+allowed\\s+to|is\\s+permitted\\s+to|has\\s+authority\\s+to)'
  const subject = '\\b(?:Root Sol|(?:any|a|the)\\s+worker|workers?)\\b'
  const action = '(?:delete\\s+(?:repository|repo)(?:\\s+files?)?|format(?:\\s+or\\s+wipe)?\\s+(?:the\\s+)?disk|wipe\\s+(?:the\\s+)?disk|(?:stop|disable)(?:\\s+or\\s+(?:stop|disable))?\\s+(?:a\\s+)?(?:host\\s+)?service|run\\s+(?:swapon|swapoff)(?:\\s+or\\s+(?:swapon|swapoff))?|(?:destroy|delete)(?:\\s+or\\s+(?:destroy|delete))?\\s+(?:a\\s+)?device|kill\\s+(?:host\\s+)?process(?:es)?|mutate\\s+(?:host(?:,\\s*device(?:,\\s*or\\s+destructive)?)?|device(?:,\\s*or\\s+destructive)?|destructive)\\s+surfaces?)'
  const expression = new RegExp(`${subject}[\\s\\S]{0,120}?\\b(${grant})\\b[\\s\\S]{0,120}?\\b${action}\\b`, 'giu')
  const denial = new RegExp(`${subject}[\\s\\S]{0,120}?\\b(?:not|never|no longer|cannot|can't|must not|do not|does not|did not)\\b[\\s\\S]{0,120}?\\b${action}\\b`, 'iu')
  if (sentences.some((sentence) => [...sentence.matchAll(expression)].some((match) => !denial.test(match[0])))) {
    findings.push(finding('unsafe-authority-grant', 'rendered prose grants Root Sol or a worker an unsafe host, device, or destructive action'))
  }
}

export function validateDocument(text) {
  const findings = []
  if (typeof text !== 'string' || text.trim() === '') return [finding('empty-document', 'orchestration rule is empty')]
  if (!text.includes(MARKER)) findings.push(finding('schema-marker', `missing ${MARKER}`))
  const rendered = extractCommonMark(text)
  const prose = rendered.prose.join('\n')
  for (const heading of HEADINGS) if (!hasHeading(prose, heading)) findings.push(finding('heading-missing', `missing ${heading} heading`))
  for (const invariant of REQUIRED_INVARIANTS) {
    if (!normalize(prose).includes(normalize(invariant))) findings.push(finding('rendered-invariant-missing', `missing rendered invariant: ${invariant}`))
  }
  contradictionFindings(prose, findings)
  unsafeAuthorityFindings(prose, findings)

  for (const route of ROUTES) if (!hasRoute(prose, route)) findings.push(finding('route-missing', `routing table is missing ${route}`, route))
  if (!/Root Sol[\s\S]{0,180}(?:orchestration-only|only orchestrates)[\s\S]{0,180}read-only/iu.test(prose)) {
    findings.push(finding('root-sol-boundary', 'Root Sol must be orchestration and read-only'))
  }
  const matrix = section(prose, 'Luna/Terra/Sol tier matrix')
  for (const [model, tier] of REQUIRED_MODEL_TIERS) {
    const row = new RegExp(`^\\|\\s*${model.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*\\|\\s*${tier}\\s*\\|`, 'mu')
    if (!row.test(matrix)) findings.push(finding('model-tier-row-missing', `model/tier matrix is missing ${model}/${tier}`, `${model}/${tier}`))
  }
  validateCostFirstRouting(prose, findings)

  if (!hasHeading(prose, 'Dispatch card')) findings.push(finding('heading-missing', 'missing Dispatch card heading'))
  if (!hasHeading(prose, 'Mandatory typed handoff')) findings.push(finding('heading-missing', 'missing Mandatory typed handoff heading'))
  validateTypedRecords(rendered.fences, findings)

  const ownership = section(prose, 'Ownership, fork, and context rules')
  for (const term of ['owner', 'fork', 'context']) if (!new RegExp(`\\b${term}\\b`, 'iu').test(ownership)) {
    findings.push(finding('ownership-rules', `ownership section is missing ${term}`))
  }
  if (!/workers?[\s\S]{0,180}do not spawn agents/iu.test(ownership)) findings.push(finding('worker-spawn', 'workers do not spawn agents'))

  const approvals = section(prose, 'Current approvals')
  for (const term of ['current', 'explicit', 'stale']) if (!new RegExp(`\\b${term}\\b`, 'iu').test(approvals)) {
    findings.push(finding('approval-rules', `approval section is missing ${term}`))
  }
  const gates = section(prose, 'Two independent Sol gates')
  if (!gates.includes('SOL-GATE-PRE-COMMIT') || !gates.includes('SOL-GATE-PRE-PR') || (gates.match(/independent/giu) ?? []).length < 2) {
    findings.push(finding('sol-gates', 'two independent Sol gates must cover SOL-GATE-PRE-COMMIT and SOL-GATE-PRE-PR'))
  }
  if (!/one Sol (?:result|verdict)[\s\S]{0,100}cannot satisfy both gates/iu.test(prose)) {
    findings.push(finding('sol-gates-independent', 'the two Sol gate results must remain independent'))
  }
  return findings
}

function pointerFindings(text, label) {
  const findings = []
  if (typeof text !== 'string' || text.trim() === '') return [finding('pointer-missing', `${label} is empty`)]
  const lines = extractCommonMark(text).prose.map((line) => line.trim()).filter(Boolean)
  const count = lines.filter((line) => line === REQUIRED_POINTER).length
  if (count === 0) findings.push(finding('pointer-missing', `${label} is missing the canonical pointer`))
  if (count !== 1) findings.push(finding('pointer-count', `${label} must contain exactly one canonical pointer`))
  if (lines.filter((line) => line === POINTER_REPRESENTATION).length !== 1) {
    findings.push(finding('pointer-representation', `${label} must declare the rendered typed-record source`))
  }
  const related = lines.filter((line) => /agent orchestration|agent-orchestration/iu.test(line))
  if (related.some((line) => line !== REQUIRED_POINTER)) findings.push(finding('pointer-not-concise', `${label} contains orchestration text beyond its pointer`))
  return findings
}

export function validatePointers(agentsText, claudeText) {
  const findings = [
    ...pointerFindings(agentsText, 'AGENTS.md'),
    ...pointerFindings(claudeText, 'CLAUDE.md'),
  ]
  if (typeof agentsText === 'string' && typeof claudeText === 'string') {
    const agentsLines = extractCommonMark(agentsText).prose.map((line) => line.trim()).filter(Boolean)
    const claudeLines = extractCommonMark(claudeText).prose.map((line) => line.trim()).filter(Boolean)
    if (agentsLines.filter((line) => line === REQUIRED_POINTER).length !== claudeLines.filter((line) => line === REQUIRED_POINTER).length) {
      findings.push(finding('pointer-sync', 'AGENTS.md and CLAUDE.md pointers are not synchronized'))
    }
  }
  return findings
}

function readFile(root, relative, findings) {
  const full = path.join(root, relative)
  if (!existsSync(full)) {
    findings.push(finding('file-missing', `${relative} does not exist`))
    return ''
  }
  try {
    return readFileSync(full, 'utf8')
  } catch (error) {
    findings.push(finding('file-read', `${relative} could not be read: ${error.message}`))
    return ''
  }
}

export function run({ root = ROOT } = {}) {
  const findings = []
  const rule = readFile(root, RULE_RELATIVE, findings)
  const agents = readFile(root, 'AGENTS.md', findings)
  const claude = readFile(root, 'CLAUDE.md', findings)
  if (rule) findings.push(...validateDocument(rule))
  findings.push(...validatePointers(agents, claude))
  findings.sort((left, right) => left.rule.localeCompare(right.rule) || left.message.localeCompare(right.message))
  return {
    ok: findings.length === 0,
    findings,
    counts: { files: 3, findings: findings.length },
  }
}

export function main(argv = process.argv.slice(2), { root = ROOT, print = console.log, error = console.error } = {}) {
  if (argv.length !== 1 || argv[0] !== '--check') {
    error('usage: check-agent-orchestration.mjs --check')
    return 2
  }
  const result = run({ root })
  print(`AGENT_ORCHESTRATION_FILES=${result.counts.files}`)
  print(`AGENT_ORCHESTRATION_FINDINGS=${result.counts.findings}`)
  for (const item of result.findings) error(`${item.rule}: ${item.message}`)
  print(`AGENT_ORCHESTRATION_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
