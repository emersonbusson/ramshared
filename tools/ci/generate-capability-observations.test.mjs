import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  buildCapabilityObservations,
  renderCapabilityObservations,
  validateObservationPolicy,
} from './generate-capability-observations.mjs'

function fixture() {
  const root = mkdtempSync(path.join(os.tmpdir(), 'ramshared-capability-observations-'))
  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  mkdirSync(path.join(root, 'docs', 'specs', 'no-milestone', 'alpha'), { recursive: true })
  mkdirSync(path.join(root, 'docs', 'specs', 'no-milestone', 'beta'), { recursive: true })
  mkdirSync(path.join(root, 'src'), { recursive: true })
  mkdirSync(path.join(root, 'tests'), { recursive: true })
  writeFileSync(path.join(root, 'src', 'alpha.rs'), 'pub fn alpha() {}\n')
  writeFileSync(path.join(root, 'tests', 'alpha_test.rs'), '#[test] fn alpha_test() {}\n')
  writeFileSync(path.join(root, 'docs', 'specs', 'no-milestone', 'alpha', 'PRD.md'), '# Alpha\n')
  writeFileSync(path.join(root, 'docs', 'specs', 'no-milestone', 'alpha', 'SPEC.md'), '# Spec\n')
  writeFileSync(path.join(root, 'docs', 'specs', 'no-milestone', 'alpha', 'IMPL.md'), '# Impl\n\nSource: `src/alpha.rs`\nTest: `tests/alpha_test.rs`\n')
  writeFileSync(path.join(root, 'docs', 'specs', 'no-milestone', 'beta', 'PRD.md'), '# Beta\n\nIgnored: `../../outside.rs`\n')
  writeFileSync(path.join(root, 'docs', 'governance', 'claims.json'), JSON.stringify({
    schema_version: 1,
    claims: [{ slug: 'alpha', state: 'DONE', owner_role: 'fixture' }],
  }, null, 2) + '\n')
  const policy = {
    schema_version: 1,
    catalog_path: 'docs/governance/capability-observations.generated.json',
    spec_root: 'docs/specs/no-milestone',
    max_specs: 8,
    max_document_bytes: 8192,
    allowed_surface_prefixes: ['src', 'tests', 'tools', 'crates', 'windows'],
  }
  writeFileSync(path.join(root, 'docs', 'governance', 'capability-observation-policy.json'), JSON.stringify(policy, null, 2) + '\n')
  return { root, policy }
}

test('observations_catalog_documented_spec_implementation_and_test_surfaces_without_promotion', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))

  const result = buildCapabilityObservations(ctx.root, ctx.policy)
  assert.deepEqual(result.findings, [])
  assert.deepEqual(result.catalog.observations.map((item) => item.slug), ['alpha', 'beta'])
  const alpha = result.catalog.observations[0]
  assert.deepEqual(alpha.documents, {
    prd: 'docs/specs/no-milestone/alpha/PRD.md',
    spec: 'docs/specs/no-milestone/alpha/SPEC.md',
    implementation: 'docs/specs/no-milestone/alpha/IMPL.md',
  })
  assert.deepEqual(alpha.documented_surface.implementation_paths, ['src/alpha.rs'])
  assert.deepEqual(alpha.documented_surface.test_paths, ['tests/alpha_test.rs'])
  assert.equal(alpha.observation_state, 'OBSERVED')
  assert.equal(alpha.claim_reconciliation.registry_state, 'DONE')
  assert.equal(alpha.claim_reconciliation.observation_is_not_a_claim, true)
  assert.equal(alpha.promotion.permitted, false)
  assert.deepEqual(result.catalog.observations[1].documented_surface.implementation_paths, [])
})

test('capability_observations_ignore_empty_untracked_spec_directory', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))
  mkdirSync(path.join(ctx.root, 'docs', 'specs', 'no-milestone', 'empty-local-slug'))

  const result = buildCapabilityObservations(ctx.root, ctx.policy)

  assert.deepEqual(result.findings, [])
  assert.deepEqual(result.catalog.observations.map((item) => item.slug), ['alpha', 'beta'])
})

test('capability_observations_preserve_readme_only_historical_surface', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))
  const historical = path.join(ctx.root, 'docs', 'specs', 'no-milestone', 'historical-readme')
  mkdirSync(historical)
  writeFileSync(path.join(historical, 'README.md'), '# Historical surface\n')

  const result = buildCapabilityObservations(ctx.root, ctx.policy)

  assert.deepEqual(result.findings, [])
  assert.deepEqual(result.catalog.observations.map((item) => item.slug), ['alpha', 'beta', 'historical-readme'])
  assert.deepEqual(result.catalog.observations[2].documents, {
    prd: null,
    spec: null,
    implementation: null,
  })
})

