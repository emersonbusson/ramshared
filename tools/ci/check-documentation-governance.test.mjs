import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  findDuplicateNormativeBlocks,
  scanProvenance,
  validateClaims,
  validateJourneyManifest,
  validateParityDocument,
  validatePostmortemEffectiveness,
  validateReferenceIndex,
  run as runGovernance,
} from './check-documentation-governance.mjs'
import { deriveStatus } from '../generate-docs-index.mjs'

function rootFixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-doc-governance-'))
  const files = [
    'ARCHITECTURE.md', 'README.md', 'validation.md', 'docs/SSDV3-PROMPTS.md',
    'docs/BENCHMARKS.md', 'docs/reliability/GAP-REGISTER.md',
    'docs/postmortems/TEMPLATE.md', 'docs/runbooks/windows-autonomous-broker.md',
    '.claude/rules/coding.md', '.claude/rules/ssdv3.md', '.claude/rules/benchmarks.md',
    'scripts/safety/README.md', 'scripts/docs-check.sh',
  ]
  for (const rel of files) {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true })
    writeFileSync(path.join(root, rel), rel === 'validation.md' ? '**Verdict:** ✅\n' : '# Fixture\n')
  }
  return root
}

function parityText() {
  const rows = [
    ['Architecture and topology', 'ARCHITECTURE.md'],
    ['Capability state', 'docs/governance/claims.json'],
    ['PRD and SPEC requirements', 'docs/SSDV3-PROMPTS.md'],
    ['Operation', 'docs/runbooks/windows-autonomous-broker.md'],
    ['Empirical validation', 'validation.md'],
    ['Benchmark comparison', 'docs/BENCHMARKS.md'],
    ['Reliability gaps', 'docs/reliability/GAP-REGISTER.md'],
    ['Postmortem closure', 'docs/postmortems/TEMPLATE.md'],
  ]
  return '| Objective | Canonical source | Owner role | Evidence source | State semantics | Known limitation |\n| --- | --- | --- | --- | --- | --- |\n' + rows.map(([a, b]) => `| ${a} | \`${b}\` | owner | \`validation.md\` | explicit | legacy |`).join('\n')
}

function referenceText() {
  const rows = [
    ['build and test', 'README.md'], ['Linux or WSL2 lifecycle', 'docs/runbooks/windows-autonomous-broker.md'],
    ['Windows lifecycle', 'docs/runbooks/windows-autonomous-broker.md'], ['host-safety campaigns', 'scripts/safety/README.md'],
    ['benchmarks', 'docs/BENCHMARKS.md'], ['reliability gaps', 'docs/reliability/GAP-REGISTER.md'],
    ['architecture decisions', 'ARCHITECTURE.md'], ['SSDV3 changes', 'docs/SSDV3-PROMPTS.md'],
    ['investigate and close an incident', 'docs/postmortems/TEMPLATE.md'],
  ]
  return '| Question | Canonical source | Next-depth references | Boundary |\n| --- | --- | --- | --- |\n' + rows.map(([a, b]) => `| ${a} | \`${b}\` | \`README.md\` | boundary |`).join('\n')
}

function validClaim(root, state = 'DONE') {
  const spec = 'docs/specs/no-milestone/fixture/SPEC.md'
  const impl = 'docs/specs/no-milestone/fixture/IMPL.md'
  const testPath = 'tools/fixture.test.mjs'
  const evidence = 'docs/specs/no-milestone/fixture/evidence.json'
  for (const [rel, text] of [[spec, '# SPEC'], [impl, '# IMPL'], [testPath, "test('fixture_test',()=>{})"], [evidence, '{}']]) {
    mkdirSync(path.dirname(path.join(root, rel)), { recursive: true })
    writeFileSync(path.join(root, rel), text)
  }
  return {
    slug: 'fixture', state, owner_role: 'owner', canonical_spec: spec,
    implementation_paths: [impl], named_tests: [{ path: testPath, name: 'fixture_test' }],
    cover: { mode: 'node-built-in', minimum_percent: 80, evidence_path: evidence },
    validation: { record_path: 'validation.md', verdict: '✅', source_commit: '1'.repeat(40), evidence_paths: [evidence] },
    binary_match_required: false, environment_blocker: null, missing_gate: null,
    next_proof: null, rollback_trigger: 'one observable failure',
  }
}

