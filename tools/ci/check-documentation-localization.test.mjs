import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  loadManifest,
  run,
  validateLocalizations,
  validateManifest,
} from './check-documentation-localization.mjs'

const CLI = path.resolve('tools/ci/check-documentation-localization.mjs')

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function write(root, relative, content) {
  const target = path.join(root, relative)
  mkdirSync(path.dirname(target), { recursive: true })
  writeFileSync(target, content)
}

function fixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-localization-'))
  const canonical = [
    '# RamShared\n',
    '\n',
    '[Português (Brasil)](README.pt-BR.md)\n',
    '\n',
    '[Portal](docs/pt-BR/README.md)\n',
  ].join('')
  const translated = [
    '# RamShared\n',
    '\n',
    '[English](README.md)\n',
    '\n',
    '> Esta página é informativa e não normativa; o README.md em inglês é a referência canônica.\n',
    '\n',
    '[Portal](docs/pt-BR/README.md)\n',
  ].join('')
  const portal = [
    '# Portal em português\n',
    '\n',
    '[English README](../../README.md)\n',
    '[README em português](../../README.pt-BR.md)\n',
    '> Este portal é informativo e não substitui a documentação técnica em inglês.\n',
    '\n',
    '## Quickstart\n',
    '[Quickstart](../../README.md#quick-start)\n',
    '\n',
    '## Installation\n',
    '[Installation](../../docs/packaging/INSTALLABLES.md)\n',
    '\n',
    '## Safe operation\n',
    '[Safe operation](../../README.md#safe-operation)\n',
    '\n',
    '## Troubleshooting\n',
    '[Troubleshooting](../../docs/FAQ.md)\n',
    '\n',
    '## Architecture\n',
    '[Architecture](../../ARCHITECTURE.md)\n',
  ].join('')
  write(root, 'README.md', canonical)
  write(root, 'README.pt-BR.md', translated)
  write(root, 'docs/pt-BR/README.md', portal)
  write(root, 'docs/packaging/INSTALLABLES.md', '# Installables\n')
  write(root, 'docs/FAQ.md', '# FAQ\n')
  write(root, 'ARCHITECTURE.md', '# Architecture\n')
  const manifest = {
    schema_version: 1,
    canonical_language: 'en',
    current_policy: 'informational-non-normative',
    protected_document_classes: ['prd', 'spec', 'impl', 'adr', 'ci', 'evidence', 'benchmark'],
    entries: [
      {
        canonical_source: 'README.md',
        localized_path: 'README.pt-BR.md',
        source_sha256: sha256(canonical),
        state: 'current',
        policy: 'informational-non-normative',
      },
      {
        canonical_source: 'README.md',
        localized_path: 'docs/pt-BR/README.md',
        source_sha256: sha256(canonical),
        state: 'current',
        policy: 'informational-non-normative',
      },
    ],
  }
  write(root, 'docs/localization/manifest.json', `${JSON.stringify(manifest, null, 2)}\n`)
  return { root, manifest }
}

function findingsFor(root) {
  const manifest = loadManifest(root)
  return validateLocalizations(manifest, root)
}

test('manifest_current_hashes_pass', () => {
  const { root } = fixture()
  assert.deepEqual(validateManifest(loadManifest(root), root), [])
  assert.deepEqual(findingsFor(root), [])
})

test('missing_required_localization_fails', () => {
  const { root } = fixture()
  rmSync(path.join(root, 'README.pt-BR.md'))
  assert.match(JSON.stringify(findingsFor(root)), /MISSING_LOCALIZATION/)
})

test('stale_source_hash_fails', () => {
  const { root } = fixture()
  write(root, 'README.md', '# Changed source\n')
  assert.match(JSON.stringify(findingsFor(root)), /STALE_HASH/)
})

test('language_switches_and_portal_links_pass', () => {
  const { root } = fixture()
  const findings = findingsFor(root)
  assert.equal(findings.some((item) => /LINK|SWITCH|OBJECTIVE/.test(item.rule)), false)
})

test('broken_language_switch_fails', () => {
  const { root } = fixture()
  write(root, 'README.pt-BR.md', '[English](missing.md)\n')
  assert.match(JSON.stringify(findingsFor(root)), /BROKEN_LINK|LANGUAGE_SWITCH/)
})

