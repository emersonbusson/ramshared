#!/usr/bin/env node
import { lstatSync, readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const DECISIONS = 'docs/decisions'
const ADR_FILENAME = /^ADR-(\d{4})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$/
const CANONICAL_HEADER = ['ADR', 'Filename', 'Title', 'Status', 'Governance profile']
const HISTORICAL_HEADER = ['Filename', 'Collides with canonical ADR', 'Registry disposition', 'Preservation note']
const GOVERNED_SECTIONS = ['status', 'context', 'decision', 'consequences', 'alternatives considered', 'kahneman', 'rollback trigger']

function safeFilename(value) {
  return typeof value === 'string' && ADR_FILENAME.test(value) && !value.includes('/') && !value.includes('\\')
}

function tableAfterHeading(text, heading) {
  const lines = String(text).split(/\r?\n/)
  const index = lines.findIndex((line) => line.trim() === heading)
  if (index < 0) return { error: `${heading === '## Canonical index' ? 'canonical-index' : heading === '## Historical filename collision' ? 'historical-collision' : 'record-format'}-heading` }
  let start = index + 1
  while (start < lines.length && lines[start].trim() === '') start++
  if (!/^\|/.test(lines[start] ?? '') || !/^\|/.test(lines[start + 1] ?? '')) return { error: `${heading === '## Canonical index' ? 'canonical-index' : 'historical-collision'}-table` }
  const cells = (line) => line.trim().replace(/^\||\|$/g, '').split('|').map((cell) => cell.trim())
  const rows = []
  for (let cursor = start + 2; cursor < lines.length && /^\|/.test(lines[cursor]); cursor++) rows.push(cells(lines[cursor]))
  return { header: cells(lines[start]), rows }
}

function filenameFromLink(cell) {
  const match = String(cell).match(/^\[[^\]]+\]\(([^)]+)\)$/)
  return match?.[1] ?? null
}