test('parity_matrix_has_one_canonical_source_per_category', () => {
  const root = rootFixture(); mkdirSync(path.join(root, 'docs/governance'), { recursive: true }); writeFileSync(path.join(root, 'docs/governance/claims.json'), '{}')
  assert.deepEqual(validateParityDocument(parityText(), root), [])
})
test('parity_rejects_duplicate_objective', () => { const root = rootFixture(); assert.match(JSON.stringify(validateParityDocument(`${parityText()}\n| Architecture and topology | \`README.md\` | owner | \`validation.md\` | x | x |`, root)), /duplicate-objective/) })
test('parity_rejects_missing_target', () => { const root = rootFixture(); assert.match(JSON.stringify(validateParityDocument(parityText().replace('ARCHITECTURE.md', 'docs/missing.md'), root)), /missing-target/) })
test('reference_index_routes_required_objectives', () => { const root = rootFixture(); assert.deepEqual(validateReferenceIndex(referenceText(), root), []) })
test('reference_index_rejects_private_or_missing_target', () => { const root = rootFixture(); const privatePath = ['', 'home', 'private', 'repo.md'].join('/'); assert.match(JSON.stringify(validateReferenceIndex(referenceText().replace('README.md', privatePath), root)), /unsafe-target/) })

test('claims_done_requires_all_evidence', () => { const root = rootFixture(); const claim = validClaim(root); assert.deepEqual(validateClaims({ schema_version: 1, claims: [claim] }, root), []) })
test('claims_partial_requires_blocker_and_next_proof', () => { const root = rootFixture(); const claim = validClaim(root, 'PARTIAL'); assert.match(JSON.stringify(validateClaims({ schema_version: 1, claims: [claim] }, root)), /partial-blocker/) })
test('claims_rejects_missing_owner_or_duplicate_slug', () => { const root = rootFixture(); const claim = validClaim(root); claim.owner_role = ''; assert.match(JSON.stringify(validateClaims({ schema_version: 1, claims: [claim, claim] }, root)), /owner|duplicate-slug/) })
test('claims_rejects_stale_artifact_and_missing_binary_match', () => { const root = rootFixture(); const claim = validClaim(root); claim.binary_match_required = true; claim.validation.evidence_paths = ['docs/missing.json']; assert.match(JSON.stringify(validateClaims({ schema_version: 1, claims: [claim] }, root)), /missing-path|binary-match/) })
test('claims_rejects_unknown_state_and_placeholder_rollback', () => { const root = rootFixture(); const claim = validClaim(root); claim.state = 'GREEN'; claim.rollback_trigger = 'TBD'; assert.match(JSON.stringify(validateClaims({ schema_version: 1, claims: [claim] }, root)), /claim-state/); assert.match(JSON.stringify(validateClaims({ schema_version: 1, claims: [claim] }, root)), /rollback-trigger/) })

test('unqualified_impl_is_not_done', () => { const root = rootFixture(); const dir = path.join(root, 'docs/specs/no-milestone/unclaimed'); mkdirSync(dir, { recursive: true }); writeFileSync(path.join(dir, 'PRD.md'), '---\nslug: unclaimed\n---'); writeFileSync(path.join(dir, 'SPEC.md'), '# SPEC'); writeFileSync(path.join(dir, 'IMPL.md'), '# IMPL'); assert.equal(deriveStatus(dir, new Map()), 'UNQUALIFIED') })
test('qualified_claim_is_the_only_done_path', () => { const root = rootFixture(); const dir = path.join(root, 'docs/specs/no-milestone/fixture'); mkdirSync(dir, { recursive: true }); writeFileSync(path.join(dir, 'IMPL.md'), '# IMPL'); assert.equal(deriveStatus(dir, new Map([['fixture', { state: 'DONE' }]])), 'DONE') })

