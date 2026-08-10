import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  main,
  validateArtifactManifest,
  validateRetention,
  validateSanitizedText,
} from './check-ci-artifacts.mjs'

function sha256(text) {
  return createHash('sha256').update(text).digest('hex')
}

function fixture({ content = '{"plan":"safe"}\n', artifact = {}, manifest = {}, writeArtifact = true } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-artifact-'))
  const artifactPath = artifact.path ?? 'plan.json'
  if (writeArtifact) writeFileSync(path.join(root, artifactPath), content)
  const record = {
    path: artifactPath,
    class: 'lab-plan',
    attachment: 'workflow',
    retention_days: 14,
    bytes: Buffer.byteLength(content),
    sha256: sha256(content),
    sanitized: true,
    summary: 'plan-only isolated-lab result',
    ...artifact,
  }
  const value = {
    schema_version: 1,
    source_sha: '0123456789abcdef0123456789abcdef01234567',
    terminal_status: 'PASS',
    artifacts: [record],
    ...manifest,
  }
  const manifestPath = path.join(root, 'artifact-manifest.json')
  writeFileSync(manifestPath, `${JSON.stringify(value, null, 2)}\n`)
  return { root, manifestPath, value }
}

test('artifact_manifest_requires_hash_and_retention', () => {
  const valid = fixture()
  assert.deepEqual(validateArtifactManifest(valid.value, { root: valid.root }), { ok: true, errors: [] })
  assert.equal(validateRetention({ class: 'lab-plan', attachment: 'workflow', retention_days: 14 }), true)
  assert.equal(validateRetention({ class: 'lab-plan', attachment: 'workflow', retention_days: 7 }), false)

  const invalid = fixture({ artifact: { sha256: '0'.repeat(64), retention_days: 7 } })
  const result = validateArtifactManifest(invalid.value, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('artifact-retention-invalid'), true)
  assert.equal(result.errors.includes('artifact-hash-mismatch'), true)
})

test('artifact_sanitizer_rejects_private_or_sensitive_content_without_echo', () => {
  const secret = 'token=synthetic-value'
  const result = validateSanitizedText(secret)
  assert.equal(result.ok, false)
  assert.deepEqual(result.errors, ['sanitizer-sensitive-content'])

  const invalid = fixture({ content: `${secret}\n` })
  const output = []
  const errors = []
  assert.equal(main(['--check', invalid.manifestPath, '--root', invalid.root], {
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(errors.includes('CI_ARTIFACT_ERROR=sanitizer-sensitive-content'), true)
  assert.equal([...output, ...errors].join('\n').includes(secret), false)

  const privatePath = ['/', 'home', '/', 'private-user', '/', 'evidence'].join('')
  assert.deepEqual(validateSanitizedText(privatePath), { ok: false, errors: ['sanitizer-sensitive-content'] })
})

test('artifact_manifest_rejects_unknown_class', () => {
  const invalid = fixture({ artifact: { class: 'unbounded-log' } })
  const result = validateArtifactManifest(invalid.value, { root: invalid.root })
  assert.equal(result.ok, false)
  assert.equal(result.errors.includes('artifact-class-invalid'), true)
})

test('artifact_manifest_rejects_unsafe_paths_missing_files_and_invalid_shape', () => {
  const unsafe = fixture({ artifact: { path: '../outside.txt' }, writeArtifact: false })
  const unsafeResult = validateArtifactManifest(unsafe.value, { root: unsafe.root })
  assert.equal(unsafeResult.errors.includes('artifact-path-unsafe'), true)

  const missing = fixture({ artifact: { path: 'missing.json' }, writeArtifact: false })
  const missingResult = validateArtifactManifest(missing.value, { root: missing.root })
  assert.equal(missingResult.errors.includes('artifact-file-missing'), true)

  const malformed = validateArtifactManifest({ schema_version: 2, artifacts: [] })
  assert.equal(malformed.ok, false)
  assert.equal(malformed.errors.includes('manifest-schema-invalid'), true)
})

test('artifact_manifest_rejects_invalid_records_and_nonregular_artifacts', () => {
  const valid = fixture()
  assert.deepEqual(validateArtifactManifest(valid.value), { ok: true, errors: [] })

  const invalidRecord = fixture({ manifest: { artifacts: [null] } })
  const invalidResult = validateArtifactManifest(invalidRecord.value, { root: invalidRecord.root })
  assert.equal(invalidResult.errors.includes('artifact-record-invalid'), true)

  const directory = fixture({ artifact: { path: 'directory' }, writeArtifact: false })
  mkdirSync(path.join(directory.root, 'directory'))
  const directoryResult = validateArtifactManifest(directory.value, { root: directory.root })
  assert.equal(directoryResult.errors.includes('artifact-file-invalid'), true)

  const rootBoundary = fixture()
  const rootResult = validateArtifactManifest(rootBoundary.value, { root: path.parse(rootBoundary.root).root })
  assert.equal(rootResult.errors.includes('artifact-path-unsafe'), true)
})

test('artifact_sanitizer_rejects_invalid_or_oversized_text', () => {
  assert.deepEqual(validateSanitizedText(null), { ok: false, errors: ['sanitizer-text-invalid'] })
  assert.deepEqual(validateSanitizedText('x'.repeat(64 * 1024 + 1)), {
    ok: false,
    errors: ['sanitizer-text-too-large'],
  })
})

test('artifact_cli_accepts_a_verified_manifest', () => {
  const valid = fixture()
  const output = []
  assert.equal(main(['--check', valid.manifestPath, '--root', valid.root], {
    print: (line) => output.push(line),
    error: () => assert.fail('verified manifest must not print an error'),
  }), 0)
  assert.equal(output.includes('CI_ARTIFACT_STATUS=PASS'), true)
  assert.equal(readFileSync(valid.manifestPath, 'utf8').includes('plan-only'), true)
})

test('artifact_cli_rejects_invalid_arguments_and_unreadable_manifest', () => {
  const errors = []
  assert.equal(main([], { print: () => {}, error: (line) => errors.push(line) }), 2)
  assert.equal(errors[0].startsWith('usage:'), true)

  const unreadable = []
  assert.equal(main(['--check', path.join(tmpdir(), 'missing-artifact-manifest.json')], {
    print: () => {},
    error: (line) => unreadable.push(line),
  }), 1)
  assert.deepEqual(unreadable, ['CI_ARTIFACT_ERROR=manifest-read-failed'])
})
