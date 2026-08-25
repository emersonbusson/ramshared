import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  CLAIM_CLOSURE_SCHEMA,
  canonicalJson,
  computeClaimDigest,
  computeClosureDigest,
  evaluateClaimClosure,
  evaluateClaimClosures,
  requiredClaimPaths,
} from './documentation-claim-closure.mjs'

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function write(root, relative, value) {
  const absolute = path.join(root, relative)
  mkdirSync(path.dirname(absolute), { recursive: true })
  writeFileSync(absolute, value)
}

function git(root, args, encoding = 'utf8') {
  return execFileSync('git', args, { cwd: root, encoding, stdio: ['ignore', 'pipe', 'pipe'] })
}

function commit(root, message) {
  git(root, ['add', '--all'])
  git(root, ['commit', '-q', '-m', message])
  return git(root, ['rev-parse', 'HEAD']).trim()
}

function closureFixture({ manifestSlug = 'fixture', manifestStatus = 'DONE' } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-claim-closure-'))
  git(root, ['init', '-q'])
  git(root, ['config', 'user.name', 'RamShared fixture'])
  git(root, ['config', 'user.email', ['fixture', 'example.invalid'].join('@')])
  write(root, 'README.md', '# Base\n')
  const base = commit(root, 'base')

  const specPath = 'docs/specs/no-milestone/fixture/SPEC.md'
  const implPath = 'tools/fixture.mjs'
  const testPath = 'tools/fixture.test.mjs'
  const summaryPath = 'docs/specs/no-milestone/fixture/evidence/validation-summary.json'
  const manifestPath = 'docs/specs/no-milestone/fixture/evidence-manifest.json'
  const spec = '# Fixture specification\n'
  const summary = '{"ok":true}\n'
  write(root, specPath, spec)
  write(root, implPath, 'export const implemented = true\n')
  write(root, testPath, "test('fixture_test', () => {})\n")
  write(root, summaryPath, summary)
  write(root, 'validation.md', '**Verdict:** PASS\n')
  const manifest = {
    schema_version: 'ramshared-spec-evidence/v1',
    slug: manifestSlug,
    status: manifestStatus,
    spec: { path: specPath, sha256: sha256(spec) },
    tests: [{ name: 'fixture_test', path: testPath, kind: 'unit', exit_code: 0 }],
    cover: [{ path: implPath, classification: 'Node named tests', justification: 'fixture' }],
    live: { evidence_artifacts: [summaryPath] },
    artifacts: [{ path: summaryPath, bytes: Buffer.byteLength(summary), sha256: sha256(summary) }],
    validation_path: 'validation.md',
    impl_path: implPath,
  }
  write(root, manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  const source = commit(root, 'evidence')
  const claim = {
    slug: 'fixture',
    state: 'DONE',
    owner_role: 'documentation-governance',
    canonical_spec: specPath,
    implementation_paths: [implPath],
    named_tests: [{ path: testPath, name: 'fixture_test' }],
    cover: { mode: 'node-built-in', minimum_percent: 80, evidence_path: summaryPath },
    validation: { record_path: 'validation.md', verdict: '✅', source_commit: source, evidence_paths: [manifestPath] },
    binary_match_required: false,
    environment_blocker: null,
    missing_gate: null,
    next_proof: null,
    rollback_trigger: 'one forged claim closure passes',
  }
  const paths = requiredClaimPaths(claim, [manifest])
  const closure = {
    slug: claim.slug,
    state: claim.state,
    base_revision: base,
    source_revision: source,
    files: paths.map((relative) => ({ path: relative, sha256: sha256(readFileSync(path.join(root, relative))) })),
    evidence_manifests: [manifestPath],
    claim_sha256: computeClaimDigest(claim),
    closure_sha256: '',
  }
  closure.closure_sha256 = computeClosureDigest(closure)
  return { root, claim, closure }
}

test('claim_canonicalization_sorts_object_keys_and_preserves_array_order', () => {
  const value = { z: 1, a: { y: true, x: null }, list: ['b', 'a'] }
  assert.equal(canonicalJson(value), '{"a":{"x":null,"y":true},"list":["b","a"],"z":1}')
  assert.equal(computeClaimDigest(value), computeClaimDigest({ list: ['b', 'a'], a: { x: null, y: true }, z: 1 }))
  assert.notEqual(computeClaimDigest(value), computeClaimDigest({ ...value, list: ['a', 'b'] }))
})

test('valid_same_revision_claim_closure_passes', () => {
  const { root, claim, closure } = closureFixture()
  const result = evaluateClaimClosure(claim, closure, { root })
  assert.equal(result.qualified, true)
  assert.equal(result.status, 'DONE')
  assert.equal(result.revision, closure.source_revision)
  assert.deepEqual(result.findings, [])
})

test('claims_reject_nonexistent_source_commit', () => {
  const { root, claim, closure } = closureFixture()
  closure.source_revision = 'f'.repeat(40)
  claim.validation.source_commit = closure.source_revision
  closure.closure_sha256 = computeClosureDigest(closure)
  const result = evaluateClaimClosure(claim, closure, { root })
  assert.equal(result.qualified, false)
  assert.match(result.findings.join('\n'), /source-revision-missing/)
})

test('claim_revision_must_contain_declared_evidence', () => {
  const { root, claim, closure } = closureFixture()
  const missing = 'docs/evidence/not-in-source.json'
  claim.implementation_paths.push(missing)
  closure.files.push({ path: missing, sha256: '0'.repeat(64) })
  closure.files.sort((a, b) => a.path.localeCompare(b.path))
  closure.closure_sha256 = computeClosureDigest(closure)
  const result = evaluateClaimClosure(claim, closure, { root })
  assert.equal(result.qualified, false)
  assert.match(result.findings.join('\n'), /revision-evidence-missing/)
})

test('claim_digest_rejects_owner_rollback_status_and_evidence_tamper', () => {
  const cases = [
    ['owner', (claim) => { claim.owner_role = 'replayed-owner' }],
    ['rollback', (claim) => { claim.rollback_trigger = 'two forged closures pass' }],
    ['status', (claim, closure) => { claim.state = 'PARTIAL'; closure.state = 'PARTIAL' }],
    ['evidence', (claim) => { claim.validation.verdict = '🟡' }],
  ]
  for (const [name, mutate] of cases) {
    const { root, claim, closure } = closureFixture()
    mutate(claim, closure)
    closure.closure_sha256 = computeClosureDigest(closure)
    const result = evaluateClaimClosure(claim, closure, { root })
    assert.equal(result.qualified, false, name)
    assert.match(result.findings.join('\n'), /claim-digest-mismatch/, name)
  }
})

test('claim_digest_rejects_cross_claim_closure_replay', () => {
  const { root, claim, closure } = closureFixture()
  claim.slug = 'replayed-fixture'
  closure.slug = claim.slug
  closure.closure_sha256 = computeClosureDigest(closure)
  const result = evaluateClaimClosure(claim, closure, { root })
  assert.equal(result.qualified, false)
  assert.match(result.findings.join('\n'), /claim-digest-mismatch/)
})

test('done_claim_rejects_foreign_or_partial_manifest', () => {
  for (const options of [{ manifestSlug: 'foreign' }, { manifestStatus: 'PARTIAL' }]) {
    const { root, claim, closure } = closureFixture(options)
    const result = evaluateClaimClosure(claim, closure, { root })
    assert.equal(result.qualified, false)
    assert.match(result.findings.join('\n'), /foreign-manifest|partial-manifest/)
  }
})

test('closure_digest_and_base_ancestry_fail_closed', () => {
  const { root, claim, closure } = closureFixture()
  closure.base_revision = closure.source_revision
  closure.closure_sha256 = '0'.repeat(64)
  const digestResult = evaluateClaimClosure(claim, closure, { root })
  assert.match(digestResult.findings.join('\n'), /closure-digest-mismatch/)

  const other = mkdtempSync(path.join(tmpdir(), 'ramshared-unrelated-'))
  git(other, ['init', '-q'])
  git(other, ['config', 'user.name', 'RamShared fixture'])
  git(other, ['config', 'user.email', ['fixture', 'example.invalid'].join('@')])
  write(other, 'other.md', '# Other\n')
  const unrelated = commit(other, 'unrelated')
  git(root, ['fetch', '-q', other, unrelated])
  closure.base_revision = unrelated
  closure.closure_sha256 = computeClosureDigest(closure)
  assert.match(evaluateClaimClosure(claim, closure, { root }).findings.join('\n'), /base-not-ancestor/)
})

test('done_without_closure_is_unqualified_and_partial_without_closure_stays_honest', () => {
  const { root, claim } = closureFixture()
  assert.equal(evaluateClaimClosure(claim, null, { root }).status, 'UNQUALIFIED')
  const partial = { ...claim, state: 'PARTIAL' }
  const result = evaluateClaimClosure(partial, null, { root })
  assert.equal(result.status, 'PARTIAL')
  assert.deepEqual(result.findings, [])
})

test('closure_registry_rejects_invalid_duplicate_and_orphan_entries', () => {
  const { root, claim, closure } = closureFixture()
  const registry = { schema_version: CLAIM_CLOSURE_SCHEMA, closures: [closure, closure, { ...closure, slug: 'orphan' }] }
  const result = evaluateClaimClosures({ claims: [claim] }, registry, { root })
  assert.match(result.findings.join('\n'), /duplicate|orphan/)
  assert.match(evaluateClaimClosures({ claims: [claim] }, null, { root }).findings.join('\n'), /invalid-registry/)
})