test('provenance_rejects_private_path_without_echoing_match', () => { const privateValue = '/home/' + 'private-user/repo'; const out = scanProvenance([{ path: 'docs/a.md', text: `path ${privateValue}` }], { entries: [] }, {}); assert.match(JSON.stringify(out), /PRIVATE_PATH/); assert.doesNotMatch(JSON.stringify(out), new RegExp(privateValue)) })
test('provenance_rejects_secret_without_echoing_match', () => { const secret = 'api_' + 'secret_value'; const out = scanProvenance([{ path: 'docs/a.md', text: `password=${secret}` }], { entries: [] }, {}); assert.match(JSON.stringify(out), /SECRET/); assert.doesNotMatch(JSON.stringify(out), new RegExp(secret)) })
test('provenance_allows_sanitized_fixture', () => { assert.deepEqual(scanProvenance([{ path: 'docs/a.md', text: 'password=<REDACTED>' }], { entries: [] }, {}), []) })
test('provenance_rejects_private_key_email_and_kernel_address', () => { const text = [['-----BEGIN ', 'PRIVATE KEY-----'].join(''), ['owner', 'example.invalid'].join('@'), ['ffff', '888012345678'].join('')].join('\n'); const output = JSON.stringify(scanProvenance([{ path: 'docs/a.md', text }], { entries: [] }, {})); assert.match(output, /PRIVATE_KEY/); assert.match(output, /EMAIL/); assert.match(output, /KERNEL_ADDRESS/) })
test('allowlist_requires_owner_review_and_expiry', () => { const out = scanProvenance([], { entries: [{ rule: 'EXTERNAL', scope: ['docs/a.md'] }] }, {}); assert.match(JSON.stringify(out), /ALLOWLIST_SHAPE/) })
test('allowlist_cannot_cover_an_entire_directory', () => { const out = scanProvenance([], { entries: [{ id: 'x', rule: 'EXTERNAL', pattern: 'x', scope: ['docs/'], reason: 'fixture', owner_role: 'owner', review_by: '2026-08-10', expires: '2026-09-01' }] }, {}); assert.match(JSON.stringify(out), /ALLOWLIST_SCOPE/) })
test('baseline_entry_requires_content_hash_and_expiry', () => { const out = scanProvenance([], { entries: [] }, { entries: [{ path: 'docs/a.md' }] }); assert.match(JSON.stringify(out), /BASELINE_SHAPE/) })
test('changed_file_cannot_use_legacy_baseline', () => { const privatePath = ['', 'home', 'private', 'x'].join('/'); const out = scanProvenance([{ path: 'docs/a.md', text: privatePath }], { entries: [] }, { entries: [{ path: 'docs/a.md', rule: 'PRIVATE_PATH', fingerprint: 'x', file_sha256: '0'.repeat(64), reason: 'old', owner_role: 'owner', review_by: '2026-08-10', expires: '2026-09-01' }] }); assert.match(JSON.stringify(out), /PRIVATE_PATH/) })

test('redundancy_reports_long_duplicate_normative_block', () => { const block = 'Operators must preserve the exact supported cleanup and evidence ordering for every bounded run. '.repeat(8); assert.match(JSON.stringify(findDuplicateNormativeBlocks([{ path: 'docs/a.md', text: block }, { path: 'docs/b.md', text: block }], {})), /DUPLICATE_NORMATIVE/) })
test('redundancy_does_not_rewrite_fixture_bytes', () => { const files = [{ path: 'docs/a.md', text: 'short content' }]; const before = JSON.stringify(files); findDuplicateNormativeBlocks(files, {}); assert.equal(JSON.stringify(files), before) })

