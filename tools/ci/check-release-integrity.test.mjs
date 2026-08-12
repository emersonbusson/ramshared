import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  main,
  validatePublicDriverEligibility,
  validateReleaseManifest,
  validateSourceBinding,
} from './check-release-integrity.mjs'

const SOURCE_SHA = '0123456789abcdef0123456789abcdef01234567'
const SBOM_GENERATOR = { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.5' }
const TARGET_TAG = 'v0.9.0-beta.1'

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function evidenceRecord(record) {
  return {
    path: record.path,
    class: 'release-evidence',
    attachment: 'release',
    retention_days: null,
    bytes: record.bytes,
    sha256: record.sha256,
    sanitized: true,
    summary: 'verified release evidence',
  }
}

function fixture({ manifest: manifestOverrides = {}, sbomContent } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-integrity-'))
  const releaseDir = path.join(root, 'artifacts', 'release')
  mkdirSync(releaseDir, { recursive: true })
  const cargoLock = 'version = 4\n'
  writeFileSync(path.join(root, 'Cargo.lock'), cargoLock)

  const bundlePath = `artifacts/release/ramshared-linux-${TARGET_TAG}.tar.gz`
  const bundleBytes = Buffer.from('ramshared-linux-bundle\n')
  writeFileSync(path.join(root, bundlePath), bundleBytes)
  const checksumPath = `${bundlePath}.sha256`
  const checksumBytes = Buffer.from(`${sha256(bundleBytes)}  ${path.basename(bundlePath)}\n`)
  writeFileSync(path.join(root, checksumPath), checksumBytes)
  const sbomPath = 'artifacts/release/ramshared-sbom.cdx.json'
  const sbomBytes = Buffer.from(sbomContent ?? JSON.stringify({
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    metadata: { tools: [{ name: 'cargo-cyclonedx', version: '0.5.9' }] },
  }, null, 2) + '\n')
  writeFileSync(path.join(root, sbomPath), sbomBytes)

  const bundle = {
    path: bundlePath,
    command: 'scripts/package/build-linux-bundle.sh',
    bytes: bundleBytes.length,
    sha256: sha256(bundleBytes),
    checksums_verified: true,
  }
  const sbom = {
    path: sbomPath,
    format: 'CycloneDX',
    spec_version: SBOM_GENERATOR.spec_version,
    generator: { name: SBOM_GENERATOR.name, version: SBOM_GENERATOR.version },
    bytes: sbomBytes.length,
    sha256: sha256(sbomBytes),
    source_sha: SOURCE_SHA,
    cargo_lock_sha256: sha256(cargoLock),
  }
  const checksum = {
    path: checksumPath,
    archive: bundlePath,
    algorithm: 'sha256',
    bytes: checksumBytes.length,
    sha256: sha256(checksumBytes),
  }
  const manifest = {
    schema_version: 1,
    terminal_status: 'PASS',
    source: {
      tag: TARGET_TAG,
      sha: SOURCE_SHA,
      clean_tree: true,
      cargo_lock_sha256: sha256(cargoLock),
      rust_version: '1.88.0',
    },
    sbom_generator: SBOM_GENERATOR,
    linux_bundle: bundle,
    detached_checksum: checksum,
    sbom,
    evidence: {
      schema_version: 1,
      source_sha: SOURCE_SHA,
      terminal_status: 'PASS',
      artifacts: [evidenceRecord(bundle), evidenceRecord(checksum), evidenceRecord(sbom)],
    },
    public_assets: [
      `ramshared-linux-${TARGET_TAG}.tar.gz`,
      `ramshared-linux-${TARGET_TAG}.tar.gz.sha256`,
      'ramshared-sbom.cdx.json',
      'release-manifest.json',
    ],
    windows_driver_status: 'not-included',
    windows_drivers: [],
    rollback: {
      prior_release: 'v0.7.4',
      trigger: 'bundle checksum mismatch',
    },
    ...manifestOverrides,
  }
  const manifestPath = path.join(root, 'artifacts', 'release', 'release-manifest.json')
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  return { root, manifestPath, manifest, bundlePath, checksumPath, sbomPath }
}

test('release_manifest_requires_bound_inputs', () => {
  const valid = fixture()
  assert.deepEqual(validateSourceBinding(valid.manifest, {
    root: valid.root,
    expected_tag: TARGET_TAG,
    expected_revision: SOURCE_SHA,
  }), { ok: true, errors: [] })
  assert.deepEqual(validateReleaseManifest(valid.manifest, {
    root: valid.root,
    expected_tag: TARGET_TAG,
    expected_revision: SOURCE_SHA,
  }), { ok: true, errors: [] })

  const missing = fixture({ manifest: { source: { tag: TARGET_TAG } } })
  const result = validateReleaseManifest(missing.manifest, { root: missing.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('source-binding-invalid'), true)
})

test('release_manifest_requires_exact_four_public_assets', () => {
  const valid = fixture()
  assert.deepEqual(validateReleaseManifest(valid.manifest, { root: valid.root }), { ok: true, errors: [] })
  const invalid = fixture({ manifest: { public_assets: [`ramshared-linux-${TARGET_TAG}.tar.gz`] } })
  const result = validateReleaseManifest(invalid.manifest, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('release-public-assets-invalid'), true)
})

test('release_manifest_rejects_invalid_detached_checksum_or_historical_target', () => {
  const invalidChecksum = fixture()
  writeFileSync(path.join(invalidChecksum.root, invalidChecksum.checksumPath), '0'.repeat(64) + '  wrong.tar.gz\n')
  const checksumResult = validateReleaseManifest(invalidChecksum.manifest, { root: invalidChecksum.root })
  assert.equal(checksumResult.errors.includes('release-file-hash-mismatch'), true)
  assert.equal(checksumResult.errors.includes('detached-checksum-content-invalid'), true)

  const historical = fixture()
  historical.manifest.source.tag = 'v0.8.0'
  const historicalResult = validateReleaseManifest(historical.manifest, { root: historical.root })
  assert.equal(historicalResult.errors.includes('release-public-assets-invalid'), true)
})

test('release_manifest_rejects_dirty_source', () => {
  const invalid = fixture({ manifest: { source: {
    tag: TARGET_TAG, sha: SOURCE_SHA, clean_tree: false,
    cargo_lock_sha256: '0'.repeat(64), rust_version: '1.88.0',
  } } })
  const result = validateReleaseManifest(invalid.manifest, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('source-tree-dirty'), true)
})

test('release_manifest_rejects_missing_sbom', () => {
  const invalid = fixture()
  rmSync(path.join(invalid.root, invalid.sbomPath))
  const result = validateReleaseManifest(invalid.manifest, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('release-file-missing'), true)
})

test('release_manifest_rejects_test_signed_driver', () => {
  const invalid = fixture({ manifest: {
    windows_driver_status: 'present',
    windows_drivers: [{
      component: 'ramshared.sys',
      signing: 'test-signed',
      attested: false,
      public_eligible: true,
    }],
  } })
  const driver = validatePublicDriverEligibility(invalid.manifest.windows_drivers)
  assert.equal(driver.ok, false)
  assert.equal(driver.errors.includes('windows-driver-test-signed-public'), true)
  const result = validateReleaseManifest(invalid.manifest, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('windows-driver-test-signed-public'), true)
})

test('release_manifest_rejects_hash_mismatch', () => {
  const invalid = fixture()
  writeFileSync(path.join(invalid.root, invalid.bundlePath), 'tampered bundle\n')
  const result = validateReleaseManifest(invalid.manifest, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('release-file-hash-mismatch'), true)
})

test('release_manifest_rejects_invalid_sbom_evidence_and_driver_shapes', () => {
  const badSbom = fixture({ sbomContent: 'not-json\n' })
  const sbomResult = validateReleaseManifest(badSbom.manifest, { root: badSbom.root })
  assert.equal(sbomResult.errors.includes('sbom-content-invalid'), true)

  const missingEvidence = fixture({ manifest: {
    evidence: {
      schema_version: 1,
      source_sha: SOURCE_SHA,
      terminal_status: 'PASS',
      artifacts: [],
    },
    windows_driver_status: 'present',
    windows_drivers: [{ component: '', signing: 'invalid', attested: 'yes', public_eligible: true }],
    rollback: { prior_release: 'invalid', trigger: '' },
  } })
  const result = validateReleaseManifest(missingEvidence.manifest, { root: missingEvidence.root })
  for (const rule of [
    'release-evidence-manifest-schema-invalid',
    'release-evidence-record-missing',
    'windows-driver-record-invalid',
    'rollback-invalid',
  ]) assert.equal(result.errors.includes(rule), true, rule)

  assert.deepEqual(validatePublicDriverEligibility(null), { ok: false, errors: ['windows-drivers-invalid'] })
  const protectedDriver = validatePublicDriverEligibility([{ component: 'ramshared.sys', signing: 'test-signed', attested: false, public_eligible: false }])
  assert.deepEqual(protectedDriver, { ok: true, errors: [] })

  const mismatchedGenerator = fixture({ manifest: {
    sbom_generator: { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.4' },
  } })
  const generatorResult = validateReleaseManifest(mismatchedGenerator.manifest, { root: mismatchedGenerator.root })
  assert.equal(generatorResult.ok, false)
  assert.equal(generatorResult.errors.includes('sbom-generator-mismatch'), true)
})

test('release_manifest_checks_expected_identity_and_accepted_sbom_tool_shapes', () => {
  const withComponents = fixture({ sbomContent: JSON.stringify({
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    metadata: { tools: { components: [{ name: 'cargo-cyclonedx', version: '0.5.9' }] } },
  }) })
  const record = withComponents.manifest.sbom
  const bytes = readFileSync(path.join(withComponents.root, withComponents.sbomPath))
  record.bytes = bytes.length
  record.sha256 = sha256(bytes)
  withComponents.manifest.evidence.artifacts[2].bytes = bytes.length
  withComponents.manifest.evidence.artifacts[2].sha256 = sha256(bytes)
  assert.deepEqual(validateReleaseManifest(withComponents.manifest, { root: withComponents.root }), { ok: true, errors: [] })

  const mismatch = validateSourceBinding(withComponents.manifest, {
    root: withComponents.root,
    expected_tag: 'v9.9.9',
    expected_revision: 'fedcba9876543210fedcba9876543210fedcba98',
  })
  assert.equal(mismatch.errors.includes('source-tag-mismatch'), true)
  assert.equal(mismatch.errors.includes('source-revision-mismatch'), true)
  assert.deepEqual(validateReleaseManifest(withComponents.manifest), { ok: true, errors: [] })
})

test('release_manifest_cli_uses_stable_errors_without_echoing_values', () => {
  const invalid = fixture({ manifest: { source: {
    tag: 'private-user/release', sha: SOURCE_SHA, clean_tree: true,
    cargo_lock_sha256: '0'.repeat(64), rust_version: '1.88.0',
  } } })
  writeFileSync(invalid.manifestPath, `${JSON.stringify(invalid.manifest, null, 2)}\n`)
  const output = []
  const errors = []
  assert.equal(main(['--check', invalid.manifestPath, '--root', invalid.root], {
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(errors.includes('RELEASE_INTEGRITY_ERROR=source-binding-invalid'), true)
  assert.equal([...output, ...errors].join('\n').includes('private-user/release'), false)
})

test('release_manifest_cli_accepts_verified_input', () => {
  const valid = fixture()
  const output = []
  assert.equal(main(['--check', valid.manifestPath, '--root', valid.root, '--tag', TARGET_TAG, '--revision', SOURCE_SHA], {
    print: (line) => output.push(line),
    error: () => assert.fail('verified manifest must not emit an error'),
  }), 0)
  assert.deepEqual(output, ['RELEASE_INTEGRITY_STATUS=PASS', 'RELEASE_INTEGRITY_VERDICT=PASS'])
})

test('release_manifest_cli_rejects_bad_arguments_and_unreadable_input', () => {
  const usage = []
  assert.equal(main([], { print: () => {}, error: (line) => usage.push(line) }), 2)
  assert.equal(usage[0].startsWith('usage:'), true)
  const errors = []
  assert.equal(main(['--check', path.join(tmpdir(), 'missing-release-manifest.json')], {
    print: () => {}, error: (line) => errors.push(line),
  }), 1)
  assert.deepEqual(errors, ['RELEASE_INTEGRITY_ERROR=manifest-read-failed'])
  assert.equal(readFileSync(fixture().manifestPath, 'utf8').includes('release-evidence'), true)
})