test('authority_claim_is_rejected_without_echo', () => {
  const { root } = fixture()
  const claim = 'Esta tradução é o documento normativo oficial e a fonte canônica.\n'
  write(root, 'README.pt-BR.md', claim)
  const findings = findingsFor(root)
  const serialized = JSON.stringify(findings)
  assert.match(serialized, /AUTHORITY_CLAIM/)
  assert.doesNotMatch(serialized, /normativo oficial/)
  write(root, 'README.pt-BR.md', '> Página informativa e não normativa.\nEsta tradução é a fonte canônica.\n')
  assert.match(JSON.stringify(findingsFor(root)), /AUTHORITY_CLAIM/)
})

test('protected_normative_localization_path_fails', () => {
  const { root, manifest } = fixture()
  const protectedPath = 'docs/specs/no-milestone/example/SPEC.pt-BR.md'
  write(root, protectedPath, '# Tradução\n')
  manifest.entries[1].localized_path = protectedPath
  write(root, 'docs/localization/manifest.json', `${JSON.stringify(manifest)}\n`)
  assert.match(JSON.stringify(findingsFor(root)), /PROTECTED_PATH/)
})

test('invalid_manifest_state_policy_fails', () => {
  const { root, manifest } = fixture()
  manifest.entries[0].state = 'stale'
  manifest.entries[1].policy = 'normative'
  write(root, 'docs/localization/manifest.json', `${JSON.stringify(manifest)}\n`)
  const findings = findingsFor(root)
  assert.match(JSON.stringify(findings), /STATE|POLICY/)
})

test('manifest_schema_and_path_guards_fail', () => {
  const { root, manifest } = fixture()
  assert.match(JSON.stringify(validateManifest(null, root)), /MANIFEST_SCHEMA/)
  const malformed = { ...manifest, canonical_language: 'pt', current_policy: 'normative', protected_document_classes: ['unknown'], entries: [] }
  const shapeFindings = JSON.stringify(validateManifest(malformed, root))
  assert.match(shapeFindings, /MANIFEST_LANGUAGE|MANIFEST_POLICY|MANIFEST_PROTECTED_CLASSES|MANIFEST_ENTRIES/)
  const malformedEntry = { ...manifest.entries[0], extra: true }
  const malformedFindings = JSON.stringify(validateManifest({ ...manifest, entries: [malformedEntry, manifest.entries[1]] }, root))
  assert.match(malformedFindings, /ENTRY_SCHEMA|MISSING_ENTRY/)
  const unsafeEntry = { ...manifest.entries[0], canonical_source: '../README.md', localized_path: '/tmp/translated.md', source_sha256: 'bad', state: 'stale', policy: 'normative' }
  const unsafeFindings = JSON.stringify(validateManifest({ ...manifest, entries: [unsafeEntry, manifest.entries[1]] }, root))
  assert.match(unsafeFindings, /UNSAFE_PATH|SOURCE_HASH|STATE|POLICY|UNEXPECTED_LOCALIZATION/)
  assert.equal(loadManifest(path.join(root, 'missing')), null)
})

test('portal_missing_objective_fails', () => {
  const { root } = fixture()
  write(root, 'docs/pt-BR/README.md', '# Portal\n[English](../../README.md)\n')
  assert.match(JSON.stringify(findingsFor(root)), /PORTAL_OBJECTIVE|DISCLAIMER/)
})

test('checker_output_is_deterministic_and_read_only', () => {
  const { root } = fixture()
  const before = readFileSync(path.join(root, 'docs/localization/manifest.json'), 'utf8')
  const first = run({ root })
  const second = run({ root })
  assert.deepEqual(first, second)
  assert.equal(readFileSync(path.join(root, 'docs/localization/manifest.json'), 'utf8'), before)
})

test('missing_canonical_readme_fails_closed', () => {
  const { root } = fixture()
  rmSync(path.join(root, 'README.md'))
  assert.match(JSON.stringify(run({ root }).findings), /MISSING_CANONICAL/)
})

test('cli_usage_error_returns_two', () => {
  const result = spawnSync(process.execPath, [CLI, '--unknown'], { encoding: 'utf8' })
  assert.equal(result.status, 2)
  assert.match(`${result.stdout}${result.stderr}`, /usage/i)
})

test('cli_all_returns_zero_for_current_repository', () => {
  const result = spawnSync(process.execPath, [CLI, '--all'], { encoding: 'utf8' })
  assert.equal(result.status, 0, `${result.stdout}${result.stderr}`)
  assert.match(result.stdout, /LOCALIZATION_STATUS=PASS/)
})

test('repository_localization_gate_passes', () => {
  const result = run({ root: path.resolve('.') })
  assert.equal(result.ok, true, JSON.stringify(result.findings, null, 2))
})
