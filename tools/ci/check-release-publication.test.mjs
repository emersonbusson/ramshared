import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { buildReleaseManifest } from './write-release-manifest.mjs'
import {
  candidateFromReleaseManifest,
  main,
  planPublication,
  validatePublicationBinding,
  validatePublicationCandidate,
  validatePublicationInput,
  validateReleasePromotionPolicy,
} from './check-release-publication.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const TARGET_TAG = 'v0.9.0-beta.1'
const SOURCE_SHA = '0123456789abcdef0123456789abcdef01234567'
const PUBLIC_ASSETS = [
  `ramshared-linux-${TARGET_TAG}.tar.gz`,
  `ramshared-linux-${TARGET_TAG}.tar.gz.sha256`,
  'ramshared-sbom.cdx.json',
  'release-manifest.json',
]

const POLICY = {
  schema_version: 1,
  target_tag: TARGET_TAG,
  release_channel: 'beta',
  public_assets: PUBLIC_ASSETS,
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function releaseCandidateFixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-publication-'))
  const releaseDir = path.join(root, 'artifacts', 'release')
  mkdirSync(releaseDir, { recursive: true })
  const archive = `ramshared-linux-${TARGET_TAG}.tar.gz`
  const archiveBytes = 'archive\n'
  writeFileSync(path.join(root, 'Cargo.lock'), 'version = 4\n')
  writeFileSync(path.join(releaseDir, archive), archiveBytes)
  writeFileSync(path.join(releaseDir, `${archive}.sha256`), `${sha256(archiveBytes)}  ${archive}\n`)
  writeFileSync(path.join(releaseDir, 'ramshared-sbom.cdx.json'), `${JSON.stringify({
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    metadata: {
      tools: [{ name: 'cargo-cyclonedx', version: '0.5.9' }],
      component: {
        type: 'application', name: 'ramshared', version: '0.9.0-beta.1',
        components: [{ name: 'ramshared-cli' }, { name: 'ramshared-wsl2d' }],
      },
      properties: [
        { name: 'ramshared:release:tag', value: TARGET_TAG },
        { name: 'ramshared:source:revision', value: SOURCE_SHA },
      ],
    },
  })}\n`)
  const manifest = buildReleaseManifest({
    tag: TARGET_TAG,
    revision: SOURCE_SHA,
    clean_tree: true,
    rust_version: '1.98.0',
    rust_commit: 'a'.repeat(40),
    bundle_path: `artifacts/release/${archive}`,
    checksum_path: `artifacts/release/${archive}.sha256`,
    sbom_path: 'artifacts/release/ramshared-sbom.cdx.json',
    prior_release: 'none',
    rollback_trigger: 'checksum mismatch',
  }, { root })
  const manifestPath = path.join(releaseDir, 'release-manifest.json')
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  writeFileSync(path.join(root, 'policy.json'), `${JSON.stringify(POLICY)}\n`)
  return { root, manifest, manifestPath }
}

function candidate() {
  return {
    source: { tag: TARGET_TAG, sha: SOURCE_SHA },
    public_assets: PUBLIC_ASSETS.map((name, index) => ({
      name,
      bytes: index + 1,
      sha256: String(index + 1).repeat(64),
    })),
  }
}

test('publication_input_rejects_historical_or_non_exact_identity', () => {
  assert.deepEqual(validateReleasePromotionPolicy(POLICY), { ok: true, errors: [] })
  assert.deepEqual(validatePublicationInput({
    tag: TARGET_TAG,
    source_sha: SOURCE_SHA,
    integrity_run_id: '123456',
  }, POLICY), { ok: true, errors: [] })
  for (const input of [
    { tag: 'v0.8.0', source_sha: SOURCE_SHA, integrity_run_id: '123456' },
    { tag: TARGET_TAG, source_sha: 'main', integrity_run_id: '123456' },
    { tag: TARGET_TAG, source_sha: SOURCE_SHA, integrity_run_id: '0' },
  ]) {
    const result = validatePublicationInput(input, POLICY)
    assert.equal(result.ok, false)
  }
})

test('publication_policy_and_candidate_refuse_malformed_records', () => {
  assert.deepEqual(validateReleasePromotionPolicy({ ...POLICY, target_tag: 'v0.8.0' }), {
    ok: false,
    errors: ['release-promotion-policy-invalid'],
  })
  const malformed = validatePublicationCandidate(POLICY, {
    source: { tag: TARGET_TAG, sha: 'main' },
    public_assets: [{ name: PUBLIC_ASSETS[0], bytes: -1, sha256: 'nope' }],
  })
  assert.equal(malformed.ok, false)
  assert.equal(malformed.errors.includes('candidate-source-invalid'), true)
  assert.equal(malformed.errors.includes('candidate-assets-invalid'), true)
})

