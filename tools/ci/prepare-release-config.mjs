#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/

function versionFromCargo(root) {
  const match = readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)
  return match?.[1] ?? null
}

function versionFromManifest(root) {
  const manifest = JSON.parse(readFileSync(path.join(root, '.release-please-manifest.json'), 'utf8'))
  return manifest['.']
}

function compareVersions(left, right) {
  const a = left.split('.').map(Number)
  const b = right.split('.').map(Number)
  return a.findIndex((value, index) => value !== b[index]) >= 0
    ? a.find((value, index) => value !== b[index]) - b[a.findIndex((value, index) => value !== b[index])]
    : 0
}

export function prepareReleaseConfig({ root = ROOT, outPath } = {}) {
  const config = JSON.parse(readFileSync(path.join(root, 'release-please-config.json'), 'utf8'))
  const cargoVersion = versionFromCargo(root)
  const manifestVersion = versionFromManifest(root)
  if (!SEMVER_RE.test(cargoVersion ?? '') || !SEMVER_RE.test(manifestVersion ?? '')) {
    throw new Error('release-config-version-invalid')
  }

  // A merged feature PR may already have advanced Cargo.toml before the
  // release PR is created. Promote that declared version once; afterwards
  // release-please resumes normal Conventional Commit bumping.
  if (compareVersions(cargoVersion, manifestVersion) > 0) config['release-as'] = cargoVersion
  else delete config['release-as']

  const output = outPath ?? path.join(root, '.release-please-runtime-config.json')
  writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`)
  return { output, cargo_version: cargoVersion, manifest_version: manifestVersion, release_as: config['release-as'] ?? null }
}

function main() {
  const result = prepareReleaseConfig({ root: ROOT })
  console.log(`RELEASE_CONFIG_STATUS=PASS`)
  console.log(`RELEASE_CONFIG_CARGO_VERSION=${result.cargo_version}`)
  console.log(`RELEASE_CONFIG_MANIFEST_VERSION=${result.manifest_version}`)
  console.log(`RELEASE_CONFIG_RELEASE_AS=${result.release_as ?? 'conventional-commits'}`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
