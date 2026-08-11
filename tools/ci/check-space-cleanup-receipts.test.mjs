import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  validateReceipt,
  validateRepository,
} from './check-space-cleanup-receipts.mjs'

function historicalReceipt(overrides = {}) {
  return {
    schema_version: 'ramshared-space-cleanup-receipt/v1',
    receipt_id: 'space-cleanup-20260713-historical-001',
    recorded_at: '2026-07-13T14:42:26-03:00',
    owner: 'host-operator',
    classification: 'historical',
    promotion: {
      qualification: 'unqualified',
      eligible: false,
    },
    source: {
      recorded_at: '2026-07-13T14:42:26-03:00',
      document: 'docs/reliability/historical-space-note.md',
      locator: '#recorded-cleanup-note',
      revision: '1'.repeat(40),
      revision_status: 'recorded',
    },
    i_volume_gate: {
      volume: 'I:',
      observed_at: '2026-07-13T14:42:26-03:00',
      free_bytes: null,
      total_bytes: null,
      byte_status: 'not-recorded',
      gate: 'not-qualified',
    },
    targets: [
      {
        class: 'rebuildable-container-cache',
        disposition: 'removed',
        before_bytes: null,
        after_bytes: null,
        byte_status: 'not-recorded',
      },
      {
        class: 'rebuildable-build-cache',
        disposition: 'removed',
        before_bytes: null,
        after_bytes: null,
        byte_status: 'not-recorded',
      },
      {
        class: 'toolchain',
        disposition: 'retained',
        before_bytes: null,
        after_bytes: null,
        byte_status: 'not-recorded',
      },
    ],
    postcondition: {
      status: 'not-qualified',
      statement: 'No before/after byte receipt survives; this record cannot approve a cleanup action.',
    },
    retention: {
      class: 'append-only',
      disposition: 'retain',
    },
    safe_paths: ['docs/reliability/historical-space-note.md'],
    ...overrides,
  }
}

function fixtureRepo(receipts = [historicalReceipt()]) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-space-cleanup-'))
  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  mkdirSync(path.join(root, 'docs', 'reliability'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'reliability', 'historical-space-note.md'), '# Recorded cleanup note\n')
  writeFileSync(
    path.join(root, 'docs', 'governance', 'space-cleanup-receipts.jsonl'),
    `${receipts.map((receipt) => JSON.stringify(receipt)).join('\n')}\n`,
  )
  return root
}

test('accepts a bounded historical unqualified receipt', () => {
  const root = fixtureRepo()
  assert.deepEqual(validateReceipt(historicalReceipt(), { root }), [])
  assert.deepEqual(validateRepository({ root }).findings, [])
})

test('accepts a repository-local historical source without an invented revision', () => {
  const root = fixtureRepo()
  writeFileSync(path.join(root, 'validation.md'), '# Historical validation\n')
  const receipt = historicalReceipt({
    receipt_id: 'space-cleanup-20260715-historical-002',
    recorded_at: '2026-07-15T15:30:00-03:00',
    source: {
      recorded_at: '2026-07-15T15:30:00-03:00',
      document: 'validation.md',
      locator: '#2026-07-15-1530-03',
      revision: null,
      revision_status: 'not-recorded',
    },
    i_volume_gate: {
      volume: 'I:',
      observed_at: '2026-07-15T15:30:00-03:00',
      free_bytes: null,
      total_bytes: null,
      byte_status: 'not-recorded',
      gate: 'not-qualified',
    },
    targets: [{
      class: 'rebuildable-container-cache',
      disposition: 'removed',
      before_bytes: null,
      after_bytes: null,
      byte_status: 'not-recorded',
    }],
    safe_paths: ['validation.md'],
  })
  assert.deepEqual(validateReceipt(receipt, { root }), [])
})

test('requires exact recorded time, source, owner, I volume gate, retention, and safe paths', () => {
  const root = fixtureRepo()
  const receipt = historicalReceipt()
  delete receipt.owner
  delete receipt.source.recorded_at
  delete receipt.source.revision
  receipt.recorded_at = '2026-07-13'
  delete receipt.i_volume_gate
  receipt.safe_paths = []
  delete receipt.retention
  const findings = validateReceipt(receipt, { root }).join('\n')
  assert.match(findings, /recorded-at/)
  assert.match(findings, /owner/)
  assert.match(findings, /source-recorded-at/)
  assert.match(findings, /source-revision/)
  assert.match(findings, /i-volume-gate/)
  assert.match(findings, /safe-paths/)
  assert.match(findings, /retention/)
})

test('requires each target to state disposition and byte knowledge consistently', () => {
  const root = fixtureRepo()
  const receipt = historicalReceipt()
  receipt.targets[0] = {
    class: 'rebuildable-container-cache',
    disposition: 'removed',
    before_bytes: 100,
    after_bytes: null,
    byte_status: 'measured',
  }
  const findings = validateReceipt(receipt, { root }).join('\n')
  assert.match(findings, /target-bytes/)

  receipt.targets[0] = {
    class: 'rebuildable-container-cache',
    disposition: 'removed',
    before_bytes: 100,
    after_bytes: 20,
    byte_status: 'measured',
  }
  assert.doesNotMatch(validateReceipt(receipt, { root }).join('\n'), /target-bytes/)
})

test('rejects historical promotion and outcomes that can imply current authorization', () => {
  const root = fixtureRepo()
  const receipt = historicalReceipt()
  receipt.promotion.eligible = true
  receipt.postcondition.status = 'approved'
  const findings = validateReceipt(receipt, { root }).join('\n')
  assert.match(findings, /historical-promotion/)
  assert.match(findings, /postcondition/)
})

test('rejects unsafe target classes and private, secret, or traversal evidence paths', () => {
  const root = fixtureRepo()
  const receipt = historicalReceipt()
  receipt.targets[0].class = 'vm-vhd'
  receipt.safe_paths = ['../outside.md']
  receipt.source.document = 'C:\\Users\\private\\receipt.md'
  receipt.postcondition.statement = 'token=private-value'
  const findings = validateReceipt(receipt, { root }).join('\n')
  assert.match(findings, /unsafe-target-class/)
  assert.match(findings, /safe-path/)
  assert.match(findings, /source-document/)
  assert.match(findings, /sensitive-content/)
})

test('repository checker rejects duplicate IDs, malformed JSONL, and a missing source document', () => {
  const duplicate = historicalReceipt()
  const root = fixtureRepo([historicalReceipt(), duplicate])
  assert.match(validateRepository({ root }).findings.join('\n'), /duplicate-receipt-id/)

  const malformedRoot = fixtureRepo()
  writeFileSync(path.join(malformedRoot, 'docs', 'governance', 'space-cleanup-receipts.jsonl'), '{not json}\n')
  assert.match(validateRepository({ root: malformedRoot }).findings.join('\n'), /jsonl-parse/)

  const missing = historicalReceipt()
  missing.source.document = 'docs/reliability/missing.md'
  assert.match(validateReceipt(missing, { root: fixtureRepo() }).join('\n'), /source-document-missing/)
})

test('checker remains a read-only validator and has no process-launch or write primitive', () => {
  const source = readFileSync(new URL('./check-space-cleanup-receipts.mjs', import.meta.url), 'utf8')
  assert.doesNotMatch(source, /node:child_process|\bexecFile\b|\bspawn\b|\bwriteFile\b|\bunlink\b|\brmSync\b/)
})
