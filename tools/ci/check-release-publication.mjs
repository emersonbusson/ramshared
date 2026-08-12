#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

import { validateReleaseManifest } from './check-release-integrity.mjs'

const SOURCE_SHA_RE = /^[0-9a-f]{40}$/
const SHA256_RE = /^[0-9a-f]{64}$/
const TARGET_TAG = 'v0.9.0-beta.1'
const TARGET_ASSETS = [
  `ramshared-linux-${TARGET_TAG}.tar.gz`,
  `ramshared-linux-${TARGET_TAG}.tar.gz.sha256`,
  'ramshared-sbom.cdx.json',
  'release-manifest.json',
]

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function add(errors, rule) {
  if (!errors.includes(rule)) errors.push(rule)
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function validRecord(value) {
  return isObject(value) && typeof value.name === 'string' &&
    Number.isInteger(value.bytes) && value.bytes >= 0 &&
    typeof value.sha256 === 'string' && SHA256_RE.test(value.sha256)
}

function orderedNames(records) {
  return Array.isArray(records) ? records.map((record) => record?.name) : []
}

function sameStrings(left, right) {
  return Array.isArray(left) && left.length === right.length &&
    left.every((value, index) => value === right[index])
}

export function validateReleasePromotionPolicy(policy) {
  const errors = []
  if (!isObject(policy) || policy.schema_version !== 1 ||
      policy.target_tag !== TARGET_TAG || policy.release_channel !== 'beta' ||
      !sameStrings(policy.public_assets, TARGET_ASSETS)) {
    add(errors, 'release-promotion-policy-invalid')
  }
  return { ok: errors.length === 0, errors }
}

export function validatePublicationInput(input, policy) {
  const errors = [...validateReleasePromotionPolicy(policy).errors]
  if (!isObject(input) || input.tag !== policy?.target_tag || !SOURCE_SHA_RE.test(input.source_sha) ||
      typeof input.integrity_run_id !== 'string' || !/^[1-9][0-9]{0,18}$/.test(input.integrity_run_id)) {
    add(errors, 'publication-input-invalid')
  }
  return { ok: errors.length === 0, errors: errors.sort() }
}

export function validatePublicationCandidate(policy, candidate) {
  const errors = [...validateReleasePromotionPolicy(policy).errors]
  if (!isObject(candidate) || !isObject(candidate.source) ||
      candidate.source.tag !== policy?.target_tag || !SOURCE_SHA_RE.test(candidate.source.sha)) {
    add(errors, 'candidate-source-invalid')
  }
  if (!Array.isArray(candidate?.public_assets) || !sameStrings(orderedNames(candidate.public_assets), policy?.public_assets ?? []) ||
      candidate.public_assets.some((record) => !validRecord(record))) {
    add(errors, 'candidate-assets-invalid')
  }
  return { ok: errors.length === 0, errors: errors.sort() }
}

export function validatePublicationBinding(input, candidate, policy) {
  const errors = [
    ...validatePublicationInput(input, policy).errors,
    ...validatePublicationCandidate(policy, candidate).errors,
  ]
  if (candidate?.source?.tag !== input?.tag || candidate?.source?.sha !== input?.source_sha) {
    add(errors, 'candidate-dispatch-mismatch')
  }
  return { ok: errors.length === 0, errors: [...new Set(errors)].sort() }
}

function normalizeAsset(record) {
  if (isObject(record) && typeof record.digest === 'string' && /^sha256:[0-9a-f]{64}$/.test(record.digest)) {
    return { name: record.name, bytes: record.size, sha256: record.digest.slice('sha256:'.length) }
  }
  return record
}

function normalizeRemoteRelease(remote) {
  if (!isObject(remote)) return null
  if ('draft' in remote || 'prerelease' in remote || 'tag_name' in remote) {
    // GitHub models target_commitish as the branch or commit supplied at
    // release creation; its documented response examples retain a branch
    // name. The immutable source binding is instead the tag resolved by the
    // protected workflow plus the integrity-run and manifest SHA checks.
    return {
      is_draft: remote.draft,
      is_prerelease: remote.prerelease,
      tag: remote.tag_name,
      assets: Array.isArray(remote.assets) ? remote.assets.map(normalizeAsset) : remote.assets,
    }
  }
  return remote
}

function remoteAssetsMatch(localAssets, remoteAssets, errors) {
  if (!Array.isArray(remoteAssets)) {
    add(errors, 'remote-assets-invalid')
    return []
  }
  const localByName = new Map(localAssets.map((record) => [record.name, record]))
  const seen = new Set()
  for (const record of remoteAssets) {
    if (!validRecord(record) || !localByName.has(record.name) || seen.has(record.name)) {
      add(errors, 'remote-assets-invalid')
      continue
    }
    seen.add(record.name)
    const local = localByName.get(record.name)
    if (local.bytes !== record.bytes || local.sha256 !== record.sha256) add(errors, 'remote-asset-mismatch')
  }
  return localAssets.filter((record) => !seen.has(record.name)).map((record) => record.name)
}

export function planPublication(policy, candidate, remote) {
  const errors = [...validatePublicationCandidate(policy, candidate).errors]
  if (errors.length > 0) return { status: 'NO_GO', errors: errors.sort(), create_draft: false, upload_assets: [], publish: false }

  const release = normalizeRemoteRelease(remote)
  if (!release) {
    return { status: 'NO_GO', errors: ['remote-release-absent'], create_draft: false, upload_assets: [], publish: false }
  }
  if (release.tag !== undefined && release.tag !== policy.target_tag) add(errors, 'remote-release-identity-invalid')
  if (typeof release.is_draft !== 'boolean' || release.is_prerelease !== true) add(errors, 'remote-release-mode-invalid')
  const missing = remoteAssetsMatch(candidate.public_assets, release.assets, errors)
  if (errors.length > 0) return { status: 'NO_GO', errors: errors.sort(), create_draft: false, upload_assets: [], publish: false }

  if (!release.is_draft) {
    if (missing.length > 0) {
      return { status: 'NO_GO', errors: ['remote-release-mode-invalid'], create_draft: false, upload_assets: [], publish: false }
    }
    return { status: 'NO_CHANGE', errors: [], create_draft: false, upload_assets: [], publish: false }
  }
  return {
    status: 'ADVANCE',
    errors: [],
    create_draft: false,
    upload_assets: missing,
    publish: true,
  }
}

export function candidateFromReleaseManifest(manifest, { root } = {}) {
  const errors = []
  const integrity = validateReleaseManifest(manifest, { root })
  for (const error of integrity.errors) add(errors, `integrity-${error}`)
  const names = manifest?.public_assets
  if (!Array.isArray(names) || !names.every((name) => typeof name === 'string' && safeRelative(name) && !name.includes('/'))) {
    add(errors, 'candidate-assets-invalid')
  }
  const records = []
  const rootPath = root ? path.resolve(root) : null
  const artifactDirectory = typeof manifest?.linux_bundle?.path === 'string'
    ? path.dirname(manifest.linux_bundle.path)
    : null
  if (rootPath && safeRelative(artifactDirectory) && Array.isArray(names)) {
    for (const name of names) {
      const assetPath = path.resolve(rootPath, artifactDirectory, name)
      if (!assetPath.startsWith(`${rootPath}${path.sep}`) || !existsSync(assetPath) ||
          lstatSync(assetPath).isSymbolicLink() || !statSync(assetPath).isFile()) {
        add(errors, 'candidate-asset-file-invalid')
        continue
      }
      const bytes = readFileSync(assetPath)
      records.push({ name, bytes: bytes.length, sha256: sha256(bytes) })
    }
  } else {
    add(errors, 'candidate-root-invalid')
  }
  if (errors.length > 0) throw new Error([...new Set(errors)].sort().join(','))
  return { source: { tag: manifest.source.tag, sha: manifest.source.sha }, public_assets: records }
}

function readJson(filePath) {
  try {
    return JSON.parse(readFileSync(filePath, 'utf8'))
  } catch {
    throw new Error('publication-json-read-failed')
  }
}

function resolveRelative(root, value, { allowRoot = false } = {}) {
  if (!safeRelative(value)) return null
  const rootPath = path.resolve(root)
  const resolved = path.resolve(rootPath, value)
  return resolved === rootPath ? (allowRoot ? resolved : null) :
    resolved.startsWith(`${rootPath}${path.sep}`) ? resolved : null
}

function writeJson(root, relativePath, value) {
  const output = resolveRelative(root, relativePath)
  if (!output) throw new Error('publication-output-invalid')
  try {
    writeFileSync(output, `${JSON.stringify(value, null, 2)}\n`)
  } catch {
    throw new Error('publication-write-failed')
  }
}

function parseArguments(argv) {
  const values = new Map()
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]
    const value = argv[index + 1]
    if (!['--candidate', '--manifest', '--out', '--policy', '--remote', '--root', '--source-sha', '--tag', '--integrity-run-id'].includes(key) ||
        typeof value !== 'string' || values.has(key)) return null
    values.set(key, value)
  }
  if (!values.has('--policy')) return null
  const inputKeys = ['--policy', '--tag', '--source-sha', '--integrity-run-id']
  if (inputKeys.every((key) => values.has(key)) && values.size === inputKeys.length) return { kind: 'input', values }

  const common = ['--policy', '--out']
  if (common.some((key) => !values.has(key))) return null
  if (values.has('--manifest')) {
    const required = [...common, '--manifest', '--root', '--tag', '--source-sha', '--integrity-run-id']
    if (required.some((key) => !values.has(key)) || values.has('--candidate') || values.has('--remote')) return null
    return { kind: 'candidate', values }
  }
  const required = [...common, '--candidate', '--remote']
  if (required.some((key) => !values.has(key)) || values.size !== required.length) return null
  return { kind: 'plan', values }
}

