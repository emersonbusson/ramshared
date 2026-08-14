#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import { validateArtifactManifest } from './check-ci-artifacts.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const SOURCE_SHA_RE = /^[0-9a-f]{40}$/
const SHA256_RE = /^[0-9a-f]{64}$/
const TAG_RE = /^v[0-9][0-9A-Za-z.+-]*$/
const RUST_VERSION_RE = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/
const SBOM_GENERATOR = { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.5' }
const DRIVER_SIGNING = new Set(['test-signed', 'unknown', 'untrusted', 'production-trusted'])
const TARGET_TAG = 'v0.9.0-beta.1'

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function add(errors, rule) {
  if (!errors.includes(rule)) errors.push(rule)
}

function releaseFile(record, root, errors) {
  if (!isObject(record) || !safeRelative(record.path) ||
      !Number.isInteger(record.bytes) || record.bytes < 0 ||
      typeof record.sha256 !== 'string' || !SHA256_RE.test(record.sha256)) {
    add(errors, 'release-file-record-invalid')
    return null
  }
  if (!root) return null
  const rootPath = path.resolve(root)
  const filePath = path.resolve(rootPath, record.path)
  if (!filePath.startsWith(`${rootPath}${path.sep}`)) {
    add(errors, 'release-file-path-unsafe')
    return null
  }
  if (!existsSync(filePath)) {
    add(errors, 'release-file-missing')
    return null
  }
  if (lstatSync(filePath).isSymbolicLink() || !statSync(filePath).isFile()) {
    add(errors, 'release-file-invalid')
    return null
  }
  const bytes = readFileSync(filePath)
  if (bytes.length !== record.bytes) add(errors, 'release-file-size-mismatch')
  if (sha256(bytes) !== record.sha256) add(errors, 'release-file-hash-mismatch')
  return { path: filePath, bytes }
}

export function validateSourceBinding(manifest, {
  root,
  expected_tag,
  expected_revision,
} = {}) {
  const errors = []
  const source = manifest?.source
  if (!isObject(source) || !TAG_RE.test(source.tag) || !SOURCE_SHA_RE.test(source.sha) ||
      typeof source.clean_tree !== 'boolean' || !SHA256_RE.test(source.cargo_lock_sha256) ||
      !RUST_VERSION_RE.test(source.rust_version)) {
    add(errors, 'source-binding-invalid')
    return { ok: false, errors }
  }
  if (!source.clean_tree) add(errors, 'source-tree-dirty')
  if (expected_tag !== undefined && source.tag !== expected_tag) add(errors, 'source-tag-mismatch')
  if (expected_revision !== undefined && source.sha !== expected_revision) add(errors, 'source-revision-mismatch')
  if (root) {
    const lockPath = path.join(root, 'Cargo.lock')
    if (!existsSync(lockPath) || !lstatSync(lockPath).isFile()) add(errors, 'source-lock-missing')
    else if (sha256(readFileSync(lockPath)) !== source.cargo_lock_sha256) add(errors, 'source-lock-hash-mismatch')
  }
  return { ok: errors.length === 0, errors: errors.sort() }
}

function validSbomGenerator(value, requireSpecVersion = false) {
  return isObject(value) && value.name === SBOM_GENERATOR.name &&
    value.version === SBOM_GENERATOR.version &&
    (!requireSpecVersion || value.spec_version === SBOM_GENERATOR.spec_version)
}

function sbomTools(value) {
  if (Array.isArray(value?.metadata?.tools)) return value.metadata.tools
  if (Array.isArray(value?.metadata?.tools?.components)) return value.metadata.tools.components
  return []
}

function exactSbomMetadata(parsed, manifest) {
  const component = parsed?.metadata?.component
  const roots = component?.components
  const properties = parsed?.metadata?.properties
  return isObject(component) && component.type === 'application' && component.name === 'ramshared' &&
    component.version === manifest.source.tag.slice(1) &&
    Array.isArray(roots) && roots.length === 2 &&
    roots[0]?.name === 'ramshared-cli' && roots[1]?.name === 'ramshared-wsl2d' &&
    Array.isArray(properties) && properties.length === 2 &&
    properties[0]?.name === 'ramshared:release:tag' && properties[0]?.value === manifest.source.tag &&
    properties[1]?.name === 'ramshared:source:revision' && properties[1]?.value === manifest.source.sha
}

function validateSbom(manifest, root, errors) {
  const sbom = manifest.sbom
  if (!isObject(sbom)) {
    add(errors, 'sbom-invalid')
    return
  }
  if (sbom.format !== 'CycloneDX' || sbom.spec_version !== SBOM_GENERATOR.spec_version) add(errors, 'sbom-format-invalid')
  if (!validSbomGenerator(manifest.sbom_generator, true) || !validSbomGenerator(sbom.generator)) add(errors, 'sbom-generator-mismatch')
  if (sbom.source_sha !== manifest?.source?.sha || sbom.cargo_lock_sha256 !== manifest?.source?.cargo_lock_sha256) {
    add(errors, 'sbom-source-binding-mismatch')
  }
  const file = releaseFile(sbom, root, errors)
  if (!file) return
  try {
    const parsed = JSON.parse(file.bytes.toString('utf8'))
    const hasGenerator = sbomTools(parsed).some((tool) =>
      tool?.name === SBOM_GENERATOR.name && tool?.version === SBOM_GENERATOR.version
    )
    const serialized = JSON.stringify(parsed)
    if (parsed.bomFormat !== 'CycloneDX' || parsed.specVersion !== SBOM_GENERATOR.spec_version || !hasGenerator ||
        !exactSbomMetadata(parsed, manifest) || serialized.includes('file://') || serialized.includes('path+file:')) {
      add(errors, 'sbom-content-invalid')
    }
  } catch {
    add(errors, 'sbom-content-invalid')
  }
}

export function validatePublicDriverEligibility(drivers) {
  const errors = []
  if (!Array.isArray(drivers)) return { ok: false, errors: ['windows-drivers-invalid'] }
  for (const driver of drivers) {
    if (!isObject(driver) || typeof driver.component !== 'string' || !driver.component ||
        !DRIVER_SIGNING.has(driver.signing) || typeof driver.attested !== 'boolean' ||
        typeof driver.public_eligible !== 'boolean') {
      add(errors, 'windows-driver-record-invalid')
      continue
    }
    if (!driver.public_eligible) continue
    if (driver.signing === 'test-signed') add(errors, 'windows-driver-test-signed-public')
    else if (driver.signing === 'unknown' || driver.signing === 'untrusted') add(errors, 'windows-driver-untrusted-public')
    else if (!driver.attested) add(errors, 'windows-driver-unattested-public')
  }
  return { ok: errors.length === 0, errors: errors.sort() }
}

function evidenceContains(evidence, record) {
  return Array.isArray(evidence?.artifacts) && evidence.artifacts.some((item) =>
    item?.path === record?.path && item?.bytes === record?.bytes && item?.sha256 === record?.sha256
  )
}

function validateEvidence(manifest, root, errors) {
  const evidence = manifest.evidence
  const result = validateArtifactManifest(evidence, { root })
  for (const rule of result.errors) add(errors, `release-evidence-${rule}`)
  if (!evidenceContains(evidence, manifest.linux_bundle) || !evidenceContains(evidence, manifest.detached_checksum) ||
      !evidenceContains(evidence, manifest.sbom)) {
    add(errors, 'release-evidence-record-missing')
  }
}

function sameStrings(left, right) {
  return Array.isArray(left) && left.length === right.length && left.every((value, index) => value === right[index])
}

function validatePublicAssets(manifest, root, errors) {
  const tag = manifest?.source?.tag
  const expected = [
    `ramshared-linux-${tag}.tar.gz`,
    `ramshared-linux-${tag}.tar.gz.sha256`,
    'ramshared-sbom.cdx.json',
    'release-manifest.json',
  ]
  if (tag !== TARGET_TAG || !sameStrings(manifest?.public_assets, expected)) add(errors, 'release-public-assets-invalid')
  const checksum = manifest?.detached_checksum
  if (!isObject(checksum) || checksum.archive !== manifest?.linux_bundle?.path || checksum.algorithm !== 'sha256' ||
      checksum.path !== `${manifest?.linux_bundle?.path}.sha256`) {
    add(errors, 'detached-checksum-invalid')
    return
  }
  const record = releaseFile(checksum, root, errors)
  if (!record || !isObject(manifest?.linux_bundle)) return
  const expectedChecksum = `${manifest.linux_bundle.sha256}  ${path.basename(manifest.linux_bundle.path)}\n`
  if (record.bytes.toString('utf8') !== expectedChecksum) add(errors, 'detached-checksum-content-invalid')
  if (path.basename(manifest.linux_bundle.path) !== expected[0] || path.basename(checksum.path) !== expected[1] ||
      path.basename(manifest.sbom?.path ?? '') !== expected[2]) {
    add(errors, 'release-public-assets-invalid')
  }
}

function validateRollback(value, errors) {
  if (!isObject(value) || (value.prior_release !== 'none' && !TAG_RE.test(value.prior_release)) ||
      typeof value.trigger !== 'string' || value.trigger.length === 0 || value.trigger.length > 256) {
    add(errors, 'rollback-invalid')
  }
}

export function validateReleaseManifest(manifest, options = {}) {
  const errors = []
  if (!isObject(manifest) || manifest.schema_version !== 1 || manifest.terminal_status !== 'PASS') {
    add(errors, 'release-manifest-schema-invalid')
    return { ok: false, errors }
  }
  const source = validateSourceBinding(manifest, options)
  for (const rule of source.errors) add(errors, rule)

  const bundle = manifest.linux_bundle
  if (!isObject(bundle) || bundle.command !== 'scripts/package/build-linux-bundle.sh' || bundle.checksums_verified !== true) {
    add(errors, 'linux-bundle-invalid')
  }
  releaseFile(bundle, options.root, errors)
  validatePublicAssets(manifest, options.root, errors)
  validateSbom(manifest, options.root, errors)
  validateEvidence(manifest, options.root, errors)

  if (!['not-included', 'present'].includes(manifest.windows_driver_status) ||
      (manifest.windows_driver_status === 'not-included' && manifest.windows_drivers?.length !== 0) ||
      (manifest.windows_driver_status === 'present' && manifest.windows_drivers?.length === 0)) {
    add(errors, 'windows-driver-status-invalid')
  }
  const drivers = validatePublicDriverEligibility(manifest.windows_drivers)
  for (const rule of drivers.errors) add(errors, rule)
  validateRollback(manifest.rollback, errors)
  return { ok: errors.length === 0, errors: errors.sort() }
}

function parseArguments(argv) {
  const values = new Map()
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]
    const value = argv[index + 1]
    if (!['--check', '--root', '--tag', '--revision'].includes(key) || typeof value !== 'string' || values.has(key)) return null
    values.set(key, value)
  }
  if (!values.has('--check')) return null
  return {
    manifest_path: values.get('--check'),
    root: values.get('--root'),
    expected_tag: values.get('--tag'),
    expected_revision: values.get('--revision'),
  }
}

export function main(argv = process.argv.slice(2), { print = console.log, error = console.error } = {}) {
  const args = parseArguments(argv)
  if (!args) {
    error('usage: check-release-integrity.mjs --check <manifest> [--root <directory>] [--tag <tag>] [--revision <sha>]')
    return 2
  }
  let manifest
  try {
    manifest = JSON.parse(readFileSync(args.manifest_path, 'utf8'))
  } catch {
    error('RELEASE_INTEGRITY_ERROR=manifest-read-failed')
    return 1
  }
  const root = args.root ?? path.dirname(args.manifest_path)
  const result = validateReleaseManifest(manifest, {
    root,
    expected_tag: args.expected_tag,
    expected_revision: args.expected_revision,
  })
  if (!result.ok) {
    for (const rule of result.errors) error(`RELEASE_INTEGRITY_ERROR=${rule}`)
    print('RELEASE_INTEGRITY_STATUS=FAIL')
    print('RELEASE_INTEGRITY_VERDICT=NO-GO')
    return 1
  }
  print('RELEASE_INTEGRITY_STATUS=PASS')
  print('RELEASE_INTEGRITY_VERDICT=PASS')
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