test('publication_validate_only_cli_stops_invalid_dispatch_before_credentials', () => {
  const output = []
  assert.equal(main([
    '--policy', 'docs/governance/release-promotion.json',
    '--tag', TARGET_TAG,
    '--source-sha', SOURCE_SHA,
    '--integrity-run-id', '123456',
  ], {
    cwd: ROOT,
    print: (line) => output.push(line),
    error: () => assert.fail('exact dispatch must not emit an error'),
  }), 0)
  assert.deepEqual(output, ['RELEASE_PUBLICATION_INPUT=PASS'])

  const errors = []
  assert.equal(main([
    '--policy', 'docs/governance/release-promotion.json',
    '--tag', 'v0.8.0',
    '--source-sha', SOURCE_SHA,
    '--integrity-run-id', '123456',
  ], {
    cwd: ROOT,
    print: () => assert.fail('historical dispatch must not pass'),
    error: (line) => errors.push(line),
  }), 1)
  assert.deepEqual(errors, ['RELEASE_PUBLICATION_ERROR=publication-input-invalid'])
})

test('publication_plan_accepts_exact_draft_candidate', () => {
  const result = planPublication(POLICY, candidate(), {
    is_draft: true,
    is_prerelease: true,
    assets: [],
  })
  assert.deepEqual(result, {
    status: 'ADVANCE',
    errors: [],
    create_draft: false,
    upload_assets: PUBLIC_ASSETS,
    publish: true,
  })
})

test('publication_accepts_branch_target_commitish_but_refuses_wrong_tag_or_dispatch_sha', () => {
  const local = candidate()
  const branchTarget = planPublication(POLICY, local, {
    draft: true,
    prerelease: true,
    tag_name: TARGET_TAG,
    target_commitish: 'main',
    assets: [],
  })
  assert.deepEqual(branchTarget, {
    status: 'ADVANCE',
    errors: [],
    create_draft: false,
    upload_assets: PUBLIC_ASSETS,
    publish: true,
  })

  const wrongTag = planPublication(POLICY, local, {
    draft: true,
    prerelease: true,
    tag_name: 'v0.8.0',
    target_commitish: 'main',
    assets: [],
  })
  assert.equal(wrongTag.status, 'NO_GO')
  assert.equal(wrongTag.errors.includes('remote-release-identity-invalid'), true)

  const wrongSource = structuredClone(local)
  wrongSource.source.sha = 'fedcba9876543210fedcba9876543210fedcba98'
  const binding = validatePublicationBinding({
    tag: TARGET_TAG,
    source_sha: SOURCE_SHA,
    integrity_run_id: '123456',
  }, wrongSource, POLICY)
  assert.equal(binding.ok, false)
  assert.equal(binding.errors.includes('candidate-dispatch-mismatch'), true)
})

test('publication_plan_is_idempotent_and_refuses_mismatched_remote_assets', () => {
  const local = candidate()
  const matching = {
    is_draft: false,
    is_prerelease: true,
    assets: local.public_assets,
  }
  assert.deepEqual(planPublication(POLICY, local, matching), {
    status: 'NO_CHANGE',
    errors: [],
    create_draft: false,
    upload_assets: [],
    publish: false,
  })

  const altered = structuredClone(matching)
  altered.assets[0].sha256 = 'f'.repeat(64)
  const mismatch = planPublication(POLICY, local, altered)
  assert.equal(mismatch.status, 'NO_GO')
  assert.equal(mismatch.errors.includes('remote-asset-mismatch'), true)

  const extra = structuredClone(matching)
  extra.assets.push({ name: 'SHA256SUMS', bytes: 1, sha256: 'e'.repeat(64) })
  const extraResult = planPublication(POLICY, local, extra)
  assert.equal(extraResult.status, 'NO_GO')
  assert.equal(extraResult.errors.includes('remote-assets-invalid'), true)
})

