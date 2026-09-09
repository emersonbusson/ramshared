import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

import { mergeReleaseSboms } from './merge-release-sboms.mjs'

const tool = [{ vendor: 'CycloneDX', name: 'cargo-cyclonedx', version: '0.5.9' }]

function bom(name, root, dependencyName = 'serde') {
  const rootRef = `path+file://${root}/crates/${name}#0.1.0`
  const dependencyRef = `registry+https://github.com/rust-lang/crates.io-index#${dependencyName}@1.0.0`
  return {
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    serialNumber: 'urn:uuid:00000000-0000-4000-8000-000000000000',
    version: 1,
    metadata: {
      timestamp: '2099-01-01T00:00:00Z',
      tools: tool,
      component: {
        type: 'application',
        'bom-ref': rootRef,
        name,
        version: '0.1.0',
        purl: `pkg:cargo/${name}@0.1.0?download_url=file://.`,
        components: [{
          type: 'application',
          'bom-ref': `${rootRef} bin-target-0`,
          name: name === 'ramshared-cli' ? 'ramshared' : 'ramsharedd',
          version: '0.1.0',
          purl: `pkg:cargo/${name}@0.1.0?download_url=file://.#src/main.rs`,
        }],
      },
    },
    components: [{
      type: 'library',
      'bom-ref': dependencyRef,
      name: dependencyName,
      version: '1.0.0',
      purl: `pkg:cargo/${dependencyName}@1.0.0`,
    }],
    dependencies: [
      { ref: rootRef, dependsOn: [dependencyRef] },
      { ref: dependencyRef, dependsOn: [] },
    ],
  }
}

test('release_workspace_sbom_is_deterministic_path_free_and_binds_both_binaries', () => {
  const options = {
    tag: 'v0.9.0-beta.1',
    revision: '361427a63cbeb2a8b0ecafb224adeecb0539af9b',
    sourceDateEpoch: '1786697752',
  }
  const first = mergeReleaseSboms([
    bom('ramshared-cli', '/workspace/ramshared'),
    bom('ramshared-wsl2d', '/workspace/ramshared'),
  ], options)
  const secondInputs = [
    bom('ramshared-cli', '/different/checkout'),
    bom('ramshared-wsl2d', '/different/checkout'),
  ]
  secondInputs[0].metadata.timestamp = '2000-01-01T00:00:00Z'
  secondInputs[1].serialNumber = 'urn:uuid:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa'
  const second = mergeReleaseSboms(secondInputs, options)

  assert.deepEqual(second, first)
  assert.equal(first.bomFormat, 'CycloneDX')
  assert.equal(first.specVersion, '1.5')
  assert.equal(first.metadata.component.name, 'ramshared')
  assert.equal(first.metadata.component.version, '0.9.0-beta.1')
  assert.deepEqual(first.metadata.component.components.map(({ name }) => name), [
    'ramshared-cli',
    'ramshared-wsl2d',
  ])
  assert.deepEqual(first.metadata.properties, [
    { name: 'ramshared:release:tag', value: options.tag },
    { name: 'ramshared:source:revision', value: options.revision },
  ])
  assert.match(first.serialNumber, /^urn:uuid:[0-9a-f-]{36}$/)
  assert.equal(JSON.stringify(first).includes('file://'), false)
  assert.equal(JSON.stringify(first).includes('/workspace'), false)
  assert.equal(first.dependencies[0].ref, 'pkg:generic/ramshared@0.9.0-beta.1')
  assert.deepEqual(first.dependencies[0].dependsOn, [
    'pkg:cargo/ramshared-cli@0.1.0',
    'pkg:cargo/ramshared-wsl2d@0.1.0',
  ])
})

