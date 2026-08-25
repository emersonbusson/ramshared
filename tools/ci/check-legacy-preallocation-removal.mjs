#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { lstatSync, readFileSync, realpathSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ACTIVE_SOURCE_ROOTS = ['crates/', 'drivers/', 'scripts/', 'tools/']
const CURRENT_ROOT_DOCS = new Set([
  'README.md',
  'README.pt-BR.md',
  'ARCHITECTURE.md',
  'docs/FAQ.md',
  'docs/reliability/GAP-REGISTER.md',
  'docs/specs/no-milestone/wsl2-revocable-vram-origin/PRD.md',
  'docs/specs/no-milestone/wsl2-revocable-vram-origin/SPEC.md',
  'docs/specs/no-milestone/wsl2-revocable-vram-origin/IMPL.md',
  'docs/specs/no-milestone/wsl2-control-plane-pressure-incident/IMPL.md',
  'docs/specs/no-milestone/cascade-vram-ondemand/SPEC.md',
  'docs/specs/no-milestone/cascade-vram-ondemand/AUDIT-2.5.md',
])
const HISTORICAL_VALIDATION_FILES = new Set(['MEMORY.md', 'validation.md'])
const HISTORICAL_DESIGN_RECORDS = new Set([
  'docs/historical.md',
  'docs/specs/no-milestone/cascade-vram-ondemand/PRD.md',
  'docs/specs/no-milestone/cascade-vram-ondemand/IMPL.md',
])
const HISTORICAL_DOCUMENT_ROOTS = ['artifacts/', 'docs/historical/', 'docs/reliability/incidents/', 'docs/upstream/']
const CONTROL_OR_AMBIGUOUS_PATH = /[\p{Cc}\p{Cf}\\]/u

const joinUnderscore = (...parts) => parts.join('_')
const joinEmpty = (...parts) => parts.join('')
const LEGACY_ENV_SELECTOR = joinUnderscore('RAMSHARED', 'VRAM', 'PREALLOC', 'LEGACY')
const LEGACY_ENV_ALIAS = joinUnderscore('RAMSHARED', 'VRAM', 'PREALLOC')
const SPARSE_EXPERIMENT_ALIAS = joinUnderscore('RAMSHARED', 'VRAM', 'SPARSE', 'EXPERIMENTAL')
const POWERSHELL_SELECTOR = joinEmpty('Preallocate', 'Vram')
const LEGACY_FIELD = joinUnderscore('legacy', 'prealloc')
const LEGACY_PARAMETER = joinUnderscore('use', 'prealloc')
const LEGACY_HELPER = joinUnderscore('prealloc', 'enabled')
const LEGACY_HELPER_VALUE = joinUnderscore('prealloc', 'enabled', 'value')
const LEGACY_PROFILES = joinUnderscore('GUARANTEED', 'PROFILES')
const LEGACY_CANDIDATES = joinUnderscore('guaranteed', 'profile', 'candidates')

export class LegacyPreallocationError extends Error {}

function escapeRegex(value) {
  return value.replace(/[.*+?^$()|[\]\\{}]/g, '\\$&')
}

function tokenRule(id, token) {
  return {
    id,
    expression: new RegExp('\\b' + escapeRegex(token) + '\\b', 'g'),
  }
}