export function main(argv = process.argv.slice(2), { cwd = process.cwd(), print = console.log, error = console.error } = {}) {
  const args = parseArguments(argv)
  if (!args) {
    error('usage: check-release-publication.mjs --policy <path> --tag <tag> --source-sha <sha> --integrity-run-id <id> | --policy <path> --manifest <path> --root <directory> --tag <tag> --source-sha <sha> --integrity-run-id <id> --out <path> | --policy <path> --candidate <path> --remote <path> --out <path>')
    return 2
  }
  try {
    const policyPath = resolveRelative(cwd, args.values.get('--policy'))
    if (!policyPath) throw new Error('publication-policy-path-invalid')
    const policy = readJson(policyPath)
    if (args.kind === 'input') {
      const inputResult = validatePublicationInput({
        tag: args.values.get('--tag'),
        source_sha: args.values.get('--source-sha'),
        integrity_run_id: args.values.get('--integrity-run-id'),
      }, policy)
      if (!inputResult.ok) throw new Error(inputResult.errors.join(','))
      print('RELEASE_PUBLICATION_INPUT=PASS')
      return 0
    }
    if (args.kind === 'candidate') {
      const input = {
        tag: args.values.get('--tag'),
        source_sha: args.values.get('--source-sha'),
        integrity_run_id: args.values.get('--integrity-run-id'),
      }
      const inputResult = validatePublicationInput(input, policy)
      if (!inputResult.ok) throw new Error(inputResult.errors.join(','))
      const root = resolveRelative(cwd, args.values.get('--root'), { allowRoot: true })
      const manifestPath = resolveRelative(cwd, args.values.get('--manifest'))
      if (!root || !manifestPath) throw new Error('publication-candidate-path-invalid')
      const candidate = candidateFromReleaseManifest(readJson(manifestPath), { root })
      const binding = validatePublicationBinding(input, candidate, policy)
      if (!binding.ok) {
        throw new Error(binding.errors.join(','))
      }
      writeJson(cwd, args.values.get('--out'), candidate)
      print('RELEASE_PUBLICATION_CANDIDATE=PASS')
      return 0
    }
    const candidatePath = resolveRelative(cwd, args.values.get('--candidate'))
    const remotePath = resolveRelative(cwd, args.values.get('--remote'))
    if (!candidatePath || !remotePath) throw new Error('publication-plan-path-invalid')
    const plan = planPublication(policy, readJson(candidatePath), readJson(remotePath))
    writeJson(cwd, args.values.get('--out'), plan)
    print(`RELEASE_PUBLICATION_STATUS=${plan.status}`)
    for (const rule of plan.errors) error(`RELEASE_PUBLICATION_ERROR=${rule}`)
    return plan.status === 'NO_GO' ? 1 : 0
  } catch (exception) {
    const message = exception instanceof Error ? exception.message : ''
    const rule = /^[a-z0-9,-]+$/.test(message) ? message : 'publication-check-failed'
    error(`RELEASE_PUBLICATION_ERROR=${rule}`)
    return 1
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