test('capability_observations_refuse_dangling_named_document_symlink', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))
  const broken = path.join(ctx.root, 'docs', 'specs', 'no-milestone', 'broken-document')
  mkdirSync(broken)
  symlinkSync(path.join(ctx.root, 'missing-spec.md'), path.join(broken, 'SPEC.md'))

  const result = buildCapabilityObservations(ctx.root, ctx.policy)

  assert.deepEqual(result.catalog.observations.map((item) => item.slug), ['alpha', 'beta', 'broken-document'])
  assert.deepEqual(result.findings, [{
    rule: 'document-unsafe',
    detail: 'docs/specs/no-milestone/broken-document/SPEC.md',
  }])
})

test('observation_policy_and_discovery_fail_closed_on_unsafe_or_unbounded_input', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))

  assert.match(JSON.stringify(validateObservationPolicy({ ...ctx.policy, spec_root: '../docs' })), /spec-root/)
  const result = buildCapabilityObservations(ctx.root, { ...ctx.policy, max_specs: 1 })
  assert.match(JSON.stringify(result.findings), /spec-limit/)
})

test('observations_refuse_unsafe_documents_oversized_inputs_and_invalid_claim_registries', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))

  const alpha = path.join(ctx.root, 'docs', 'specs', 'no-milestone', 'alpha')
  unlinkSync(path.join(alpha, 'SPEC.md'))
  symlinkSync(path.join(ctx.root, 'src', 'alpha.rs'), path.join(alpha, 'SPEC.md'))
  writeFileSync(path.join(alpha, 'PRD.md'), 'x'.repeat(ctx.policy.max_document_bytes + 1))
  writeFileSync(path.join(ctx.root, 'docs', 'governance', 'claims.json'), JSON.stringify({
    schema_version: 1,
    claims: [{ slug: 'alpha', state: 'DONE' }, { slug: 'alpha', state: 'BAD' }],
  }))

  const result = buildCapabilityObservations(ctx.root)
  assert.match(JSON.stringify(result.findings), /document-unsafe/)
  assert.match(JSON.stringify(result.findings), /document-byte-limit/)
  assert.match(JSON.stringify(result.findings), /claims-schema/)
  assert.equal(result.catalog.observations[0].documents.prd, null)
  assert.equal(result.catalog.observations[0].documents.spec, null)
})

test('missing_or_malformed_governance_inputs_remain_nonpromoting_and_report_findings', (t) => {
  const emptyRoot = mkdtempSync(path.join(os.tmpdir(), 'ramshared-capability-empty-'))
  t.after(() => rmSync(emptyRoot, { recursive: true, force: true }))
  assert.match(JSON.stringify(buildCapabilityObservations(emptyRoot).findings), /policy-read/)

  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))
  assert.match(JSON.stringify(validateObservationPolicy({
    schema_version: 0,
    catalog_path: '/absolute.json',
    spec_root: 'docs/specs/no-milestone',
    max_specs: 0,
    max_document_bytes: 1,
    allowed_surface_prefixes: ['src/nested'],
  })), /policy-schema|catalog-path|spec-limit|document-byte-limit|surface-prefixes/)
  assert.match(JSON.stringify(buildCapabilityObservations(ctx.root, {
    ...ctx.policy,
    spec_root: 'docs/specs/missing',
  }).findings), /spec-root-missing/)
  writeFileSync(path.join(ctx.root, 'docs', 'governance', 'claims.json'), '{')
  const malformedClaims = buildCapabilityObservations(ctx.root)
  assert.match(JSON.stringify(malformedClaims.findings), /claims-registry/)
  assert.equal(malformedClaims.catalog.observations[0].claim_reconciliation.registry_state, null)
})

test('rendering_is_deterministic_and_never_writes_without_explicit_write_mode', (t) => {
  const ctx = fixture()
  t.after(() => rmSync(ctx.root, { recursive: true, force: true }))

  const first = renderCapabilityObservations(buildCapabilityObservations(ctx.root, ctx.policy).catalog)
  const second = renderCapabilityObservations(buildCapabilityObservations(ctx.root, ctx.policy).catalog)
  assert.equal(first, second)
  assert.equal(readFileSync(path.join(ctx.root, 'docs', 'governance', 'capability-observation-policy.json'), 'utf8').includes('schema_version'), true)
})