const SOURCE_RULES = [
  tokenRule('LEGACY_ENV_SELECTOR', LEGACY_ENV_SELECTOR),
  tokenRule('LEGACY_ENV_ALIAS', LEGACY_ENV_ALIAS),
  tokenRule('SPARSE_EXPERIMENT_ALIAS', SPARSE_EXPERIMENT_ALIAS),
  tokenRule('LEGACY_POWERSHELL_SELECTOR', POWERSHELL_SELECTOR),
  tokenRule('LEGACY_APP_ARGS_FIELD', LEGACY_FIELD),
  tokenRule('LEGACY_NBD_PARAMETER', LEGACY_PARAMETER),
  tokenRule('LEGACY_HELPER', LEGACY_HELPER),
  tokenRule('LEGACY_HELPER_VALUE', LEGACY_HELPER_VALUE),
  tokenRule('LEGACY_GUARANTEED_PROFILES', LEGACY_PROFILES),
  tokenRule('LEGACY_PROFILE_CANDIDATES', LEGACY_CANDIDATES),
  {
    id: 'LEGACY_BE_PRE_REFERENCE',
    expression: /\bBe\s*::\s*Pre\b/g,
  },
  {
    id: 'LEGACY_BE_PRE_VARIANT',
    expression: /\benum\s+Be\b[\s\S]{0,600}?\bPre\s*\(/g,
  },
]

const DOCUMENTED_SELECTOR_TOKENS = [
  LEGACY_ENV_SELECTOR,
  LEGACY_ENV_ALIAS,
  SPARSE_EXPERIMENT_ALIAS,
  POWERSHELL_SELECTOR,
]
const REMOVAL_COMPLETE = /\b(?:was|were|has been|have been)\s+removed\b|\bno longer\s+(?:exists?|available|supported|selectable)\b|\b(?:foi|foram)\s+removid[oa]s?\b/iu
const DOC_RULES = [
  {
    id: 'DOC_LEGACY_BACKEND_AVAILABLE',
    expression: /(?:\b(?:legacy(?:[- ]preallocation)?\s+backend|full[- ]VRAM\s+NBD\s+backend|preallocation\s+backend)\b[^.\n]{0,160}\b(?:is|remains?|stays?|continues?\s+to\s+be|may\s+(?:remain|be))\s+(?:available|supported|selectable|retained|enabled|present)\b|\b(?:backend\s+legado(?:\s+de\s+pr[eé]-aloca[cç][aã]o)?|backend\s+NBD\s+de\s+VRAM\s+completa|backend\s+de\s+pr[eé]-aloca[cç][aã]o|composi[cç][aã]o\s+NBD\s+legada\s+de\s+VRAM\s+completa)\b[^.\n]{0,160}\b(?:est[aá]|permanece|continua)(?:\s+ainda)?\s+(?:dispon[ií]vel|suportad[oa]|selecion[aá]vel|retid[oa]|habilitad[oa]|presente)\b)/giu,
  },
  {
    id: 'DOC_LEGACY_REMOVAL_PENDING',
    expression: /\b(?:legacy[- ]preallocation|preallocation\s+backend|full[- ]VRAM\s+NBD\s+backend)\b[\s\S]{0,180}?\b(?:must|shall|needs?\s+to|is\s+required\s+to)\s+be\s+removed\b|\b(?:legacy[- ]preallocation\s+)?(?:removal|sunset)\s+gate\b[\s\S]{0,120}?\b(?:remains?\s+(?:open|blocked)|must\s+(?:close|pass)|is\s+(?:open|blocked|required))\b|\b(?:pré-alocação\s+legada|backend\s+de\s+pré-alocação)\b[\s\S]{0,180}?\bdeve(?:m)?\s+ser\s+removid[oa]s?\b/giu,
  },
]

function gitCandidatePaths(root) {
  try {
    return execFileSync('git', ['ls-files', '-co', '--exclude-standard', '-z'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 32 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
      .split('\0')
      .filter(Boolean)
  } catch {
    throw new LegacyPreallocationError('git-candidate-query-failed')
  }
}

function hasSafeSegments(file) {
  return file.split('/').every((segment) => segment && segment !== '.' && segment !== '..')
}

export function isSafeRepoPath(root, file) {
  if (
    typeof root !== 'string' ||
    typeof file !== 'string' ||
    !file ||
    CONTROL_OR_AMBIGUOUS_PATH.test(file) ||
    path.posix.isAbsolute(file) ||
    path.win32.isAbsolute(file) ||
    file.startsWith('-') ||
    file.startsWith(':') ||
    !hasSafeSegments(file)
  ) {
    return false
  }
  const resolvedRoot = path.resolve(root)
  const resolvedFile = path.resolve(resolvedRoot, file)
  const relative = path.relative(resolvedRoot, resolvedFile)
  return relative !== '' &&
    relative !== '..' &&
    !relative.startsWith('..' + path.sep) &&
    !path.isAbsolute(relative)
}

function isActiveSourceCandidate(file) {
  return ACTIVE_SOURCE_ROOTS.some((prefix) => file.startsWith(prefix))
}

function isHistoricalValidationCandidate(file) {
  return HISTORICAL_VALIDATION_FILES.has(file) ||
    HISTORICAL_DESIGN_RECORDS.has(file) ||
    HISTORICAL_DOCUMENT_ROOTS.some((prefix) => file.startsWith(prefix)) ||
    file.includes('/evidence/')
}

function isCurrentDocumentationCandidate(file) {
  if (isHistoricalValidationCandidate(file)) return false
  return CURRENT_ROOT_DOCS.has(file)
}

function contained(root, candidate) {
  const relative = path.relative(root, candidate)
  return relative !== '' &&
    relative !== '..' &&
    !relative.startsWith('..' + path.sep) &&
    !path.isAbsolute(relative)
}

function candidateFile(root, file) {
  if (!isSafeRepoPath(root, file)) throw new LegacyPreallocationError('unsafe-candidate-path')
  const canonicalRoot = realpathSync.native(root)
  const lexical = path.resolve(canonicalRoot, file)
  if (!contained(canonicalRoot, lexical)) throw new LegacyPreallocationError('unsafe-candidate-path')
  lstatSync(lexical)
  const canonicalFile = realpathSync.native(lexical)
  if (!contained(canonicalRoot, canonicalFile)) {
    throw new LegacyPreallocationError('candidate-symlink-escape')
  }
  return canonicalFile
}

function lineNumber(text, offset) {
  let line = 1
  for (let index = 0; index < offset; index++) if (text.charCodeAt(index) === 10) line++
  return line
}

function allMatches(expression, text) {
  const flags = expression.flags.includes('g') ? expression.flags : expression.flags + 'g'
  const matcher = new RegExp(expression.source, flags)
  const matches = []
  for (let match = matcher.exec(text); match; match = matcher.exec(text)) {
    matches.push(match)
    if (match[0] === '') matcher.lastIndex++
  }
  return matches
}

function selectorDocumentationFindings(file, text) {
  const findings = []
  for (const token of DOCUMENTED_SELECTOR_TOKENS) {
    const matcher = new RegExp('\\b' + escapeRegex(token) + '\\b', 'g')
    for (let match = matcher.exec(text); match; match = matcher.exec(text)) {
      const paragraphStart = text.lastIndexOf('\n\n', match.index)
      const paragraphEnd = text.indexOf('\n\n', match.index + match[0].length)
      const paragraph = text.slice(
        paragraphStart < 0 ? 0 : paragraphStart + 2,
        paragraphEnd < 0 ? text.length : paragraphEnd,
      )
      if (!REMOVAL_COMPLETE.test(paragraph)) {
        findings.push({
          path: file,
          line: lineNumber(text, match.index),
          rule: 'DOC_LEGACY_SELECTOR_AVAILABLE',
        })
      }
    }
  }
  return findings
}

export function scanText(file, text) {
  const findings = []
  const rules = isCurrentDocumentationCandidate(file) ? DOC_RULES : SOURCE_RULES
  for (const rule of rules) {
    for (const match of allMatches(rule.expression, text)) {
      findings.push({ path: file, line: lineNumber(text, match.index), rule: rule.id })
    }
  }
  if (isCurrentDocumentationCandidate(file)) {
    findings.push(...selectorDocumentationFindings(file, text))
  }
  return findings
}

export function run({ root = process.cwd() } = {}) {
  const findings = []
  for (const file of gitCandidatePaths(root).sort((left, right) => left.localeCompare(right))) {
    if (!isActiveSourceCandidate(file) && !isCurrentDocumentationCandidate(file)) continue
    try {
      const text = readFileSync(candidateFile(root, file), 'utf8')
      findings.push(...scanText(file, text))
    } catch (error) {
      const rule = error instanceof LegacyPreallocationError
        ? error.message.toUpperCase().replaceAll('-', '_')
        : 'CANDIDATE_READ_FAILED'
      findings.push({ path: file, line: 1, rule })
    }
  }
  findings.sort((left, right) =>
    left.path.localeCompare(right.path) ||
    left.line - right.line ||
    left.rule.localeCompare(right.rule))
  return { ok: findings.length === 0, findings }
}

function main(argv) {
  if (argv.length !== 1 || argv[0] !== '--candidate') {
    process.stderr.write('usage: check-legacy-preallocation-removal.mjs --candidate\n')
    return 2
  }
  try {
    const result = run()
    for (const finding of result.findings) {
      process.stderr.write(finding.path + ':' + finding.line + ' ' + finding.rule + '\n')
    }
    if (!result.ok) return 1
    process.stdout.write('PASS legacy-preallocation removal candidate scan\n')
    return 0
  } catch (error) {
    const reason = error instanceof LegacyPreallocationError ? error.message : 'unexpected-checker-error'
    process.stderr.write('legacy-preallocation checker: ' + reason + '\n')
    return 2
  }
}

const invokedPath = process.argv[1] && path.resolve(process.argv[1])
if (invokedPath === fileURLToPath(import.meta.url)) process.exitCode = main(process.argv.slice(2))
