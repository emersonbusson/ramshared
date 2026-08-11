import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { validateClaimManifest, validateRepositoryClaims } from './check-spec-evidence.mjs'

function hash(text) {
  return createHash('sha256').update(text).digest('hex')
}

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-spec-evidence-'))
  const dir = path.join(root, 'docs', 'specs', 'no-milestone', 'fixture')
  mkdirSync(path.join(dir, 'evidence'), { recursive: true })
  const specText = '# SPEC\n'
  writeFileSync(path.join(dir, 'SPEC.md'), specText)
  writeFileSync(path.join(dir, 'IMPL.md'), '# IMPL\n## Status\nimplemented\n')
  writeFileSync(path.join(dir, 'validation.md'), '# Validation\n')
  writeFileSync(path.join(dir, 'test.mjs'), "test('fixture_named_test', () => {})\n")
  writeFileSync(path.join(dir, 'evidence', 'result.json'), '{}')
  return { root, dir, specText }
}

function doneManifest(ctx) {
  const rel = path.relative(ctx.root, ctx.dir).replaceAll('\\', '/')
  return {
    schema_version: 'ramshared-spec-evidence/v1',
    slug: 'fixture',
    status: 'DONE',
    spec: { path: `${rel}/SPEC.md`, sha256: hash(ctx.specText) },
    tests: [{ name: 'fixture_named_test', path: `${rel}/test.mjs`, kind: 'unit', exit_code: 0 }],
    cover: [{ path: 'tools/fixture.mjs', classification: 'N/A — Node unit-tested', justification: 'Rust slice coverage is inapplicable.' }],
    live: {
      required: true,
      before: { clean: true },
      action: { command: 'public fixture command' },
      after: { clean: true },
      legitimate: { verdict: 'PASS' },
      refusals: [{ name: 'invalid-input', verdict: 'PASS' }],
      cleanup: { complete: true, residue: 0 },
      evidence_artifacts: [`${rel}/evidence/result.json`],
    },
    binary_match: { required: true, passed: true, identities: [{ name: 'fixture', sha256: 'a'.repeat(64) }] },
    artifacts: [{ path: `${rel}/evidence/result.json`, bytes: 2, sha256: hash('{}') }],
    validation_path: `${rel}/validation.md`,
    impl_path: `${rel}/IMPL.md`,
    gaps: { open: [], env_bound: [] },
    rollback_trigger: 'one missing or mismatched artifact',
  }
}

test('complete_done_manifest_passes', () => {
  const ctx = fixture()
  assert.deepEqual(validateClaimManifest(doneManifest(ctx), ctx.root), [])
})

test('valid_partial_manifest_passes', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  record.status = 'PARTIAL'
  record.live.required = false
  record.live.evidence_artifacts = []
  record.binary_match = { required: false, passed: false, identities: [] }
  record.gaps.env_bound = [{ blocker: 'lab GPU unavailable', next_proof: 'run the named lab drill' }]
  assert.deepEqual(validateClaimManifest(record, ctx.root), [])
})

test('partial_status_with_implemented_word_is_not_done', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  record.status = 'PARTIAL'
  record.gaps.env_bound = [{ blocker: 'hardware unavailable', next_proof: 'run hardware drill' }]
  assert.deepEqual(validateClaimManifest(record, ctx.root), [])
  assert.notEqual(record.status, 'DONE')
})

test('status_heading_variant_or_missing_manifest_fails_closed_when_claimed', () => {
  const ctx = fixture()
  assert.deepEqual(validateRepositoryClaims({ root: ctx.root }).findings, [])
  writeFileSync(path.join(ctx.dir, 'claim-status.json'), JSON.stringify({ status: 'DONE' }))
  assert.match(validateRepositoryClaims({ root: ctx.root }).findings.join('\n'), /claim-without-manifest/)
})

test('done_requires_named_tests_cover_live_e2e_refusal_cleanup_and_binary_match', () => {
  const ctx = fixture()
  const mutations = [
    (r) => { r.tests = [] },
    (r) => { r.cover = [] },
    (r) => { r.live.after = null },
    (r) => { r.live.refusals = [] },
    (r) => { r.live.cleanup.complete = false },
    (r) => { r.binary_match.passed = false },
  ]
  for (const mutate of mutations) {
    const record = doneManifest(ctx)
    mutate(record)
    assert.notDeepEqual(validateClaimManifest(record, ctx.root), [])
  }
})

test('env_bound_evidence_cannot_publish_done', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  record.gaps.env_bound = [{ blocker: 'no lab', next_proof: 'run lab drill' }]
  assert.match(validateClaimManifest(record, ctx.root).join('\n'), /done-env-bound/)
})

test('artifact_hash_mismatch_fails', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  record.artifacts[0].sha256 = 'b'.repeat(64)
  assert.match(validateClaimManifest(record, ctx.root).join('\n'), /artifact-hash/)
})