test('release_workspace_sbom_refuses_missing_wrong_or_conflicting_inputs', () => {
  const options = {
    tag: 'v0.9.0-beta.1',
    revision: '361427a63cbeb2a8b0ecafb224adeecb0539af9b',
    sourceDateEpoch: '1786697752',
  }
  assert.throws(() => mergeReleaseSboms([bom('ramshared-cli', '/tmp/root')], options), /exact release SBOM roots/)
  assert.throws(() => mergeReleaseSboms([
    bom('ramshared-cli', '/tmp/root'),
    bom('ramshared-agent', '/tmp/root'),
  ], options), /exact release SBOM roots/)

  const cli = bom('ramshared-cli', '/tmp/root')
  const daemon = bom('ramshared-wsl2d', '/tmp/root')
  daemon.components[0].version = '2.0.0'
  assert.throws(() => mergeReleaseSboms([cli, daemon], options), /conflicting component/)
  assert.throws(() => mergeReleaseSboms([cli, bom('ramshared-wsl2d', '/tmp/root')], {
    ...options,
    sourceDateEpoch: 'not-a-number',
  }), /source date epoch/)

  assert.throws(() => mergeReleaseSboms([cli, bom('ramshared-wsl2d', '/tmp/root')], {
    ...options,
    tag: 'invalid-tag',
  }), /exact release identity is required/)

  const badBom = bom('ramshared-cli', '/tmp/root')
  badBom.bomFormat = 'Invalid'
  assert.throws(() => mergeReleaseSboms([badBom, bom('ramshared-wsl2d', '/tmp/root')], options), /invalid cargo-cyclonedx input BOM/)

  const badDepBom = bom('ramshared-cli', '/tmp/root')
  badDepBom.dependencies[0].ref = 123
  assert.throws(() => mergeReleaseSboms([badDepBom, bom('ramshared-wsl2d', '/tmp/root')], options), /invalid dependency record/)

  const badLocalPathBom = bom('ramshared-cli', '/tmp/root')
  const badLocalPathBom2 = bom('ramshared-wsl2d', '/tmp/root')
  const res = mergeReleaseSboms([badLocalPathBom, badLocalPathBom2], options)
  const resultStr = JSON.stringify(res)
  assert.equal(resultStr.includes('file://'), false)

  const badLocalPathBom3 = bom('ramshared-cli', '/tmp/root')
  const badLocalPathBom4 = bom('ramshared-wsl2d', '/tmp/root')
  badLocalPathBom3.components[0].description = 'test file:// string';
  badLocalPathBom4.components[0].description = 'test file:// string';
  assert.throws(() => mergeReleaseSboms([badLocalPathBom3, badLocalPathBom4], options), /release SBOM contains a local path reference/)
})

test('release_workspace_sbom_cli_writes_once_and_refuses_unknown_or_clobber', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-sbom-'))
  try {
    const cli = path.join(root, 'cli.json')
    const daemon = path.join(root, 'daemon.json')
    const out = path.join(root, 'out', 'ramshared-sbom.cdx.json')
    writeFileSync(cli, JSON.stringify(bom('ramshared-cli', root)))
    writeFileSync(daemon, JSON.stringify(bom('ramshared-wsl2d', root)))
    const args = [
      'tools/ci/merge-release-sboms.mjs',
      '--tag', 'v0.9.0-beta.1',
      '--revision', '361427a63cbeb2a8b0ecafb224adeecb0539af9b',
      '--source-date-epoch', '1786697752',
      '--input', cli,
      '--input', daemon,
      '--out', out,
    ]
    const env = { ...process.env }
    delete env.NODE_TEST_CONTEXT
    const first = spawnSync(process.execPath, args, { encoding: 'utf8', env })
    assert.equal(first.status, 0, first.stderr)
    assert.equal(JSON.parse(readFileSync(out, 'utf8')).metadata.component.name, 'ramshared')

    const second = spawnSync(process.execPath, args, { encoding: 'utf8', env })
    assert.equal(second.status, 1)
    assert.match(second.stderr, /EEXIST/)
    const invalid = spawnSync(process.execPath, [
      'tools/ci/merge-release-sboms.mjs', '--unknown', 'value',
    ], { encoding: 'utf8', env })
    assert.equal(invalid.status, 1)
    assert.match(invalid.stderr, /unknown argument/)

    const missingVal = spawnSync(process.execPath, [
      'tools/ci/merge-release-sboms.mjs', '--input', '--out'
    ], { encoding: 'utf8', env })
    assert.equal(missingVal.status, 1)
    assert.match(missingVal.stderr, /missing value for --input/)

    const missingFile = spawnSync(process.execPath, [
      'tools/ci/merge-release-sboms.mjs',
      '--tag', 'v0.9.0-beta.1',
      '--revision', '361427a63cbeb2a8b0ecafb224adeecb0539af9b',
      '--source-date-epoch', '1786697752',
      '--input', path.join(root, 'non-existent.json'),
      '--input', daemon,
      '--out', out,
    ], { encoding: 'utf8', env })
    assert.equal(missingFile.status, 1)
    assert.match(missingFile.stderr, /failed to read input file/)

    const invalidJson = path.join(root, 'invalid.json')
    writeFileSync(invalidJson, 'not-json')
    const badFile = spawnSync(process.execPath, [
      'tools/ci/merge-release-sboms.mjs',
      '--tag', 'v0.9.0-beta.1',
      '--revision', '361427a63cbeb2a8b0ecafb224adeecb0539af9b',
      '--source-date-epoch', '1786697752',
      '--input', invalidJson,
      '--input', daemon,
      '--out', out,
    ], { encoding: 'utf8', env })
    assert.equal(badFile.status, 1)
    assert.match(badFile.stderr, /Unexpected token/)

    const missingArgs = spawnSync(process.execPath, [
      'tools/ci/merge-release-sboms.mjs',
      '--input', cli,
    ], { encoding: 'utf8', env })
    assert.equal(missingArgs.status, 1)
    assert.match(missingArgs.stderr, /two inputs and one output are required/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
