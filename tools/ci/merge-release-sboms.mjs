#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const EXPECTED_ROOTS = ['ramshared-cli', 'ramshared-wsl2d']
const EXPECTED_TOOL = [{ vendor: 'CycloneDX', name: 'cargo-cyclonedx', version: '0.5.9' }]

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical)
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]))
  }
  return value
}

function canonicalText(value) {
  return JSON.stringify(canonical(value))
}

function normalizeReference(value) {
  if (typeof value !== 'string') return value
  const match = value.match(/^path\+file:\/\/.*\/crates\/([^/#]+)#([^\s]+)(?:\s+(.+))?$/)
  if (!match) return value
  const [, name, version, target] = match
  return `pkg:cargo/${name}@${version}${target ? `#${encodeURIComponent(target)}` : ''}`
}

function normalizePurl(value) {
  if (typeof value !== 'string') return value
  const match = value.match(/^(pkg:cargo\/[^@]+@[^?#]+)\?download_url=file:\/\/[^#]*(#.*)?$/)
  return match ? `${match[1]}${match[2] ?? ''}` : value
}

function normalize(value, key = '') {
  if (Array.isArray(value)) return value.map((item) => normalize(item, key))
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([childKey, childValue]) => [
      childKey,
      normalize(childValue, childKey),
    ]))
  }
  if (key === 'bom-ref' || key === 'ref' || key === 'dependsOn') return normalizeReference(value)
  if (key === 'purl') return normalizePurl(value)
  return value
}

function deterministicUuid(tag, revision) {
  const bytes = createHash('sha256')
    .update('ramshared-release-sbom-v1\0')
    .update(tag)
    .update('\0')
    .update(revision)
    .digest()
    .subarray(0, 16)
  bytes[6] = (bytes[6] & 0x0f) | 0x50
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = bytes.toString('hex')
  return `urn:uuid:${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

function requireInputBom(bom) {
  if (!bom || bom.bomFormat !== 'CycloneDX' || bom.specVersion !== '1.5' || bom.version !== 1 ||
      !bom.metadata?.component || canonicalText(bom.metadata.tools) !== canonicalText(EXPECTED_TOOL) ||
      !Array.isArray(bom.components) || !Array.isArray(bom.dependencies)) {
    throw new Error('invalid cargo-cyclonedx input BOM')
  }
}

function insertExactComponent(components, component) {
  const ref = component?.['bom-ref']
  if (typeof ref !== 'string' || ref.length === 0) throw new Error('component bom-ref is required')
  const prior = components.get(ref)
  if (prior && canonicalText(prior) !== canonicalText(component)) {
    throw new Error(`conflicting component: ${ref}`)
  }
  components.set(ref, component)
}

export function mergeReleaseSboms(inputBoms, { tag, revision, sourceDateEpoch }) {
  if (!Array.isArray(inputBoms) || inputBoms.length !== EXPECTED_ROOTS.length) {
    throw new Error('exact release SBOM roots are required')
  }
  if (!/^v0\.9\.0-beta\.1$/.test(tag ?? '') || !/^[0-9a-f]{40}$/.test(revision ?? '')) {
    throw new Error('exact release identity is required')
  }
  if (!/^(?:0|[1-9][0-9]*)$/.test(sourceDateEpoch ?? '')) {
    throw new Error('source date epoch must be canonical decimal')
  }
  const epochMillis = Number(sourceDateEpoch) * 1000
  if (!Number.isSafeInteger(epochMillis)) throw new Error('source date epoch is out of range')

  for (const bom of inputBoms) requireInputBom(bom)
  const normalized = inputBoms.map((bom) => normalize(bom))
  normalized.sort((left, right) => left.metadata.component.name.localeCompare(right.metadata.component.name))
  if (canonicalText(normalized.map((bom) => bom.metadata.component.name)) !== canonicalText(EXPECTED_ROOTS)) {
    throw new Error('exact release SBOM roots are required')
  }

  const components = new Map()
  const dependencies = new Map()
  for (const bom of normalized) {
    insertExactComponent(components, bom.metadata.component)
    for (const component of bom.components) insertExactComponent(components, component)
    for (const dependency of bom.dependencies) {
      const dependsOn = dependency.dependsOn ?? []
      if (typeof dependency.ref !== 'string' || !Array.isArray(dependsOn)) {
        throw new Error('invalid dependency record')
      }
      const refs = dependencies.get(dependency.ref) ?? new Set()
      for (const ref of dependsOn) {
        if (typeof ref !== 'string') throw new Error('invalid dependency target')
        refs.add(ref)
      }
      dependencies.set(dependency.ref, refs)
    }
  }

  const version = tag.slice(1)
  const releaseRef = `pkg:generic/ramshared@${version}`
  const rootComponents = normalized.map((bom) => bom.metadata.component)
  const dependencyRecords = [...dependencies.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([ref, refs]) => ({ ref, dependsOn: [...refs].sort() }))

  const result = {
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    serialNumber: deterministicUuid(tag, revision),
    version: 1,
    metadata: {
      timestamp: new Date(epochMillis).toISOString(),
      tools: EXPECTED_TOOL,
      component: {
        type: 'application',
        'bom-ref': releaseRef,
        name: 'ramshared',
        version,
        components: rootComponents,
      },
      properties: [
        { name: 'ramshared:release:tag', value: tag },
        { name: 'ramshared:source:revision', value: revision },
      ],
    },
    components: [...components.values()].sort((left, right) => left['bom-ref'].localeCompare(right['bom-ref'])),
    dependencies: [
      { ref: releaseRef, dependsOn: rootComponents.map((component) => component['bom-ref']).sort() },
      ...dependencyRecords,
    ],
  }
  const serialized = canonicalText(result)
  if (serialized.includes('file://') || serialized.includes('path+file:')) {
    throw new Error('release SBOM contains a local path reference')
  }
  return canonical(result)
}

function parseArgs(argv) {
  const options = { inputs: [] }
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index]
    const value = argv[index + 1]
    if (!value) throw new Error(`missing value for ${flag}`)
    if (flag === '--input') options.inputs.push(value)
    else if (flag === '--tag') options.tag = value
    else if (flag === '--revision') options.revision = value
    else if (flag === '--source-date-epoch') options.sourceDateEpoch = value
    else if (flag === '--out') options.out = value
    else throw new Error(`unknown argument: ${flag}`)
  }
  if (options.inputs.length !== EXPECTED_ROOTS.length || !options.out) {
    throw new Error('two inputs and one output are required')
  }
  return options
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  const boms = options.inputs.map((input) => JSON.parse(readFileSync(input, 'utf8')))
  const merged = mergeReleaseSboms(boms, options)
  mkdirSync(path.dirname(options.out), { recursive: true })
  writeFileSync(options.out, `${JSON.stringify(merged, null, 2)}\n`, { flag: 'wx' })
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main()
  } catch (error) {
    process.stderr.write(`release-sbom-merge: ${error.message}\n`)
    process.exitCode = 1
  }
}
