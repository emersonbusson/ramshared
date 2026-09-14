import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { prepareReleaseConfig } from './prepare-release-config.mjs'

function fixture(cargoVersion, manifestVersion) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-config-'))
  writeFileSync(path.join(root, 'Cargo.toml'), `[workspace.package]\nversion = "${cargoVersion}"\n`)
  writeFileSync(path.join(root, '.release-please-manifest.json'), JSON.stringify({ '.': manifestVersion }))
  writeFileSync(path.join(root, 'release-please-config.json'), JSON.stringify({ 'release-type': 'simple' }))
  return root
}

test('promotes_declared_workspace_version_once', () => {
  const root = fixture('0.13.0', '0.12.0')
  const result = prepareReleaseConfig({ root })
  const config = JSON.parse(readFileSync(result.output, 'utf8'))
  assert.equal(result.release_as, '0.13.0')
  assert.equal(config['release-as'], '0.13.0')
})

test('uses_conventional_commits_after_versions_are_aligned', () => {
  const root = fixture('0.13.0', '0.13.0')
  const result = prepareReleaseConfig({ root })
  const config = JSON.parse(readFileSync(result.output, 'utf8'))
  assert.equal(result.release_as, null)
  assert.equal(Object.hasOwn(config, 'release-as'), false)
})

test('rejects_prerelease_or_invalid_versions', () => {
  const root = fixture('0.13.0-beta.1', '0.12.0')
  assert.throws(() => prepareReleaseConfig({ root }), /release-config-version-invalid/)
})
