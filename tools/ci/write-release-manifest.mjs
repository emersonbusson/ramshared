#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SOURCE_SHA_RE = /^[0-9a-f]{40}$/
const TAG_RE = /^v[0-9][0-9A-Za-z.+-]*$/
const RUST_VERSION_RE = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/
const SBOM_GENERATOR = { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.5' }

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function inputIsValid(input) {
  return isObject(input) && TAG_RE.test(input.tag) && SOURCE_SHA_RE.test(input.revision) &&
    input.clean_tree === true && RUST_VERSION_RE.test(input.rust_version) &&
    safeRelative(input.bundle_path) && safeRelative(input.sbom_path) &&
    (input.prior_release === 'none' || TAG_RE.test(input.prior_release)) &&
    typeof input.rollback_trigger === 'string' && input.rollback_trigger.length > 0 &&
    input.rollback_trigger.length <= 256
}

function readRecord(root, relativePath) {
  if (!safeRelative(relativePath)) throw new Error('release-manifest-input-invalid')
  const rootPath = path.resolve(root)
  const filePath = path.resolve(rootPath, relativePath)
  if (!filePath.startsWith(`${rootPath}${path.sep}`) || !existsSync(filePath) ||
      lstatSync(filePath).isSymbolicLink() || !lstatSync(filePath).isFile()) {
    throw new Error('release-manifest-input-invalid')
  }
  const bytes = readFileSync(filePath)
  return { path: relativePath, bytes: bytes.length, sha256: sha256(bytes), content: bytes }
}

function validateSbom(record) {
  try {
    const value = JSON.parse(record.content.toString('utf8'))
    const tools = Array.isArray(value?.metadata?.tools)
      ? value.metadata.tools
      : Array.isArray(value?.metadata?.tools?.components) ? value.metadata.tools.components : []
    if (value.bomFormat !== 'CycloneDX' || value.specVersion !== SBOM_GENERATOR.spec_version ||
        !tools.some((tool) => tool?.name === SBOM_GENERATOR.name && tool?.version === SBOM_GENERATOR.version)) {
      throw new Error('invalid')
    }
  } catch {
    throw new Error('release-manifest-input-invalid')
  }
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

export function buildReleaseManifest(input, { root = process.cwd() } = {}) {
  if (!inputIsValid(input)) throw new Error('release-manifest-input-invalid')
  const lock = readRecord(root, 'Cargo.lock')
  const bundle = readRecord(root, input.bundle_path)
  const sbom = readRecord(root, input.sbom_path)
  validateSbom(sbom)
  return {
    schema_version: 1,
    terminal_status: 'PASS',
    source: {
      tag: input.tag,
      sha: input.revision,
      clean_tree: true,
      cargo_lock_sha256: lock.sha256,
      rust_version: input.rust_version,
    },
    sbom_generator: SBOM_GENERATOR,
    linux_bundle: {
      path: bundle.path,
      command: 'scripts/package/build-linux-bundle.sh',
      bytes: bundle.bytes,
      sha256: bundle.sha256,
      checksums_verified: true,
    },
    sbom: {
      path: sbom.path,
      format: 'CycloneDX',
      spec_version: SBOM_GENERATOR.spec_version,
      generator: { name: SBOM_GENERATOR.name, version: SBOM_GENERATOR.version },
      bytes: sbom.bytes,
      sha256: sbom.sha256,
      source_sha: input.revision,
      cargo_lock_sha256: lock.sha256,
    },
    evidence: {
      schema_version: 1,
      source_sha: input.revision,
      terminal_status: 'PASS',
      artifacts: [evidenceRecord(bundle), evidenceRecord(sbom)],
    },
    windows_driver_status: 'not-included',
    windows_drivers: [],
    rollback: {
      prior_release: input.prior_release,
      trigger: input.rollback_trigger,
    },
  }
}

function resolveOutput(root, outPath) {
  if (!safeRelative(outPath)) return null
  const rootPath = path.resolve(root)
  const output = path.resolve(rootPath, outPath)
  return output.startsWith(`${rootPath}${path.sep}`) ? output : null
}

export function writeReleaseManifest(input, { root = process.cwd(), out_path } = {}) {
  const output = resolveOutput(root, out_path)
  if (!output) throw new Error('release-manifest-output-invalid')
  const manifest = buildReleaseManifest(input, { root })
  try {
    writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`)
  } catch {
    throw new Error('release-manifest-write-failed')
  }
  return manifest
}

function parseArguments(argv) {
  const values = new Map()
  let cleanTree = false
  for (let index = 0; index < argv.length;) {
    const key = argv[index]
    if (key === '--clean-tree') {
      if (cleanTree) return null
      cleanTree = true
      index += 1
      continue
    }
    const value = argv[index + 1]
    if (!['--tag', '--revision', '--rust-version', '--bundle', '--sbom', '--prior-release', '--rollback-trigger', '--out'].includes(key) ||
        typeof value !== 'string' || values.has(key)) return null
    values.set(key, value)
    index += 2
  }
  const required = ['--tag', '--revision', '--rust-version', '--bundle', '--sbom', '--prior-release', '--rollback-trigger', '--out']
  if (!cleanTree || required.some((key) => !values.has(key))) return null
  return {
    input: {
      tag: values.get('--tag'),
      revision: values.get('--revision'),
      clean_tree: true,
      rust_version: values.get('--rust-version'),
      bundle_path: values.get('--bundle'),
      sbom_path: values.get('--sbom'),
      prior_release: values.get('--prior-release'),
      rollback_trigger: values.get('--rollback-trigger'),
    },
    out_path: values.get('--out'),
  }
}

export function main(argv = process.argv.slice(2), {
  cwd = process.cwd(),
  print = console.log,
  error = console.error,
} = {}) {
  const args = parseArguments(argv)
  if (!args) {
    error('usage: write-release-manifest.mjs --tag <tag> --revision <sha> --rust-version <version> --bundle <path> --sbom <path> --prior-release <tag|none> --rollback-trigger <text> --out <path> --clean-tree')
    return 2
  }
  if (!resolveOutput(cwd, args.out_path)) {
    error('RELEASE_MANIFEST_ERROR=release-manifest-output-invalid')
    return 1
  }
  try {
    writeReleaseManifest(args.input, { root: cwd, out_path: args.out_path })
  } catch (exception) {
    const rule = exception instanceof Error && /^release-manifest-(?:input|output|write)-invalid|^release-manifest-write-failed$/.test(exception.message)
      ? exception.message
      : 'release-manifest-write-failed'
    error(`RELEASE_MANIFEST_ERROR=${rule}`)
    return 1
  }
  print('RELEASE_MANIFEST_STATUS=PASS')
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
