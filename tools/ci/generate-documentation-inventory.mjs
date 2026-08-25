#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import { classifyDocument, listDocumentPaths } from './check-document-lifecycle.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const POLICY_PATH = 'docs/governance/document-lifecycle-policy.json'
const OUTPUT_PATH = 'docs/reference/DOCUMENTATION-INVENTORY.json'
const TIMESTAMPED_RUN_LABEL = /\b(?!SANITIZED_)[A-Za-z][A-Za-z0-9_]*(?:-[A-Za-z0-9_]+)*-\d{8}(?:[-T_]\d{6})\b/g

/** Render public-safe references without exposing a dated run identity. */
export function publicDocumentReference(value) {
  return value.replace(TIMESTAMPED_RUN_LABEL, (label) => `SANITIZED_RUN_REF_${createHash('sha256').update(label).digest('hex').slice(0, 12).toUpperCase()}`)
}

function loadPolicy(root) {
  return JSON.parse(readFileSync(path.join(root, POLICY_PATH), 'utf8'))
}

export function buildInventory({ root = ROOT, paths = null, policy = null } = {}) {
  const documentPaths = paths ?? listDocumentPaths(root)
  const activePolicy = policy ?? loadPolicy(root)
  const entries = [...documentPaths].sort().map((pathname) => {
    const result = classifyDocument(pathname, activePolicy)
    const common = { path: publicDocumentReference(pathname), disposition: result.disposition, ruleId: result.ruleId ?? null, verification: { state: result.verificationState ?? 'unverified' } }
    if (result.disposition === 'classified') return { ...common, owner: result.owner, canonicalSource: result.canonicalSource, lifecycle: result.lifecycle, freshnessDays: result.freshnessDays }
    if (result.disposition === 'excluded') return { ...common, owner: result.owner, reason: result.reason }
    return common
  })
  const count = (predicate) => entries.filter(predicate).length
  return {
    schemaVersion: 'ramshared.documentation-inventory.v1',
    source: 'Public-safe Markdown references visible in the repository worktree; classification is not content verification',
    counts: { total: entries.length, classified: count((entry) => entry.disposition === 'classified'), excluded: count((entry) => entry.disposition === 'excluded'), unclassified: count((entry) => entry.disposition === 'unclassified'), ambiguous: count((entry) => entry.disposition === 'ambiguous') },
    entries,
  }
}

export function renderInventory(inventory) {
  return `${JSON.stringify(inventory, null, 2)}\n`
}

export function checkInventory({ root = ROOT, current = null } = {}) {
  const actual = current ?? readFileSync(path.join(root, OUTPUT_PATH), 'utf8')
  const expected = renderInventory(buildInventory({ root }))
  return { ok: actual === expected, reason: actual === expected ? 'in-sync' : 'out-of-sync', expected }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  if (argv.length !== 1 || !['--write', '--check'].includes(argv[0])) {
    console.error('usage: generate-documentation-inventory.mjs --write|--check')
    return 2
  }
  if (argv[0] === '--write') {
    writeFileSync(path.join(ROOT, OUTPUT_PATH), renderInventory(buildInventory({ root: ROOT })))
    console.log(`DOCUMENTATION_INVENTORY=written ${OUTPUT_PATH}`)
    return 0
  }
  const result = checkInventory({ root: ROOT })
  console.log(`DOCUMENTATION_INVENTORY=${result.reason}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
/* node:coverage enable */
