import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { validateReleaseManifest } from './check-release-integrity.mjs'
import {
  buildReleaseManifest,
  main,
  writeReleaseManifest,
} from './write-release-manifest.mjs'

const SOURCE_SHA = '0123456789abcdef0123456789abcdef01234567'
const TARGET_TAG = 'v0.9.0-beta.1'

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-writer-'))
  const releaseDir = path.join(root, 'artifacts', 'release')
  mkdirSync(releaseDir, { recursive: true })
  const cargoLock = 'version = 4\n'
  writeFileSync(path.join(root, 'Cargo.lock'), cargoLock)
  const bundleName = `ramshared-linux-${TARGET_TAG}.tar.gz`
  const bundle = 'bundle\n'
  writeFileSync(path.join(releaseDir, bundleName), bundle)
  writeFileSync(path.join(releaseDir, `${bundleName}.sha256`), `${sha256(bundle)}  ${bundleName}\n`)
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
  return {
    root,
    input: {
      tag: TARGET_TAG,
      revision: SOURCE_SHA,
      clean_tree: true,
      rust_version: '1.98.0',
      rust_commit: 'a'.repeat(40),
      bundle_path: `artifacts/release/${bundleName}`,
      checksum_path: `artifacts/release/${bundleName}.sha256`,
      sbom_path: 'artifacts/release/ramshared-sbom.cdx.json',
      prior_release: 'v0.7.4',
      rollback_trigger: 'bundle checksum mismatch',
    },
    cargoLock,
  }
}

test('release_manifest_writer_binds_exact_input_hashes', () => {
  const valid = fixture()
  const manifest = buildReleaseManifest(valid.input, { root: valid.root })
  assert.equal(manifest.source.sha, SOURCE_SHA)
  assert.equal(manifest.source.cargo_lock_sha256, sha256(valid.cargoLock))
  assert.equal(manifest.sbom_generator.version, '0.5.9')
  assert.equal(manifest.sbom.spec_version, '1.5')
  assert.equal(manifest.evidence.artifacts.length, 3)
  assert.deepEqual(validateReleaseManifest(manifest, {
    root: valid.root,
    expected_tag: valid.input.tag,
    expected_revision: valid.input.revision,
  }), { ok: true, errors: [] })

  const written = writeReleaseManifest(valid.input, {
    root: valid.root,
    out_path: 'artifacts/release/release-manifest.json',
  })
  assert.deepEqual(JSON.parse(readFileSync(path.join(valid.root, 'artifacts/release/release-manifest.json'), 'utf8')), written)
})

test('release_manifest_writer_binds_exact_four_public_assets', () => {
  const valid = fixture()
  const manifest = buildReleaseManifest(valid.input, { root: valid.root })
  assert.deepEqual(manifest.public_assets, [
    `ramshared-linux-${TARGET_TAG}.tar.gz`,
    `ramshared-linux-${TARGET_TAG}.tar.gz.sha256`,
    'ramshared-sbom.cdx.json',
    'release-manifest.json',
  ])
})

test('release_manifest_writer_rejects_unsafe_output_or_revision', () => {
  const valid = fixture()
  assert.throws(() => buildReleaseManifest({ ...valid.input, revision: 'main' }, { root: valid.root }), /release-manifest-input-invalid/)
  assert.throws(() => buildReleaseManifest({ ...valid.input, tag: 'v0.8.0' }, { root: valid.root }), /release-manifest-input-invalid/)
  assert.throws(() => writeReleaseManifest(valid.input, { root: valid.root, out_path: '../release-manifest.json' }), /release-manifest-output-invalid/)
  assert.throws(() => buildReleaseManifest({ ...valid.input, clean_tree: false }, { root: valid.root }), /release-manifest-input-invalid/)
})

test('release_manifest_writer_rejects_missing_invalid_and_unwritable_artifacts', () => {
  const valid = fixture()
  writeFileSync(path.join(valid.root, valid.input.sbom_path), 'not-json\n')
  assert.throws(() => buildReleaseManifest(valid.input, { root: valid.root }), /release-manifest-input-invalid/)

  const missing = fixture()
  assert.throws(() => buildReleaseManifest({ ...missing.input, bundle_path: 'missing.tar.gz' }, { root: missing.root }), /release-manifest-input-invalid/)

  const unwritable = fixture()
  assert.throws(() => writeReleaseManifest(unwritable.input, {
    root: unwritable.root,
    out_path: 'missing/release-manifest.json',
  }), /release-manifest-write-failed/)
})

test('release_manifest_writer_cli_writes_only_verified_relative_output', () => {
  const valid = fixture()
  const output = []
  assert.equal(main([
    '--tag', valid.input.tag,
    '--revision', valid.input.revision,
    '--rust-version', valid.input.rust_version,
    '--rust-commit', valid.input.rust_commit,
    '--bundle', valid.input.bundle_path,
    '--checksum', valid.input.checksum_path,
    '--sbom', valid.input.sbom_path,
    '--prior-release', valid.input.prior_release,
    '--rollback-trigger', valid.input.rollback_trigger,
    '--out', 'artifacts/release/release-manifest.json',
    '--clean-tree',
  ], {
    cwd: valid.root,
    print: (line) => output.push(line),
    error: () => assert.fail('valid release manifest must not emit an error'),
  }), 0)
  assert.equal(output.includes('RELEASE_MANIFEST_STATUS=PASS'), true)

  const errors = []
  const privateValue = 'private-user/release'
  assert.equal(main([
    '--tag', privateValue,
    '--revision', 'main',
    '--rust-version', 'unknown',
    '--rust-commit', 'unknown',
    '--bundle', '../bundle',
    '--checksum', '../bundle.sha256',
    '--sbom', '../sbom',
    '--prior-release', 'none',
    '--rollback-trigger', '',
    '--out', '../manifest.json',
    '--clean-tree',
  ], {
    cwd: valid.root,
    print: () => {}, error: (line) => errors.push(line),
  }), 1)
  assert.equal(errors.every((line) => !line.includes(privateValue)), true)
  assert.equal(errors.includes('RELEASE_MANIFEST_ERROR=release-manifest-output-invalid'), true)
})

test('release_manifest_writer_cli_rejects_usage_and_invalid_manifest_input', () => {
  const usage = []
  assert.equal(main([], { print: () => {}, error: (line) => usage.push(line) }), 2)
  assert.equal(usage[0].startsWith('usage:'), true)

  const valid = fixture()
  const errors = []
  assert.equal(main([
    '--tag', valid.input.tag,
    '--revision', 'main',
    '--rust-version', valid.input.rust_version,
    '--rust-commit', valid.input.rust_commit,
    '--bundle', valid.input.bundle_path,
    '--checksum', valid.input.checksum_path,
    '--sbom', valid.input.sbom_path,
    '--prior-release', valid.input.prior_release,
    '--rollback-trigger', valid.input.rollback_trigger,
    '--out', 'artifacts/release/invalid.json',
    '--clean-tree',
  ], {
    cwd: valid.root,
    print: () => {}, error: (line) => errors.push(line),
  }), 1)
  assert.deepEqual(errors, ['RELEASE_MANIFEST_ERROR=release-manifest-input-invalid'])
})