function statusFromRecord(text) {
  const inline = String(text).match(/^\s*(?:[-*]\s+)?\*\*Status:\*\*\s*([^\n]+)/mi)
  if (inline) return inline[1].match(/^(Proposed|Accepted|Deprecated|Superseded)/i)?.[1] ?? null
  const heading = String(text).match(/^##\s+Status\s*$\r?\n+([^\n]+)/mi)
  return heading?.[1]?.trim().match(/^(Proposed|Accepted|Deprecated|Superseded)/i)?.[1] ?? null
}

function headings(text) {
  return new Set([...String(text).matchAll(/^##\s+(.+?)\s*$/gmi)].map((match) => match[1].trim().toLowerCase()))
}

function unique(items) {
  return [...new Set(items)].sort()
}

export function validateAdrIndex(indexText, files) {
  const errors = []
  const canonicalTable = tableAfterHeading(indexText, '## Canonical index')
  const historicalTable = tableAfterHeading(indexText, '## Historical filename collision')
  const hasFormat = /^##\s+Record format\s*$/mi.test(String(indexText))
  if (canonicalTable.error) errors.push(canonicalTable.error)
  if (historicalTable.error) errors.push(historicalTable.error)
  if (!hasFormat) errors.push('record-format-heading')
  if (canonicalTable.error || historicalTable.error || !hasFormat) {
    for (const file of files) if (!safeFilename(file?.filename)) errors.push(`filename-invalid:${file?.filename ?? '<missing>'}`)
    return unique(errors)
  }
  if (JSON.stringify(canonicalTable.header) !== JSON.stringify(CANONICAL_HEADER)) errors.push('canonical-index-header')
  if (JSON.stringify(historicalTable.header) !== JSON.stringify(HISTORICAL_HEADER)) errors.push('historical-collision-header')

  const records = new Map()
  for (const file of files) {
    if (!safeFilename(file?.filename)) {
      errors.push(`filename-invalid:${file?.filename ?? '<missing>'}`)
      continue
    }
    if (records.has(file.filename)) errors.push(`duplicate-filename:${file.filename}`)
    records.set(file.filename, file)
  }

  const canonical = []
  const canonicalNames = new Set()
  const canonicalIds = new Set()
  for (const row of canonicalTable.rows) {
    if (row.length !== 5) {
      errors.push('canonical-index-row')
      continue
    }
    const [number, filenameCell, , status, profile] = row
    const filename = filenameFromLink(filenameCell)
    if (!/^\d{4}$/.test(number) || !safeFilename(filename)) {
      errors.push('canonical-index-entry')
      continue
    }
    const parsed = ADR_FILENAME.exec(filename)
    if (parsed[1] !== number) errors.push(`canonical-number-mismatch:${filename}`)
    if (!['legacy', 'governed-v1'].includes(profile)) errors.push(`canonical-profile:${filename}`)
    if (!status) errors.push(`canonical-status:${filename}`)
    if (canonicalNames.has(filename)) errors.push(`canonical-filename-duplicate:${filename}`)
    if (canonicalIds.has(number)) errors.push(`canonical-id-duplicate:${number}`)
    canonicalNames.add(filename)
    canonicalIds.add(number)
    canonical.push({ number, filename, status, profile })
  }

  const historical = []
  const historicalNames = new Set()
  for (const row of historicalTable.rows) {
    if (row.length !== 4) {
      errors.push('historical-collision-row')
      continue
    }
    if (row[0] === 'None') continue
    const [filenameCell, collidesWith, disposition, note] = row
    const filename = filenameFromLink(filenameCell)
    if (!safeFilename(filename) || !/^\d{4}$/.test(collidesWith)) {
      errors.push('historical-collision-entry')
      continue
    }
    if (disposition !== 'historical-noncanonical') errors.push(`historical-disposition:${filename}`)
    if (!/preserved filename collision/i.test(note)) errors.push(`historical-preservation-note:${filename}`)
    if (historicalNames.has(filename) || canonicalNames.has(filename)) errors.push(`historical-filename-duplicate:${filename}`)
    historicalNames.add(filename)
    historical.push({ filename, collidesWith, disposition })
  }

  for (const entry of canonical) {
    const record = records.get(entry.filename)
    if (!record) {
      errors.push(`canonical-record-missing:${entry.filename}`)
      continue
    }
    const recordStatus = statusFromRecord(record.text)
    if (!recordStatus || recordStatus.toLowerCase() !== entry.status.toLowerCase()) errors.push(`canonical-status-mismatch:${entry.filename}`)
    if (entry.profile === 'governed-v1') {
      const actual = headings(record.text)
      for (const section of GOVERNED_SECTIONS) if (!actual.has(section)) errors.push(`governed-section-missing:${entry.filename}:${section}`)
    }
  }

  for (const entry of historical) {
    const record = records.get(entry.filename)
    if (!record) errors.push(`historical-record-missing:${entry.filename}`)
    const parsed = ADR_FILENAME.exec(entry.filename)
    if (parsed && parsed[1] !== entry.collidesWith) errors.push(`historical-not-duplicate:${entry.filename}`)
    if (!canonicalIds.has(entry.collidesWith)) errors.push(`historical-canonical-missing:${entry.filename}`)
  }

  for (const filename of records.keys()) if (!canonicalNames.has(filename) && !historicalNames.has(filename)) errors.push(`orphan-record:${filename}`)

  const byNumber = new Map()
  for (const filename of records.keys()) {
    const number = ADR_FILENAME.exec(filename)?.[1]
    if (!number) continue
    byNumber.set(number, [...(byNumber.get(number) ?? []), filename])
  }
  for (const [number, names] of byNumber) {
    if (names.length < 2) continue
    const canonicalForNumber = canonical.filter((entry) => entry.number === number)
    const historicalForNumber = historical.filter((entry) => entry.collidesWith === number)
    const declared = canonicalForNumber.length === 1 && historicalForNumber.length === names.length - 1 &&
      historicalForNumber.every((entry) => names.includes(entry.filename) && entry.disposition === 'historical-noncanonical')
    if (!declared) errors.push(`duplicate-numeric-id:${number}`)
  }
  return unique(errors)
}

export function readAdrFiles(root, { lstat = lstatSync, read = readFileSync, list = readdirSync } = {}) {
  const directory = path.join(root, DECISIONS)
  const errors = []
  const files = []
  let names = []
  try { names = list(directory).filter((name) => name.startsWith('ADR-')).sort() } catch { return { files, errors: ['adr-directory-unreadable'] } }
  for (const filename of names) {
    const target = path.join(directory, filename)
    try {
      const stat = lstat(target)
      if (!stat.isFile() || stat.isSymbolicLink()) {
        errors.push(`adr-record-unsafe:${filename}`)
        continue
      }
      files.push({ filename, text: read(target, 'utf8') })
    } catch { errors.push(`adr-record-unreadable:${filename}`) }
  }
  return { files, errors }
}

export function run({ root = ROOT } = {}) {
  const indexPath = path.join(root, DECISIONS, 'README.md')
  let indexText = ''
  const errors = []
  try {
    const stat = lstatSync(indexPath)
    if (!stat.isFile() || stat.isSymbolicLink()) errors.push('adr-index-unsafe')
    else indexText = readFileSync(indexPath, 'utf8')
  } catch { errors.push('adr-index-unreadable') }
  const scanned = readAdrFiles(root)
  errors.push(...scanned.errors)
  if (indexText) errors.push(...validateAdrIndex(indexText, scanned.files))
  return { ok: errors.length === 0, records: scanned.files.length, errors: unique(errors) }
}

export function main(argv = process.argv.slice(2)) {
  if (argv.length !== 1 || !['--check', '--all'].includes(argv[0])) {
    process.stderr.write('usage: node tools/ci/check-adr-index.mjs --check\n')
    return 2
  }
  const result = run()
  if (result.ok) {
    process.stdout.write(`ADR_INDEX PASS records=${result.records}\n`)
    return 0
  }
  for (const error of result.errors) process.stderr.write(`ADR_INDEX ${error}\n`)
  return 1
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exitCode = main()