function journey() { return { schema_version: 1, journey_id: 'fixture', version: 1, run_id: 'fixture-run', profile: 'smoke', target_layer: 'documentation', seed: 1, clock_policy: 'monotonic', timeout_seconds: 30, parameters: { n: 1 }, runner_refs: ['scripts/docs-check.sh'], actions: [{ id: 'check', runner_ref: 'scripts/docs-check.sh', wait_strategy: 'process-exit', timeout_seconds: 30 }], checkpoints: { before: 'state', action: 'exit', after: 'state' }, legitimate_case: { name: 'clean', expected_exit: 0 }, refusal_cases: [{ name: 'bad', expected_exit: 1, terminal_state: 'NO-GO' }, { name: 'usage', expected_exit: 2, terminal_state: 'NO-GO' }], invariants: ['no-sensitive-output', 'no-host-mutation', 'deterministic-ordering', 'idempotent-cleanup'], reporters: ['status'], artifacts: ['tmp/fixture/'], cleanup: { idempotent: true, mode: 'remove-temporary-report' }, consumer_paths: ['scripts/docs-check.sh'], verdict: 'PASS', rollback_trigger: 'one false pass' } }
test('journey_manifest_accepts_seeded_bounded_run', () => { const root = rootFixture(); assert.deepEqual(validateJourneyManifest(journey(), root), []) })
test('journey_manifest_rejects_implicit_clock_sleep_only_and_missing_cleanup', () => { const root = rootFixture(); const j = journey(); delete j.clock_policy; j.actions[0].wait_strategy = 'sleep'; delete j.cleanup; assert.match(validateJourneyManifest(j, root).join('\n'), /clock|wait|cleanup/) })
test('journey_manifest_rejects_duplicate_version_missing_consumer', () => { const root = rootFixture(); const j = journey(); j.version = 0; j.consumer_paths = []; assert.match(validateJourneyManifest(j, root).join('\n'), /version|consumer/) })
test('journey_manifest_rejects_bad_schema_seed_timeout_and_paths', () => { const root = rootFixture(); const j = journey(); j.schema_version = 2; j.seed = -1; j.timeout_seconds = 0; delete j.checkpoints.after; j.runner_refs = ['/private/runner']; j.artifacts = ['/private/artifact']; const output = validateJourneyManifest(j, root).join('\n'); assert.match(output, /schema/); assert.match(output, /seed/); assert.match(output, /timeout/); assert.match(output, /checkpoints/); assert.match(output, /consumer-path/); assert.match(output, /artifact-path/) })

test('postmortem_effectiveness_requires_regression_threshold_and_revalidation', () => { const text = '**Governance schema:** 1\n**Closure state:** effective\n'; assert.match(validatePostmortemEffectiveness(text, 'docs/postmortems/x.md').join('\n'), /regression|threshold|revalidation/) })
test('postmortem_open_action_cannot_close', () => { const text = '**Governance schema:** 1\n**Closure state:** open\n'; assert.deepEqual(validatePostmortemEffectiveness(text, 'docs/postmortems/x.md'), []) })
test('postmortem_effective_action_requires_both_paths', () => { const text = '**Governance schema:** 1\n**Closure state:** effective\n**Regression command:** tools/test.mjs\n**Threshold:** 0 failures\n**Revalidation:** legitimate only\n**Observed result:** 0 failures\n**Evidence:** validation.md\n'; assert.match(validatePostmortemEffectiveness(text, 'docs/postmortems/x.md').join('\n'), /critical-refusal/) })

test('governance_check_is_deterministic', () => { const privatePath = ['', 'home', 'private', 'x'].join('/'); const files = [{ path: 'docs/z.md', text: privatePath }, { path: 'docs/a.md', text: 'ok' }]; assert.equal(JSON.stringify(scanProvenance(files, { entries: [] }, {})), JSON.stringify(scanProvenance([...files].reverse(), { entries: [] }, {}))) })
test('repository_governance_run_passes', () => { const result = runGovernance({ root: process.cwd() }); assert.equal(result.ok, true); assert.equal(result.counts.findings, 0); assert.ok(result.counts.files > 100) })
test('governance_cli_is_read_only', () => { const src = readFileSync(new URL('./check-documentation-governance.mjs', import.meta.url), 'utf8'); assert.doesNotMatch(src, /\b(?:writeFile|rmSync|unlinkSync|execSync|spawnSync)\b/) })
test('docs_check_includes_governance_gate', () => { const text = readFileSync(new URL('../../scripts/docs-check.sh', import.meta.url), 'utf8'); assert.match(text, /check-documentation-governance\.mjs --all/) })
test('docs_check_is_read_only', () => { const text = readFileSync(new URL('../../scripts/docs-check.sh', import.meta.url), 'utf8'); assert.doesNotMatch(text, /\b(?:rm|swapoff|sc\.exe|shutdown|Restart-Computer)\b/) })