test('publication_planner_refuses_absent_invalid_or_incomplete_releases_and_normalizes_api_assets', () => {
  const local = candidate()
  assert.deepEqual(planPublication(POLICY, local, null), {
    status: 'NO_GO',
    errors: ['remote-release-absent'],
    create_draft: false,
    upload_assets: [],
    publish: false,
  })

  const invalidMode = planPublication(POLICY, local, {
    is_draft: 'yes',
    is_prerelease: false,
    assets: 'not-an-array',
  })
  assert.equal(invalidMode.status, 'NO_GO')
  assert.equal(invalidMode.errors.includes('remote-release-mode-invalid'), true)
  assert.equal(invalidMode.errors.includes('remote-assets-invalid'), true)

  const publishedMissing = planPublication(POLICY, local, {
    is_draft: false,
    is_prerelease: true,
    assets: [],
  })
  assert.deepEqual(publishedMissing, {
    status: 'NO_GO',
    errors: ['remote-release-mode-invalid'],
    create_draft: false,
    upload_assets: [],
    publish: false,
  })

  const githubResponse = planPublication(POLICY, local, {
    draft: false,
    prerelease: true,
    tag_name: TARGET_TAG,
    target_commitish: 'main',
    assets: local.public_assets.map((asset) => ({
      name: asset.name,
      size: asset.bytes,
      digest: `sha256:${asset.sha256}`,
    })),
  })
  assert.equal(githubResponse.status, 'NO_CHANGE')

  const malformedAsset = planPublication(POLICY, local, {
    draft: true,
    prerelease: true,
    tag_name: TARGET_TAG,
    assets: [{ name: PUBLIC_ASSETS[0], size: 1, digest: 'unverified' }],
  })
  assert.equal(malformedAsset.status, 'NO_GO')
  assert.equal(malformedAsset.errors.includes('remote-assets-invalid'), true)
})

test('publication_candidate_and_plan_cli_bind_files_and_fail_closed_on_paths', () => {
  const fixture = releaseCandidateFixture()
  const direct = candidateFromReleaseManifest(fixture.manifest, { root: fixture.root })
  assert.deepEqual(direct.source, { tag: TARGET_TAG, sha: SOURCE_SHA })
  assert.deepEqual(direct.public_assets.map((record) => record.name), PUBLIC_ASSETS)

  const output = []
  assert.equal(main([
    '--policy', 'policy.json',
    '--manifest', 'artifacts/release/release-manifest.json',
    '--root', '.',
    '--tag', TARGET_TAG,
    '--source-sha', SOURCE_SHA,
    '--integrity-run-id', '123456',
    '--out', 'candidate.json',
  ], {
    cwd: fixture.root,
    print: (line) => output.push(line),
    error: () => assert.fail('verified candidate must not emit an error'),
  }), 0)
  assert.deepEqual(output, ['RELEASE_PUBLICATION_CANDIDATE=PASS'])
  const candidateJson = JSON.parse(readFileSync(path.join(fixture.root, 'candidate.json'), 'utf8'))
  const remote = {
    draft: true,
    prerelease: true,
    tag_name: TARGET_TAG,
    target_commitish: 'main',
    assets: [],
  }
  writeFileSync(path.join(fixture.root, 'remote.json'), `${JSON.stringify(remote)}\n`)
  assert.equal(main([
    '--policy', 'policy.json',
    '--candidate', 'candidate.json',
    '--remote', 'remote.json',
    '--out', 'plan.json',
  ], {
    cwd: fixture.root,
    print: (line) => output.push(line),
    error: () => assert.fail('draft plan must not emit an error'),
  }), 0)
  assert.deepEqual(JSON.parse(readFileSync(path.join(fixture.root, 'plan.json'), 'utf8')), {
    status: 'ADVANCE',
    errors: [],
    create_draft: false,
    upload_assets: candidateJson.public_assets.map((asset) => asset.name),
    publish: true,
  })

  const errors = []
  assert.equal(main(['--policy', '../policy.json', '--tag', TARGET_TAG, '--source-sha', SOURCE_SHA, '--integrity-run-id', '123456'], {
    cwd: fixture.root,
    print: () => assert.fail('unsafe policy path must not pass'),
    error: (line) => errors.push(line),
  }), 1)
  assert.deepEqual(errors, ['RELEASE_PUBLICATION_ERROR=publication-policy-path-invalid'])
  assert.equal(main([], {
    cwd: fixture.root,
    print: () => assert.fail('usage must not pass'),
    error: (line) => errors.push(line),
  }), 2)
  assert.match(errors.at(-1), /^usage:/)
})

test('publication_candidate_refuses_missing_manifest_asset_and_invalid_root', () => {
  const fixture = releaseCandidateFixture()
  assert.throws(() => candidateFromReleaseManifest(fixture.manifest, {}), /candidate-root-invalid/)
  assert.throws(() => candidateFromReleaseManifest({ ...fixture.manifest, public_assets: ['../escape'] }, {
    root: fixture.root,
  }), /candidate-assets-invalid/)
})
