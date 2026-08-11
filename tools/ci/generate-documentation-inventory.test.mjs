import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

import {
  buildInventory,
  checkInventory,
} from './generate-documentation-inventory.mjs'
import { listDocumentPaths } from './check-document-lifecycle.mjs'

test('inventory covers every tracked Markdown path without claiming verification', () => {
  const paths = listDocumentPaths(process.cwd())
  const inventory = buildInventory({ root: process.cwd(), paths })
  assert.equal(inventory.counts.total, paths.length)
  assert.ok(paths.length >= 210)
  assert.ok(inventory.entries.filter((entry) => entry.disposition === 'classified').every((entry) => entry.verification.state === 'unverified'))
  assert.ok(inventory.entries.filter((entry) => entry.disposition === 'excluded').every((entry) => entry.verification.state === 'not-applicable'))
})

test('checked inventory matches its deterministic generator output', () => {
  const output = readFileSync('docs/reference/DOCUMENTATION-INVENTORY.json', 'utf8')
  const result = checkInventory({ root: process.cwd(), current: output })
  assert.equal(result.ok, true, result.reason)
})
