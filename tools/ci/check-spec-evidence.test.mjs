import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { validateClaimManifest, validateRepositoryClaims, main } from './check-spec-evidence.mjs'

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

test('rejects_symlinked_or_oversized_evidence_artifacts', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  const evidence = path.join(ctx.dir, 'evidence', 'result.json')
  const target = path.join(ctx.dir, 'evidence', 'target.json')
  writeFileSync(target, '{}')
  rmSync(evidence)
  symlinkSync(target, evidence)
  assert.match(validateClaimManifest(record, ctx.root).join('\n'), /artifact-symlink/)

  rmSync(evidence)
  writeFileSync(evidence, 'x'.repeat(8 * 1024 * 1024 + 1))
  record.artifacts[0] = {
    path: record.artifacts[0].path,
    bytes: 8 * 1024 * 1024 + 1,
    sha256: hash('x'.repeat(8 * 1024 * 1024 + 1)),
  }
  assert.match(validateClaimManifest(record, ctx.root).join('\n'), /artifact-byte-limit/)
})

test('rejects_artifact_inventory_exhaustion_and_sensitive_provenance', () => {
  const ctx = fixture()
  const record = doneManifest(ctx)
  record.artifacts = Array.from({ length: 129 }, () => ({ ...record.artifacts[0] }))
  assert.match(validateClaimManifest(record, ctx.root).join('\n'), /artifact-count-limit/)

  const sensitive = doneManifest(ctx)
  sensitive.live.action.command = 'run --token private-value'
  const findings = validateClaimManifest(sensitive, ctx.root).join('\n')
  assert.match(findings, /sensitive-content/)
  assert.doesNotMatch(findings, /private-value/)
})

test('rejects_traversal_and_malformed_repository_manifests', () => {
  const ctx = fixture()
  const traversal = doneManifest(ctx)
  traversal.artifacts[0].path = '../outside.json'
  assert.match(validateClaimManifest(traversal, ctx.root).join('\n'), /artifact-path/)

  writeFileSync(path.join(ctx.dir, 'evidence-manifest.json'), '{not-json')
  assert.match(validateRepositoryClaims({ root: ctx.root }).findings.join('\n'), /manifest-parse/)
})

test('test_ci_tooling_spec_missing_rejects', () => {
  const ctx = fixture()
  rmSync(path.join(ctx.dir, 'SPEC.md'))
  const result = validateRepositoryClaims({ root: ctx.root })
  assert.match(result.findings.join('\n'), /spec-missing/)
})

test('test_ci_tooling_evidence_dir_missing_rejects', () => {
  const ctx = fixture()
  rmSync(path.join(ctx.dir, 'evidence', 'result.json'))
  rmSync(path.join(ctx.dir, 'evidence'), { recursive: true, force: true })
  const result = validateRepositoryClaims({ root: ctx.root })
  assert.match(result.findings.join('\n'), /evidence-dir-missing/)
})

test('test_ci_tooling_invalid_root_rejects', () => {
  assert.match(validateRepositoryClaims({ root: '--invalid' }).findings.join('\n'), /invalid-root/)
  assert.match(validateRepositoryClaims({ root: '' }).findings.join('\n'), /invalid-root/)
  assert.match(validateRepositoryClaims({ root: null }).findings.join('\n'), /invalid-root/)
})

test('test_ci_tooling_specs_dir_missing_rejects', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-spec-evidence-empty-'))
  const result = validateRepositoryClaims({ root })
  assert.match(result.findings.join('\n'), /specs-dir-missing/)
})

test('test_ci_tooling_broken_evidence_link_rejects', () => {
  const ctx = fixture()
  const evidence = path.join(ctx.dir, 'evidence', 'result.json')
  const target = path.join(ctx.dir, 'evidence', 'target.json')
  writeFileSync(target, '{}')
  rmSync(evidence)
  symlinkSync(target, evidence)
  rmSync(target)
  const manifest = path.join(ctx.dir, 'evidence-manifest.json')
  symlinkSync(target, manifest)
  const result = validateRepositoryClaims({ root: ctx.root })
  assert.match(result.findings.join('\n'), /broken-evidence-link/)
})

test('test_ci_tooling_specs_dir_error_rejects', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-spec-evidence-err-'))
  mkdirSync(path.join(root, 'docs'))
  writeFileSync(path.join(root, 'docs', 'specs'), '')
  const result = validateRepositoryClaims({ root })
  assert.match(result.findings.join('\n'), /specs-dir-missing/)
})

test('test_ci_tooling_main_help', () => {
  const originalArgv = process.argv;
  const originalError = console.error;
  let errorMsg = '';
  process.argv = ['node', 'script.js', '--help'];
  console.error = (msg) => { errorMsg += msg; };
  const code = main();
  process.argv = originalArgv;
  console.error = originalError;
  assert.equal(code, 64);
  assert.match(errorMsg, /usage/);
})

test('test_ci_tooling_main_error', () => {
  const originalArgv = process.argv;
  const originalError = console.error;
  let errorMsg = '';
  process.argv = ['node', 'script.js', '--check'];
  console.error = (msg) => { errorMsg += msg; };
  process.argv = originalArgv;
  console.error = originalError;
})

test('test_ci_tooling_main_success', () => {
  const originalArgv = process.argv;
  const originalExit = process.exit;
  const originalError = console.error;
  const originalLog = console.log;
  let exitCode;
  let errorMsg = '';
  let logMsg = '';
  process.argv = ['node', 'tools/ci/check-spec-evidence.mjs', '--check'];
  process.exit = (code) => { exitCode = code; };
  console.error = (msg) => { errorMsg += msg; };
  console.log = (msg) => { logMsg += msg; };

  const code = main();

  process.argv = originalArgv;
  process.exit = originalExit;
  console.error = originalError;
  console.log = originalLog;
  assert.equal(code, 1);
})

test('test_ci_tooling_main_execution_mock', () => {
  const originalArgv = process.argv;
  const originalExit = process.exit;
  const originalError = console.error;
  let exitCode;
  let errorMsg = '';
  process.argv = ['node', 'tools/ci/check-spec-evidence.mjs', '--invalid'];
  process.exit = (code) => { exitCode = code; };
  console.error = (msg) => { errorMsg += msg; };

  const code = main();

  process.argv = originalArgv;
  process.exit = originalExit;
  console.error = originalError;
  assert.equal(code, 64);
})

test('test_ci_tooling_main_not_ok', () => {
  const originalArgv = process.argv;
  const originalExit = process.exit;
  const originalError = console.error;
  let exitCode;
  let errorMsg = '';
  process.argv = ['node', 'tools/ci/check-spec-evidence.mjs', '--check'];
  process.exit = (code) => { exitCode = code; };
  console.error = (msg) => { errorMsg += msg; };

  const code = main();

  process.argv = originalArgv;
  process.exit = originalExit;
  console.error = originalError;
  assert.equal(code, 1);
  assert.match(errorMsg, /spec-evidence —/);
})
