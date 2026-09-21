import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  classifyDocument,
  listDocumentPaths,
  listTrackedMarkdown,
  readBasePolicy,
  run,
  validatePolicy,
} from './check-document-lifecycle.mjs'
import {
  buildInventory,
  renderInventory,
} from './generate-documentation-inventory.mjs'

function policy(overrides = {}) {
  return {
    schemaVersion: 'ramshared.document-lifecycle-policy.v1',
    owner: 'documentation-governance',
    registeredAt: '2026-08-11T14:00:00Z',
    routes: [{
      id: 'docs', pattern: 'docs/**/*.md', owner: 'documentation-governance',
      canonicalSource: 'docs/DOCUMENTATION-PARITY.md', lifecycle: 'reviewable',
      freshnessDays: 30, verification: { state: 'unverified' },
    }],
    exclusions: [],
    ...overrides,
  }
}

test('classification does not imply verification', () => {
  const result = classifyDocument('docs/example.md', policy())
  assert.deepEqual(result, {
    disposition: 'classified', ruleId: 'docs', owner: 'documentation-governance',
    canonicalSource: 'docs/DOCUMENTATION-PARITY.md', lifecycle: 'reviewable',
    freshnessDays: 30, verificationState: 'unverified',
  })
})

test('policy rejects unsafe, future, and incomplete lifecycle metadata', () => {
  const unsafe = policy({ routes: [{ ...policy().routes[0], pattern: '../docs/**/*.md' }] })
  assert.match(JSON.stringify(validatePolicy(unsafe, new Date('2026-08-11T14:01:00Z'))), /unsafe-pattern/)
  assert.match(JSON.stringify(validatePolicy(policy({ registeredAt: '2099-01-01T00:00:00Z' }), new Date('2026-08-11T14:01:00Z'))), /future-metadata/)
  assert.match(JSON.stringify(validatePolicy(policy({ routes: [{ ...policy().routes[0], freshnessDays: null }] }), new Date('2026-08-11T14:01:00Z'))), /freshness/)
})

test('unclassified documents fail closed', () => {
  assert.equal(classifyDocument('README.md', policy()).disposition, 'unclassified')
})

test('base comparison rejects a changed document downgraded to an exclusion', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-document-lifecycle-'))
  mkdirSync(path.join(root, 'docs'), { recursive: true })
  writeFileSync(path.join(root, 'docs', 'example.md'), '# Example\n')
  const current = policy({
    routes: [],
    exclusions: [{ id: 'legacy', pattern: 'docs/example.md', owner: 'documentation-governance', reason: 'fixture', registeredAt: '2026-08-11T14:00:00Z' }],
  })
  const report = run({ root, policy: current, paths: ['docs/example.md'], basePolicy: policy() })
  assert.match(JSON.stringify(report.findings), /policy-regression/)
})

test('new lifecycle policy has no prior policy only after a valid Git base', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-document-lifecycle-base-'))
  try {
    writeFileSync(path.join(root, 'README.md'), '# Baseline\n')
    execFileSync('git', ['init', '-q'], { cwd: root })
    execFileSync('git', ['config', 'user.email', 'fixture'], { cwd: root })
    execFileSync('git', ['config', 'user.name', 'Fixture'], { cwd: root })
    execFileSync('git', ['add', 'README.md'], { cwd: root })
    execFileSync('git', ['commit', '-qm', 'baseline'], { cwd: root })

    assert.equal(readBasePolicy(root, 'HEAD'), null)
    assert.throws(() => readBasePolicy(root, 'not-a-revision'))
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('passive inventory is deterministic and preserves unverified state', () => {
  const inventory = buildInventory({ paths: ['docs/b.md', 'docs/a.md'], policy: policy() })
  assert.equal(inventory.entries[0].path, 'docs/a.md')
  assert.equal(inventory.entries[0].verification.state, 'unverified')
  assert.equal(renderInventory(inventory), renderInventory(inventory))
})

test('worktree document lists omit deleted tracked Markdown', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-document-worktree-'))
  try {
    mkdirSync(path.join(root, 'docs'), { recursive: true })
    writeFileSync(path.join(root, 'docs', 'live.md'), '# Live\n')
    writeFileSync(path.join(root, 'docs', 'deleted.md'), '# Deleted\n')
    execFileSync('git', ['init', '-q'], { cwd: root })
    execFileSync('git', ['config', 'user.email', 'fixture'], { cwd: root })
    execFileSync('git', ['config', 'user.name', 'Fixture'], { cwd: root })
    execFileSync('git', ['add', 'docs/live.md', 'docs/deleted.md'], { cwd: root })
    execFileSync('git', ['commit', '-qm', 'baseline'], { cwd: root })
    rmSync(path.join(root, 'docs', 'deleted.md'))
    writeFileSync(path.join(root, 'docs', 'untracked.md'), '# Untracked\n')

    assert.deepEqual(listTrackedMarkdown(root), ['docs/live.md'])
    assert.deepEqual(listDocumentPaths(root), ['docs/live.md', 'docs/untracked.md'])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('repository lifecycle policy and passive inventory are current', () => {
  const result = run({ root: process.cwd() })
  assert.equal(result.ok, true, JSON.stringify(result.findings, null, 2))
})
