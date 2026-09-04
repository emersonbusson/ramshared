#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, lstatSync, readFileSync, realpathSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { TextDecoder } from 'node:util'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const DEFAULT_MAP = 'docs/governance/rust-slice-coverage.json'
const COVERAGE_SCRIPT = 'tools/ci/check-rust-slice-coverage.mjs'
const MAP_SCHEMA_VERSION = 2
const LINE_COVERAGE_KIND = 'rust-line-coverage'
const WINDOWS_PLATFORM_KIND = 'windows-platform-e2e'
const LOCALIZATION_KIND = 'rust-localization-comment-differential'
const TEST_ONLY_LOCALIZATION_KIND = 'rust-test-only-localization-differential'
const IGNORED_TEST_RELOCATION_KIND = 'rust-ignored-test-relocation'
const MODULE_EXPORT_GLUE_KIND = 'rust-module-export-glue-differential'
const STRUCTURAL_KIND = 'rust-structural-contract'
const PLATFORM_MARKER = 'rust-slice-platform-e2e-v1'
const LOCALIZATION_MARKER = 'rust-slice-localization-comment-differential-v1'
const TEST_ONLY_LOCALIZATION_MARKER = 'rust-slice-test-only-localization-differential-v1'
const IGNORED_TEST_RELOCATION_MARKER = 'rust-slice-ignored-test-relocation-v1'
const MODULE_EXPORT_GLUE_MARKER = 'rust-slice-module-export-glue-differential-v1'
const STRUCTURAL_MARKER = 'rust-slice-structural-contract-v1'
const WINDOWS_STATIC_WRAPPER = 'scripts/windows/Test-WindowsCiStatic.ps1'
const VALIDATION_EVIDENCE_PATH = 'validation.md'
const MODULE_EXPORT_GLUE_PATH = 'crates/ramshared-tier/src/lib.rs'
const MODULE_EXPORT_GLUE_PACKAGE = 'ramshared-tier'
const MODULE_EXPORT_GLUE_DECLARATION = 'pub mod n3_state;\npub mod nbd_readiness;'
const MODULE_EXPORT_GLUE_CARGO_TEST = ['cargo', 'test', '-p', MODULE_EXPORT_GLUE_PACKAGE, '--all-targets']
const FULL_SHA = /^[0-9a-f]{40}$/i
const SHA256 = /^[0-9a-f]{64}$/i
const TEST_NAME = /^[a-z][a-z0-9_]*$/
const MAX_TRUSTED_FILE_BYTES = 2 * 1024 * 1024
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true })
const UNSAFE_PATH_CHARACTER = /[\p{Cc}\p{Cf}\\]/u

function finding(rule, detail = '') {
  return { rule, detail }
}

function sortFindings(items) {
  return [...items].sort((left, right) => left.rule.localeCompare(right.rule) || left.detail.localeCompare(right.detail))
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !UNSAFE_PATH_CHARACTER.test(value) &&
    !path.isAbsolute(value) && !/^[A-Za-z]:[\\/]/.test(value) && !value.startsWith('-') &&
    !value.startsWith(':') && value.normalize('NFC') === value &&
    value.split('/').every((segment) => segment && segment !== '..' && segment !== '.')
}

function normalizedText(value) {
  return value.replace(/\\\r?\n\s*/g, ' ').replace(/\s+/g, ' ').trim()
}

function normalizedCommand(command) {
  return command.join(' ')
}

function exactKeys(value, keys) {
  if (!isObject(value)) return false
  const observed = Object.keys(value).sort()
  const expected = [...keys].sort()
  return observed.length === expected.length && observed.every((item, index) => item === expected[index])
}

function uniqueStrings(value) {
  return Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === 'string') &&
    new Set(value).size === value.length
}

function isRustProductionPath(value) {
  return safeRelative(value) && /^crates\/[^/]+\/src\/.+\.rs$/.test(value)
}

function commandFields(command) {
  if (!Array.isArray(command) || command.length < 8 || command[0] !== 'node' || command[1] !== COVERAGE_SCRIPT) return null
  const fields = { packages: null, files: null, min: null }
  for (let index = 2; index < command.length; index++) {
    const token = command[index]
    if (token === '-p' || token === '--packages') {
      if (fields.packages !== null || typeof command[index + 1] !== 'string') return null
      fields.packages = command[++index].split(',').filter(Boolean)
    } else if (token === '--files') {
      if (fields.files !== null || typeof command[index + 1] !== 'string') return null
      fields.files = command[++index].split(',').filter(Boolean)
    } else if (token === '--min') {
      if (fields.min !== null || !/^\d+$/.test(command[index + 1] ?? '')) return null
      fields.min = Number(command[++index])
    } else if (token === '--report-json') {
      if (!safeRelative(command[++index])) return null
    } else {
      return null
    }
  }
  if (!fields.packages?.length || !fields.files?.length || fields.min === null) return null
  return fields
}

function canonicalJson(value) {
  if (Array.isArray(value)) return value.map((item) => canonicalJson(item))
  if (isObject(value)) {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalJson(value[key])]))
  }
  return value
}

function sameJson(left, right) {
  return JSON.stringify(canonicalJson(left)) === JSON.stringify(canonicalJson(right))
}

function readTextInsideRoot(root, relative) {
  const buffer = readRegularFileInsideRoot(root, relative)
  if (buffer === null) return null
  try { return UTF8.decode(buffer) } catch { return null }
}

function parseSpecDeclaration(specText, marker) {
  const text = specText.replace(/\r\n/g, '\n')
  const startToken = `<!-- ${marker}\n`
  const start = text.indexOf(startToken)
  if (start < 0) return { state: 'missing', value: null }
  if (text.indexOf(startToken, start + startToken.length) >= 0) return { state: 'invalid', value: null }
  const end = text.indexOf('\n-->', start + startToken.length)
  if (end < 0) return { state: 'invalid', value: null }
  try {
    const value = JSON.parse(text.slice(start + startToken.length, end))
    return isObject(value) ? { state: 'ok', value } : { state: 'invalid', value: null }
  } catch {
    return { state: 'invalid', value: null }
  }
}

function expectedPlatformDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

function expectedLocalizationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
  }
}

function expectedTestOnlyLocalizationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

function expectedIgnoredTestRelocationDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    base_revision: entry.base_revision,
    base_source_sha256: entry.base_source_sha256,
    verification: entry.verification,
  }
}

function expectedModuleExportGlueDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    package: entry.package,
    declaration: entry.declaration,
    cargo_test: entry.cargo_test,
  }
}

function expectedStructuralDeclaration(entry) {
  return {
    schema_version: 1,
    id: entry.id,
    kind: entry.kind,
    files: entry.files,
    verifications: entry.verifications,
  }
}

function escapedRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function namedStaticTestExists(source, name) {
  const escaped = escapedRegex(name)
  return new RegExp(`(?:Assert-Static[\\s\\S]{0,4096}["']${escaped}["']|Write-Output\\s+["']PASS\\s+${escaped}(?:["'\\s])|\\b${escaped}\\s*=)`).test(source)
}

function namedLiveTestExists(source, name) {
  return new RegExp(`\\bPass\\s+["']${escapedRegex(name)}["']`).test(source)
}

function validNamedCheck(value) {
  return exactKeys(value, ['path', 'test']) && safeRelative(value.path) &&
    /^scripts\/windows\/.+\.ps1$/.test(value.path) && typeof value.test === 'string' && TEST_NAME.test(value.test)
}

function validateCommonEntry(entry, root, errors, ids) {
  if (!isObject(entry) || typeof entry.id !== 'string' || !/^[a-z0-9][a-z0-9-]*$/.test(entry.id)) {
    errors.push(finding('coverage-entry-invalid'))
    return null
  }
  if (ids.has(entry.id)) errors.push(finding('coverage-entry-duplicate', entry.id))
  ids.add(entry.id)
  if (![LINE_COVERAGE_KIND, WINDOWS_PLATFORM_KIND, LOCALIZATION_KIND, TEST_ONLY_LOCALIZATION_KIND,
    IGNORED_TEST_RELOCATION_KIND,
    MODULE_EXPORT_GLUE_KIND, STRUCTURAL_KIND].includes(entry.kind)) {
    errors.push(finding('coverage-kind-invalid', entry.id))
    return null
  }
  if (!safeRelative(entry.spec) || !entry.spec.endsWith('/SPEC.md')) {
    errors.push(finding('coverage-spec-path-invalid', entry.id))
    return null
  }
  if (!uniqueStrings(entry.files) || entry.files.some((item) => !isRustProductionPath(item))) {
    errors.push(finding('coverage-files-invalid', entry.id))
    return null
  }
  for (const file of entry.files) {
    if (readRegularFileInsideRoot(root, file) !== null) continue
    errors.push(finding(existsSync(path.join(root, file)) ? 'coverage-file-untrusted' : 'coverage-file-missing', entry.id))
  }
  const specText = readTextInsideRoot(root, entry.spec)
  if (specText === null) {
    errors.push(finding(existsSync(path.join(root, entry.spec)) ? 'coverage-spec-untrusted' : 'coverage-spec-missing', entry.id))
    return null
  }
  return specText
}

function validateLineCoverageEntry(entry, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'command', 'packages', 'files', 'min'])) {
    errors.push(finding('coverage-entry-fields-invalid', entry.id))
  }
  if (!uniqueStrings(entry.packages) || entry.packages.some((item) => !/^[a-z0-9][a-z0-9-]*$/.test(item))) {
    errors.push(finding('coverage-packages-invalid', entry.id))
  }
  if (!Number.isInteger(entry.min) || entry.min !== 80) errors.push(finding('coverage-min-invalid', entry.id))
  const fields = commandFields(entry.command)
  if (!fields) {
    errors.push(finding('coverage-command-invalid', entry.id))
    return
  }
  if (JSON.stringify(fields.packages) !== JSON.stringify(entry.packages) ||
      JSON.stringify(fields.files) !== JSON.stringify(entry.files) || fields.min !== entry.min) {
    errors.push(finding('coverage-command-fields-mismatch', entry.id))
  }
  if (!normalizedText(specText).includes(normalizedCommand(entry.command))) {
    errors.push(finding('spec-command-missing', entry.id))
  }
}

function validatePlatformEntry(entry, root, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'files', 'verifications'])) {
    errors.push(finding('platform-entry-fields-invalid', entry.id))
  }
  if (!Array.isArray(entry.verifications) || entry.verifications.length !== entry.files.length) {
    errors.push(finding('platform-verifications-invalid', entry.id))
    return
  }
  const observedSources = []
  for (const verification of entry.verifications) {
    if (!exactKeys(verification, ['source', 'static', 'live']) || !isRustProductionPath(verification.source) ||
        !validNamedCheck(verification.static) || !validNamedCheck(verification.live)) {
      errors.push(finding('platform-verification-invalid', entry.id))
      continue
    }
    observedSources.push(verification.source)
    const staticSource = readTextInsideRoot(root, verification.static.path)
    if (staticSource === null) {
      errors.push(finding('platform-static-file-missing', entry.id))
    } else if (!namedStaticTestExists(staticSource, verification.static.test)) {
      errors.push(finding('platform-static-test-missing', entry.id))
    }
    const liveSource = readTextInsideRoot(root, verification.live.path)
    if (liveSource === null) {
      errors.push(finding('platform-live-file-missing', entry.id))
    } else if (!namedLiveTestExists(liveSource, verification.live.test)) {
      errors.push(finding('platform-live-test-missing', entry.id))
    }
    const wrapper = readTextInsideRoot(root, WINDOWS_STATIC_WRAPPER)
    if (wrapper === null) {
      errors.push(finding('platform-static-wrapper-missing', entry.id))
    } else {
      const staticName = path.basename(verification.static.path)
      if (!new RegExp(`Name\\s*=\\s*["']${escapedRegex(staticName)}["']`).test(wrapper)) {
        errors.push(finding('platform-static-harness-not-run', entry.id))
      }
    }
  }
  if (JSON.stringify(observedSources) !== JSON.stringify(entry.files) || new Set(observedSources).size !== observedSources.length) {
    errors.push(finding('platform-source-files-mismatch', entry.id))
  }
  const declaration = parseSpecDeclaration(specText, PLATFORM_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('platform-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('platform-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedPlatformDeclaration(entry))) {
    errors.push(finding('platform-spec-contract-mismatch', entry.id))
  }
}

function validateLocalizationEntry(entry, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'files'])) {
    errors.push(finding('localization-entry-fields-invalid', entry.id))
  }
  const declaration = parseSpecDeclaration(specText, LOCALIZATION_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('localization-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('localization-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedLocalizationDeclaration(entry))) {
    errors.push(finding('localization-spec-contract-mismatch', entry.id))
  }
}

function validCargoTestCommand(command, packageName) {
  return Array.isArray(command) && command.length === 5 &&
    command.every((item) => typeof item === 'string') &&
    command[0] === 'cargo' && command[1] === 'test' && command[2] === '-p' &&
    command[3] === packageName && command[4] === '--lib'
}

function validIgnoredGpuCommand(command, packageName, name) {
  if (!Array.isArray(command) || command.some((item) => typeof item !== 'string') ||
      command[0] !== 'cargo' || command[1] !== 'test' || command[2] !== '-p' || command[3] !== packageName) {
    return false
  }
  let index = 4
  if (command[index] !== '--') {
    if (typeof command[index] !== 'string' || !command[index].endsWith(name)) return false
    index++
  }
  return command.length === index + 3 && command[index] === '--' &&
    command[index + 1] === '--ignored' && command[index + 2] === '--test-threads=1'
}

function exactCommand(observed, expected) {
  return Array.isArray(observed) && observed.every((item) => typeof item === 'string') &&
    JSON.stringify(observed) === JSON.stringify(expected)
}

function validIgnoredGpuTest(value, verification, relocated) {
  const keys = relocated ? ['name', 'command', 'historical_command', 'evidence'] : ['name', 'command', 'evidence']
  if (!exactKeys(value, keys) || typeof value.name !== 'string' || !TEST_NAME.test(value.name) ||
      value.evidence !== VALIDATION_EVIDENCE_PATH) return false
  if (!relocated) return validIgnoredGpuCommand(value.command, verification.package, value.name)

  const current = [
    'cargo', 'test', '-p', verification.package,
    '--test', verification.ignored_test_target, value.name,
    '--', '--ignored', '--test-threads=1',
  ]
  const sourceModule = path.posix.basename(verification.source, '.rs')
  const historical = [
    'cargo', 'test', '-p', verification.package,
    `${sourceModule}::${verification.test_module}::${value.name}`,
    '--', '--ignored', '--test-threads=1',
  ]
  return exactCommand(value.command, current) && exactCommand(value.historical_command, historical)
}

function validTestOnlyVerification(value) {
  return exactKeys(value, ['source', 'package', 'test_module', 'cargo_test', 'ignored_gpu_tests']) &&
    isRustProductionPath(value.source) && typeof value.package === 'string' &&
    /^[a-z0-9][a-z0-9-]*$/.test(value.package) && typeof value.test_module === 'string' &&
    value.source.startsWith(`crates/${value.package}/src/`) && TEST_NAME.test(value.test_module) &&
    validCargoTestCommand(value.cargo_test, value.package) &&
    Array.isArray(value.ignored_gpu_tests) && value.ignored_gpu_tests.length > 0 &&
    value.ignored_gpu_tests.every((item) => validIgnoredGpuTest(item, value, false)) &&
    new Set(value.ignored_gpu_tests.map((item) => item.name)).size === value.ignored_gpu_tests.length
}

function validRelocationImports(value) {
  return exactKeys(value, ['base', 'head']) && uniqueStrings(value.base) && uniqueStrings(value.head) &&
    value.base.every((statement) => rustUseProjection(statement) !== null) &&
    value.head.every((statement) => rustUseProjection(statement) !== null)
}

function validIgnoredTestRelocationVerification(value) {
  return exactKeys(value, [
    'source', 'package', 'test_module', 'ignored_test_source', 'ignored_test_target',
    'relocation_imports', 'ignored_gpu_tests',
  ]) && isRustProductionPath(value.source) && typeof value.package === 'string' &&
    /^[a-z0-9][a-z0-9-]*$/.test(value.package) && typeof value.test_module === 'string' &&
    value.source.startsWith(`crates/${value.package}/src/`) && TEST_NAME.test(value.test_module) &&
    typeof value.ignored_test_target === 'string' && TEST_NAME.test(value.ignored_test_target) &&
    value.ignored_test_source === `crates/${value.package}/tests/${value.ignored_test_target}.rs` &&
    validRelocationImports(value.relocation_imports) &&
    Array.isArray(value.ignored_gpu_tests) && value.ignored_gpu_tests.length > 0 &&
    value.ignored_gpu_tests.every((item) => validIgnoredGpuTest(item, value, true)) &&
    new Set(value.ignored_gpu_tests.map((item) => item.name)).size === value.ignored_gpu_tests.length
}

function readRegularFileInsideRoot(root, relative) {
  if (!safeRelative(relative)) return null
  try {
    const canonicalRoot = realpathSync(root)
    const candidate = path.resolve(canonicalRoot, relative)
    if (candidate !== canonicalRoot && !candidate.startsWith(`${canonicalRoot}${path.sep}`)) return null
    const metadata = lstatSync(candidate)
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > MAX_TRUSTED_FILE_BYTES ||
        realpathSync(candidate) !== candidate) return null
    return readFileSync(candidate)
  } catch {
    return null
  }
}

function validateTestOnlyLocalizationEntry(entry, root, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'files', 'verifications'])) {
    errors.push(finding('test-only-entry-fields-invalid', entry.id))
  }
  if (!Array.isArray(entry.verifications) || entry.verifications.length !== entry.files.length) {
    errors.push(finding('test-only-verifications-invalid', entry.id))
    return
  }
  const observedSources = []
  for (const verification of entry.verifications) {
    if (!validTestOnlyVerification(verification)) {
      errors.push(finding('test-only-verification-invalid', entry.id))
      continue
    }
    observedSources.push(verification.source)
    const source = readTextInsideRoot(root, verification.source)
    const analysis = testOnlyModuleAnalysis(source, verification.test_module)
    if (analysis === null) {
      errors.push(finding('test-only-rust-source-invalid', verification.source))
      continue
    }
    if (analysis.region === null) {
      errors.push(finding('test-only-declared-module-missing', verification.source))
      continue
    }
    for (const ignored of verification.ignored_gpu_tests) {
      if (!ignoredGpuTestExists(analysis, ignored.name)) {
        errors.push(finding('test-only-ignored-test-missing', verification.source))
      }
    }
  }
  if (JSON.stringify(observedSources) !== JSON.stringify(entry.files) || new Set(observedSources).size !== observedSources.length) {
    errors.push(finding('test-only-source-files-mismatch', entry.id))
  }
  const declaration = parseSpecDeclaration(specText, TEST_ONLY_LOCALIZATION_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('test-only-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('test-only-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedTestOnlyLocalizationDeclaration(entry))) {
    errors.push(finding('test-only-spec-contract-mismatch', entry.id))
  }
}

function validateIgnoredTestRelocationEntry(entry, root, specText, errors) {
  if (!exactKeys(entry, [
    'id', 'kind', 'spec', 'files', 'base_revision', 'base_source_sha256', 'verification',
  ])) {
    errors.push(finding('ignored-test-relocation-entry-fields-invalid', entry.id))
  }
  if (typeof entry.base_revision !== 'string' || !FULL_SHA.test(entry.base_revision) ||
      entry.base_revision !== entry.base_revision.toLowerCase() ||
      typeof entry.base_source_sha256 !== 'string' || !SHA256.test(entry.base_source_sha256) ||
      entry.base_source_sha256 !== entry.base_source_sha256.toLowerCase() ||
      !validIgnoredTestRelocationVerification(entry.verification)) {
    errors.push(finding('ignored-test-relocation-verification-invalid', entry.id))
    return
  }
  if (JSON.stringify(entry.files) !== JSON.stringify([entry.verification.source])) {
    errors.push(finding('ignored-test-relocation-source-files-mismatch', entry.id))
  }

  const source = readRegularFileInsideRoot(root, entry.verification.ignored_test_source)
  const analysis = integrationTestAnalysis(source)
  if (analysis === null) {
    errors.push(finding('ignored-test-relocation-head-source-invalid', entry.verification.ignored_test_source))
  } else {
    const projections = entry.verification.ignored_gpu_tests
      .map((ignored) => ignoredGpuTestProjection(analysis, ignored.name))
    if (projections.some((projection) => projection === null)) {
      errors.push(finding('ignored-test-relocation-head-test-missing', entry.verification.ignored_test_source))
    } else if (!integrationSourceContainsOnlyImportsAndTests(analysis, projections)) {
      errors.push(finding('ignored-test-relocation-head-source-invalid', entry.verification.ignored_test_source))
    }
    const headImports = directUseProjections(analysis)
    const declaredHead = entry.verification.relocation_imports.head.map(rustUseProjection)
    if (headImports === null || JSON.stringify(headImports) !== JSON.stringify(declaredHead)) {
      errors.push(finding('ignored-test-relocation-imports-mismatch', entry.verification.ignored_test_source))
    }
  }

  const declaration = parseSpecDeclaration(specText, IGNORED_TEST_RELOCATION_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('ignored-test-relocation-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('ignored-test-relocation-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedIgnoredTestRelocationDeclaration(entry))) {
    errors.push(finding('ignored-test-relocation-spec-contract-mismatch', entry.id))
  }
}

function validateModuleExportGlueEntry(entry, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'files', 'package', 'declaration', 'cargo_test'])) {
    errors.push(finding('module-export-glue-entry-fields-invalid', entry.id))
  }
  if (JSON.stringify(entry.files) !== JSON.stringify([MODULE_EXPORT_GLUE_PATH])) {
    errors.push(finding('module-export-glue-path-invalid', entry.id))
  }
  if (entry.package !== MODULE_EXPORT_GLUE_PACKAGE) {
    errors.push(finding('module-export-glue-package-invalid', entry.id))
  }
  if (entry.declaration !== MODULE_EXPORT_GLUE_DECLARATION) {
    errors.push(finding('module-export-glue-declaration-invalid', entry.id))
  }
  if (JSON.stringify(entry.cargo_test) !== JSON.stringify(MODULE_EXPORT_GLUE_CARGO_TEST)) {
    errors.push(finding('module-export-glue-cargo-test-invalid', entry.id))
  }
  const declaration = parseSpecDeclaration(specText, MODULE_EXPORT_GLUE_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('module-export-glue-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('module-export-glue-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedModuleExportGlueDeclaration(entry))) {
    errors.push(finding('module-export-glue-spec-contract-mismatch', entry.id))
  }
}

function validStructuralVerification(value) {
  return exactKeys(value, ['source', 'package', 'cargo_test']) &&
    isRustProductionPath(value.source) && typeof value.package === 'string' &&
    /^[a-z0-9][a-z0-9-]*$/.test(value.package) &&
    value.source.startsWith(`crates/${value.package}/src/`) &&
    validCargoTestCommand(value.cargo_test, value.package)
}

function validateStructuralEntry(entry, root, specText, errors) {
  if (!exactKeys(entry, ['id', 'kind', 'spec', 'files', 'verifications'])) {
    errors.push(finding('structural-entry-fields-invalid', entry.id))
  }
  if (!Array.isArray(entry.verifications) || entry.verifications.length !== entry.files.length) {
    errors.push(finding('structural-verifications-invalid', entry.id))
    return
  }
  const observedSources = []
  for (const verification of entry.verifications) {
    if (!validStructuralVerification(verification)) {
      errors.push(finding('structural-verification-invalid', entry.id))
      continue
    }
    observedSources.push(verification.source)
    const source = readTextInsideRoot(root, verification.source)
    if (!isStructuralRustModule(source)) {
      errors.push(finding('structural-rust-source-invalid', verification.source))
    }
  }
  if (JSON.stringify(observedSources) !== JSON.stringify(entry.files) || new Set(observedSources).size !== observedSources.length) {
    errors.push(finding('structural-source-files-mismatch', entry.id))
  }
  const declaration = parseSpecDeclaration(specText, STRUCTURAL_MARKER)
  if (declaration.state === 'missing') {
    errors.push(finding('structural-spec-contract-missing', entry.id))
  } else if (declaration.state !== 'ok') {
    errors.push(finding('structural-spec-contract-invalid', entry.id))
  } else if (!sameJson(declaration.value, expectedStructuralDeclaration(entry))) {
    errors.push(finding('structural-spec-contract-mismatch', entry.id))
  }
}

function validEntry(entry, root, errors, ids) {
  const specText = validateCommonEntry(entry, root, errors, ids)
  if (specText === null) return
  if (entry.kind === LINE_COVERAGE_KIND) validateLineCoverageEntry(entry, specText, errors)
  else if (entry.kind === WINDOWS_PLATFORM_KIND) validatePlatformEntry(entry, root, specText, errors)
  else if (entry.kind === LOCALIZATION_KIND) validateLocalizationEntry(entry, specText, errors)
  else if (entry.kind === TEST_ONLY_LOCALIZATION_KIND) validateTestOnlyLocalizationEntry(entry, root, specText, errors)
  else if (entry.kind === IGNORED_TEST_RELOCATION_KIND) validateIgnoredTestRelocationEntry(entry, root, specText, errors)
  else if (entry.kind === MODULE_EXPORT_GLUE_KIND) validateModuleExportGlueEntry(entry, specText, errors)
  else validateStructuralEntry(entry, root, specText, errors)
}

function validateIgnoredTestRelocationOwnership(entries, errors) {
  const relocations = entries.filter((entry) =>
    isObject(entry) && entry.kind === IGNORED_TEST_RELOCATION_KIND && isObject(entry.verification))
  const byLineCoverageSource = new Map()
  const byProductionSource = new Map()
  const byIntegration = new Map()
  const byIntegrationTest = new Map()

  for (const entry of entries) {
    if (!isObject(entry) || entry.kind !== LINE_COVERAGE_KIND || !Array.isArray(entry.files)) continue
    for (const source of entry.files) {
      if (!isRustProductionPath(source)) continue
      const owners = byLineCoverageSource.get(source) ?? []
      owners.push(entry)
      byLineCoverageSource.set(source, owners)
    }
  }
  for (const [source, owners] of byLineCoverageSource) {
    if (owners.length > 1) errors.push(finding('line-coverage-production-owner-duplicate', source))
  }

  for (const entry of relocations) {
    const { verification } = entry
    if (isRustProductionPath(verification.source)) {
      const owners = byProductionSource.get(verification.source) ?? []
      owners.push(entry)
      byProductionSource.set(verification.source, owners)
    }
    if (typeof verification.ignored_test_source !== 'string' ||
        typeof verification.ignored_test_target !== 'string') continue
    const integrationKey = JSON.stringify([
      verification.ignored_test_source,
      verification.ignored_test_target,
    ])
    const integrationOwners = byIntegration.get(integrationKey) ?? []
    integrationOwners.push(entry)
    byIntegration.set(integrationKey, integrationOwners)
    if (!Array.isArray(verification.ignored_gpu_tests)) continue
    for (const ignored of verification.ignored_gpu_tests) {
      if (!isObject(ignored) || typeof ignored.name !== 'string') continue
      const testKey = JSON.stringify([
        verification.ignored_test_source,
        verification.ignored_test_target,
        ignored.name,
      ])
      const testOwners = byIntegrationTest.get(testKey) ?? []
      testOwners.push(entry)
      byIntegrationTest.set(testKey, testOwners)
    }
  }

  for (const [source, relocationOwners] of byProductionSource) {
    if (relocationOwners.length > 1) {
      errors.push(finding('ignored-test-relocation-production-owner-duplicate', source))
    }
    const allOwners = entries.filter((entry) => Array.isArray(entry?.files) && entry.files.includes(source))
    const lineOwners = allOwners.filter((entry) => entry.kind === LINE_COVERAGE_KIND)
    const proofOwners = allOwners.filter((entry) => entry.kind === IGNORED_TEST_RELOCATION_KIND)
    if (lineOwners.length > 1 || proofOwners.length > 1 ||
        allOwners.some((entry) => ![LINE_COVERAGE_KIND, IGNORED_TEST_RELOCATION_KIND].includes(entry.kind))) {
      errors.push(finding('ignored-test-relocation-production-owner-conflict', source))
    }
  }
  for (const [key, owners] of byIntegration) {
    if (owners.length > 1) {
      const [source, target] = JSON.parse(key)
      errors.push(finding('ignored-test-relocation-integration-owner-duplicate', `${source}#${target}`))
    }
  }
  for (const [key, owners] of byIntegrationTest) {
    if (owners.length > 1) {
      const [source, target, name] = JSON.parse(key)
      errors.push(finding('ignored-test-relocation-integration-test-owner-duplicate', `${source}#${target}#${name}`))
    }
  }
}

export function validateCoverageMap(map, root = ROOT) {
  const errors = []
  if (!isObject(map) || map.schema_version !== MAP_SCHEMA_VERSION || !Array.isArray(map.entries) || map.entries.length === 0) {
    return { ok: false, errors: [finding('coverage-map-invalid')] }
  }
  const ids = new Set()
  for (const entry of map.entries) validEntry(entry, root, errors, ids)
  validateIgnoredTestRelocationOwnership(map.entries, errors)
  return { ok: errors.length === 0, errors: sortFindings(errors) }
}

function isBusinessRustPath(file) {
  return isRustProductionPath(file)
}

function decodeUtf8(value) {
  try {
    if (typeof value === 'string') return value
    return new TextDecoder('utf-8', { fatal: true }).decode(value)
  } catch {
    return null
  }
}

function rawStringEnd(source, index) {
  let cursor
  if (source.startsWith('br', index) || source.startsWith('cr', index)) cursor = index + 2
  else if (source[index] === 'r') cursor = index + 1
  else return null
  if (index > 0 && /[A-Za-z0-9_]/.test(source[index - 1])) return null
  let hashes = 0
  while (source[cursor] === '#') {
    hashes++
    cursor++
  }
  if (source[cursor] !== '"') return null
  const closing = `"${'#'.repeat(hashes)}`
  const end = source.indexOf(closing, cursor + 1)
  return end < 0 ? -1 : end + closing.length
}

function quotedStringEnd(source, index) {
  for (let cursor = index + 1; cursor < source.length; cursor++) {
    if (source[cursor] === '\\') {
      cursor++
      continue
    }
    if (source[cursor] === '"') return cursor + 1
    if (source[cursor] === '\r' || source[cursor] === '\n') return -1
  }
  return -1
}

function blockCommentEnd(source, index) {
  let depth = 1
  let preserved = ''
  for (let cursor = index + 2; cursor < source.length; cursor++) {
    if (source[cursor] === '/' && source[cursor + 1] === '*') {
      depth++
      cursor++
      continue
    }
    if (source[cursor] === '*' && source[cursor + 1] === '/') {
      depth--
      if (depth === 0) return { end: cursor + 2, preserved }
      cursor++
      continue
    }
    if (source[cursor] === '\r' || source[cursor] === '\n') preserved += source[cursor]
  }
  return null
}

function isRustIdentifierStart(value) {
  return typeof value === 'string' && /^[A-Za-z_]$/.test(value)
}

function isRustIdentifierContinue(value) {
  return typeof value === 'string' && /^[A-Za-z0-9_]$/.test(value)
}

function rustCharacterLiteralEnd(source, index) {
  const firstIndex = index + 1
  if (firstIndex >= source.length || source[firstIndex] === '\r' || source[firstIndex] === '\n') return -1
  let cursor
  if (source[firstIndex] === '\\') {
    cursor = firstIndex + 1
    const escape = source[cursor]
    if (escape === undefined) return -1
    if (escape === 'x') {
      if (!/^[0-9A-Fa-f]{2}$/.test(source.slice(cursor + 1, cursor + 3))) return -1
      cursor += 3
    } else if (escape === 'u') {
      if (source[cursor + 1] !== '{') return -1
      const end = source.indexOf('}', cursor + 2)
      if (end < 0 || !/^[0-9A-Fa-f]{1,6}$/.test(source.slice(cursor + 2, end))) return -1
      cursor = end + 1
    } else if (['n', 'r', 't', '\\', '0', "'", '"'].includes(escape)) {
      cursor++
    } else {
      return -1
    }
  } else {
    if (source[firstIndex] === "'") return -1
    const codePoint = source.codePointAt(firstIndex)
    if (codePoint === undefined) return -1
    cursor = firstIndex + (codePoint > 0xffff ? 2 : 1)
  }
  if (source[cursor] === "'") return cursor + 1
  // A leading apostrophe followed by an identifier can be a valid lifetime.
  // It is emitted as punctuation by the caller, never as a test-module token.
  return isRustIdentifierStart(source[firstIndex]) ? null : -1
}

function lexRustForTestOnly(source) {
  const text = decodeUtf8(source)
  if (text === null) return null
  const tokens = []
  const pairs = new Map()
  const stack = []

  const emit = (value, start, end) => tokens.push({ value, start, end, depth: stack.length })
  for (let index = 0; index < text.length;) {
    if (/\s/.test(text[index])) {
      index++
      continue
    }
    if (text[index] === '/' && text[index + 1] === '/') {
      index += 2
      while (index < text.length && text[index] !== '\r' && text[index] !== '\n') index++
      continue
    }
    if (text[index] === '/' && text[index + 1] === '*') {
      const block = blockCommentEnd(text, index)
      if (block === null) return null
      index = block.end
      continue
    }
    const rawEnd = rawStringEnd(text, index)
    if (rawEnd === -1) return null
    if (rawEnd !== null) {
      emit('<literal>', index, rawEnd)
      index = rawEnd
      continue
    }
    if (text[index] === '"' || (text[index] === 'b' && text[index + 1] === '"')) {
      const quote = text[index] === '"' ? index : index + 1
      const end = quotedStringEnd(text, quote)
      if (end < 0) return null
      emit('<literal>', index, end)
      index = end
      continue
    }
    if (text[index] === "'" || (text[index] === 'b' && text[index + 1] === "'")) {
      const quote = text[index] === "'" ? index : index + 1
      const end = rustCharacterLiteralEnd(text, quote)
      if (end === -1) return null
      if (end !== null) {
        emit('<literal>', index, end)
        index = end
        continue
      }
    }
    if (isRustIdentifierStart(text[index])) {
      const start = index
      index++
      while (isRustIdentifierContinue(text[index])) index++
      emit(text.slice(start, index), start, index)
      continue
    }
    if (/[0-9]/.test(text[index])) {
      const start = index
      index++
      while (/[A-Za-z0-9_.]/.test(text[index] ?? '')) index++
      emit('<number>', start, index)
      continue
    }
    const value = text[index]
    const tokenIndex = tokens.length
    emit(value, index, index + 1)
    if (['(', '[', '{'].includes(value)) {
      stack.push({ value, tokenIndex })
    } else if ([')', ']', '}'].includes(value)) {
      const expected = value === ')' ? '(' : value === ']' ? '[' : '{'
      const opening = stack.pop()
      if (!opening || opening.value !== expected) return null
      pairs.set(opening.tokenIndex, tokenIndex)
      pairs.set(tokenIndex, opening.tokenIndex)
    }
    index++
  }
  if (stack.length !== 0) return null
  return { text, tokens, pairs }
}

export function isStructuralRustModule(source) {
  const lexed = lexRustForTestOnly(source)
  if (lexed === null) return false
  const { tokens, pairs } = lexed
  let index = 0
  let declarations = 0

  while (index < tokens.length) {
    while (tokens[index]?.value === '#') {
      let bracket = index + 1
      if (tokens[bracket]?.value === '!') bracket++
      if (tokens[bracket]?.value !== '[' || tokens[bracket].depth !== 0) return false
      const end = pairs.get(bracket)
      if (end === undefined) return false
      index = end + 1
    }
    if (index >= tokens.length) break

    if (tokens[index]?.value === 'pub') {
      index++
      if (tokens[index]?.value === '(') {
        const end = pairs.get(index)
        if (end === undefined) return false
        index = end + 1
      }
    }

    if (tokens[index]?.value === 'mod') {
      if (!isRustIdentifierStart(tokens[index + 1]?.value?.[0]) || tokens[index + 2]?.value !== ';') return false
      index += 3
      declarations++
      continue
    }

    if (tokens[index]?.value === 'use') {
      const statementDepth = tokens[index].depth
      index++
      let sawPath = false
      while (index < tokens.length && !(tokens[index].value === ';' && tokens[index].depth === statementDepth)) {
        const value = tokens[index].value
        if (value === '<literal>' || value === '<number>' || ['(', ')', '[', ']', '=', '!', '#'].includes(value)) return false
        if (!isRustIdentifierStart(value?.[0]) && ![':', '{', '}', ',', '*'].includes(value)) return false
        sawPath = true
        index++
      }
      if (!sawPath || tokens[index]?.value !== ';') return false
      index++
      declarations++
      continue
    }
    return false
  }
  return declarations > 0
}

function testOnlyModuleAnalysis(source, testModule) {
  const lexed = lexRustForTestOnly(source)
  if (lexed === null) return null
  const values = ['#', '[', 'cfg', '(', 'test', ')', ']', 'mod', testModule, '{']
  const candidates = []
  for (let index = 0; index <= lexed.tokens.length - values.length; index++) {
    if (lexed.tokens[index].depth !== 0) continue
    if (!values.every((value, offset) => lexed.tokens[index + offset].value === value)) continue
    const openIndex = index + values.length - 1
    const closeIndex = lexed.pairs.get(openIndex)
    if (closeIndex === undefined) return null
    candidates.push({ index, openIndex, closeIndex })
  }
  if (candidates.length !== 1) return { ...lexed, region: null }
  const candidate = candidates[0]
  return {
    ...lexed,
    region: {
      start: lexed.tokens[candidate.index].start,
      end: lexed.tokens[candidate.closeIndex].end,
      openIndex: candidate.openIndex,
      closeIndex: candidate.closeIndex,
    },
  }
}

function exactRustToken(analysis, token) {
  return token.value === '<literal>' || token.value === '<number>'
    ? analysis.text.slice(token.start, token.end)
    : token.value
}

function integrationTestAnalysis(source) {
  const analysis = lexRustForTestOnly(source)
  if (analysis === null) return null
  return {
    ...analysis,
    region: { openIndex: -1, closeIndex: analysis.tokens.length },
  }
}

function rustUseProjection(statement) {
  const analysis = lexRustForTestOnly(statement)
  if (analysis === null || analysis.tokens[0]?.value !== 'use' ||
      analysis.tokens.at(-1)?.value !== ';' || analysis.tokens.at(-1)?.depth !== 0) return null
  const firstSemicolon = analysis.tokens.findIndex((token) => token.value === ';' && token.depth === 0)
  if (firstSemicolon !== analysis.tokens.length - 1) return null
  return analysis.tokens.map((token) => exactRustToken(analysis, token))
}

function directUseProjections(analysis) {
  if (analysis === null || analysis?.region === null) return null
  const expectedDepth = analysis.region.openIndex < 0
    ? 0
    : analysis.tokens[analysis.region.openIndex].depth + 1
  const projections = []
  for (let index = analysis.region.openIndex + 1; index < analysis.region.closeIndex;) {
    if (analysis.tokens[index]?.value !== 'use' || analysis.tokens[index].depth !== expectedDepth) {
      index++
      continue
    }
    const start = index
    while (index < analysis.region.closeIndex &&
           !(analysis.tokens[index].value === ';' && analysis.tokens[index].depth === expectedDepth)) index++
    if (index >= analysis.region.closeIndex) return null
    projections.push(analysis.tokens.slice(start, index + 1)
      .map((token) => exactRustToken(analysis, token)))
    index++
  }
  return projections
}

function ignoredGpuTestProjection(analysis, name) {
  if (analysis === null || analysis?.region === null) return null
  const { tokens, pairs, region } = analysis
  const expectedDepth = region.openIndex < 0 ? 0 : tokens[region.openIndex].depth + 1
  const matches = []

  for (let functionIndex = region.openIndex + 1; functionIndex < region.closeIndex - 2; functionIndex++) {
    if (tokens[functionIndex].depth !== expectedDepth || tokens[functionIndex].value !== 'fn' ||
        tokens[functionIndex + 1]?.value !== name) continue

    const attributes = []
    let before = functionIndex - 1
    while (tokens[before]?.value === ']') {
      const opening = pairs.get(before)
      if (opening === undefined || tokens[opening - 1]?.value !== '#' || tokens[opening - 1]?.depth !== expectedDepth) break
      attributes.unshift({ start: opening - 1, open: opening, end: before })
      before = opening - 2
    }
    const hasTest = attributes.some(({ open, end }) =>
      end === open + 2 && tokens[open + 1]?.value === 'test')
    const hasIgnore = attributes.some(({ open, end }) =>
      end === open + 4 && tokens[open + 1]?.value === 'ignore' &&
      tokens[open + 2]?.value === '=' && tokens[open + 3]?.value === '<literal>')
    if (!hasTest || !hasIgnore) continue

    let bodyOpen = functionIndex + 2
    while (bodyOpen < region.closeIndex &&
           !(tokens[bodyOpen].value === '{' && tokens[bodyOpen].depth === expectedDepth)) bodyOpen++
    const bodyClose = pairs.get(bodyOpen)
    if (bodyOpen >= region.closeIndex || bodyClose === undefined || bodyClose >= region.closeIndex) continue

    const importSpans = []
    let cursor = bodyOpen + 1
    while (tokens[cursor]?.value === 'use' && tokens[cursor].depth === expectedDepth + 1) {
      const importStart = cursor
      while (cursor < bodyClose && !(tokens[cursor].value === ';' && tokens[cursor].depth === expectedDepth + 1)) cursor++
      if (cursor >= bodyClose) return null
      importSpans.push({ start: importStart, end: cursor })
      cursor++
    }

    const startIndex = attributes[0].start
    const ignoredIndexes = new Set(importSpans.flatMap(({ start, end }) =>
      Array.from({ length: end - start + 1 }, (_, offset) => start + offset)))
    matches.push({
      startIndex,
      endIndex: bodyClose,
      imports: importSpans.map(({ start, end }) =>
        tokens.slice(start, end + 1).map((token) => exactRustToken(analysis, token))),
      tokens: tokens.slice(startIndex, bodyClose + 1)
        .filter((_token, offset) => !ignoredIndexes.has(startIndex + offset))
        .map((token) => exactRustToken(analysis, token)),
    })
  }
  return matches.length === 1 ? matches[0] : null
}

function ignoredGpuTestExists(analysis, name) {
  return ignoredGpuTestProjection(analysis, name) !== null
}

function integrationSourceContainsOnlyImportsAndTests(analysis, projections) {
  const consumed = new Set(projections.flatMap(({ startIndex, endIndex }) =>
    Array.from({ length: endIndex - startIndex + 1 }, (_, offset) => startIndex + offset)))
  const { tokens, pairs } = analysis
  let allowedLintAttribute = false
  for (let index = 0; index < tokens.length;) {
    if (consumed.has(index)) {
      index++
      continue
    }
    if (tokens[index].depth !== 0) return false
    if (tokens[index].value === '#' && tokens[index + 1]?.value === '!' && tokens[index + 2]?.value === '[') {
      const end = pairs.get(index + 2)
      if (end === undefined) return false
      const values = tokens.slice(index, end + 1).map((token) => token.value)
      const expected = [
        '#', '!', '[', 'allow', '(', 'clippy', ':', ':', 'expect_used', ',',
        'clippy', ':', ':', 'unwrap_used', ')', ']',
      ]
      if (allowedLintAttribute || JSON.stringify(values) !== JSON.stringify(expected)) return false
      allowedLintAttribute = true
      index = end + 1
      continue
    }
    if (tokens[index].value === 'use') {
      while (index < tokens.length && !(tokens[index].value === ';' && tokens[index].depth === 0)) index++
      if (index >= tokens.length) return false
      index++
      continue
    }
    return false
  }
  return true
}

function normalizedRelocationImports(imports, packageName, fromProductionModule) {
  const crateName = packageName.replaceAll('-', '_')
  return imports.map((statement) => statement.map((token, index) =>
    fromProductionModule && index === 1 && token === 'crate' ? crateName : token))
}

function exactIgnoredTestRelocation(base, head, packageName) {
  return base !== null && head !== null &&
    JSON.stringify(base.tokens) === JSON.stringify(head.tokens) &&
    JSON.stringify(normalizedRelocationImports(base.imports, packageName, true)) ===
      JSON.stringify(normalizedRelocationImports(head.imports, packageName, false))
}

function projectTestOnlyModule(source, testModule) {
  const analysis = testOnlyModuleAnalysis(source, testModule)
  if (analysis === null || analysis.region === null) return { analysis, projection: null }
  return {
    analysis,
    projection: `${analysis.text.slice(0, analysis.region.start)}${analysis.text.slice(analysis.region.end)}`,
    header: analysis.text.slice(analysis.region.start, analysis.tokens[analysis.region.openIndex].end),
  }
}

function moduleExportGlueSpans(source, declaration = MODULE_EXPORT_GLUE_DECLARATION) {
  const lexed = lexRustForTestOnly(source)
  if (lexed === null) return null
  const declarations = declaration.split('\n')
  if (declarations.length === 0 || new Set(declarations).size !== declarations.length ||
    declarations.some((item) => !/^pub mod [a-z][a-z0-9_]*;$/.test(item))) return null

  const spans = []
  for (const expected of declarations) {
    const values = ['pub', 'mod', expected.slice('pub mod '.length, -1), ';']
    const matches = []
    for (let index = 0; index <= lexed.tokens.length - values.length; index++) {
      if (lexed.tokens[index].depth !== 0) continue
      if (!values.every((value, offset) => lexed.tokens[index + offset].value === value)) continue
      const start = lexed.tokens[index].start
      const end = lexed.tokens[index + values.length - 1].end
      const lineStart = start === 0 ? 0 : lexed.text.lastIndexOf('\n', start - 1) + 1
      if (lexed.text.slice(lineStart, end + 1) !== `${expected}\n`) continue
      matches.push({ start: lineStart, end: end + 1 })
    }
    if (matches.length === 0) continue
    if (matches.length !== 1) return null
    spans.push(matches[0])
  }
  if (spans.length === 0) return []
  return spans.length === declarations.length ? spans.sort((left, right) => left.start - right.start) : null
}

function projectModuleExportGlue(source, declaration = MODULE_EXPORT_GLUE_DECLARATION) {
  if (!isStructuralRustModule(source)) return null
  const spans = moduleExportGlueSpans(source, declaration)
  const declarationCount = declaration.split('\n').length
  if (spans === null || spans.length !== declarationCount) return null
  let projection = source
  for (const span of [...spans].sort((left, right) => right.start - left.start)) {
    projection = `${projection.slice(0, span.start)}${projection.slice(span.end)}`
  }
  return projection
}

export function isExactModuleExportGlueDifferential(
  baseSource,
  headSource,
  declaration = MODULE_EXPORT_GLUE_DECLARATION,
) {
  const base = decodeUtf8(baseSource)
  const head = decodeUtf8(headSource)
  if (base === null || head === null || !isStructuralRustModule(base)) return false
  if (base === head) return projectModuleExportGlue(head, declaration) !== null
  if (moduleExportGlueSpans(base, declaration)?.length !== 0) return false
  const projection = projectModuleExportGlue(head, declaration)
  return projection !== null && projection === base
}

export function stripRustComments(source) {
  const text = decodeUtf8(source)
  if (text === null) return null
  let output = ''
  for (let index = 0; index < text.length;) {
    const rawEnd = rawStringEnd(text, index)
    if (rawEnd === -1) return null
    if (rawEnd !== null) {
      output += text.slice(index, rawEnd)
      index = rawEnd
      continue
    }
    if (text[index] === '"') {
      const end = quotedStringEnd(text, index)
      if (end < 0) return null
      output += text.slice(index, end)
      index = end
      continue
    }
    if (text[index] === '/' && text[index + 1] === '/') {
      index += 2
      while (index < text.length && text[index] !== '\r' && text[index] !== '\n') index++
      continue
    }
    if (text[index] === '/' && text[index + 1] === '*') {
      const block = blockCommentEnd(text, index)
      if (block === null) return null
      output += block.preserved
      index = block.end
      continue
    }
    output += text[index]
    index++
  }
  return output
}

export function isCommentOnlyRustDifferential(baseSource, headSource) {
  const base = stripRustComments(baseSource)
  const head = stripRustComments(headSource)
  return base !== null && head !== null && base === head
}

function readGitBaseFile(root, baseRevision, file) {
  let result = spawnSync('git', ['show', `${baseRevision}:${file}`], {
    cwd: root,
    shell: false,
    encoding: null,
    maxBuffer: 2 * 1024 * 1024,
  })
  if ((result?.status !== 0 || !result.stdout) && typeof baseRevision === 'string' && FULL_SHA.test(baseRevision)) {
    spawnSync('git', ['fetch', '--depth=1', 'origin', baseRevision], {
      cwd: root,
      shell: false,
      stdio: 'ignore',
    })
    result = spawnSync('git', ['show', `${baseRevision}:${file}`], {
      cwd: root,
      shell: false,
      encoding: null,
      maxBuffer: 2 * 1024 * 1024,
    })
  }
  if (result?.status !== 0 || result.error || !result.stdout) return null
  return result.stdout
}

function validateLocalizationDifferential(entry, root, options, errors) {
  if (options.baseRevision === null || options.baseRevision === undefined) {
    errors.push(finding('localization-differential-base-required', entry.id))
    return
  }
  if (typeof options.baseRevision !== 'string' || !FULL_SHA.test(options.baseRevision)) {
    errors.push(finding('localization-differential-base-invalid', entry.id))
    return
  }
  const readBaseFile = typeof options.readBaseFile === 'function'
    ? options.readBaseFile
    : (baseRevision, file) => readGitBaseFile(root, baseRevision, file)
  for (const file of entry.files) {
    let baseSource
    try {
      baseSource = readBaseFile(options.baseRevision, file)
    } catch {
      baseSource = null
    }
    const headSource = readRegularFileInsideRoot(root, file)
    if (baseSource === null || baseSource === undefined) {
      errors.push(finding('localization-differential-base-read-failed', file))
    } else if (headSource === null) {
      errors.push(finding('localization-differential-head-read-failed', file))
    } else if (!isCommentOnlyRustDifferential(baseSource, headSource)) {
      errors.push(finding('localization-differential-not-proven', file))
    }
  }
}

function baseFileReader(root, options) {
  return typeof options.readBaseFile === 'function'
    ? options.readBaseFile
    : (baseRevision, file) => readGitBaseFile(root, baseRevision, file)
}

function readBaseFileSafely(readBaseFile, baseRevision, file) {
  try {
    return readBaseFile(baseRevision, file)
  } catch {
    return null
  }
}

function ignoredGpuEvidenceExists(source, command) {
  const text = decodeUtf8(source)
  if (text === null) return false
  const exact = escapedRegex(normalizedCommand(command))
  return new RegExp('`' + exact + '`:\\s*\\*\\*PASS\\*\\*(?:\\.|\\s|$)').test(text)
}

function validateTestOnlyLocalizationDifferential(entry, root, options, errors) {
  if (options.baseRevision === null || options.baseRevision === undefined) {
    errors.push(finding('test-only-differential-base-required', entry.id))
    return
  }
  if (typeof options.baseRevision !== 'string' || !FULL_SHA.test(options.baseRevision)) {
    errors.push(finding('test-only-differential-base-invalid', entry.id))
    return
  }
  const readBaseFile = baseFileReader(root, options)
  let baseEvidence
  let evidenceRead = false
  for (const verification of entry.verifications) {
    const baseSource = readBaseFileSafely(readBaseFile, options.baseRevision, verification.source)
    const headSource = readRegularFileInsideRoot(root, verification.source)
    if (baseSource === null || baseSource === undefined) {
      errors.push(finding('test-only-differential-base-read-failed', verification.source))
      continue
    }
    if (headSource === null) {
      errors.push(finding('test-only-differential-head-read-failed', verification.source))
      continue
    }
    const base = projectTestOnlyModule(baseSource, verification.test_module)
    const head = projectTestOnlyModule(headSource, verification.test_module)
    if (base.analysis === null || head.analysis === null) {
      errors.push(finding('test-only-rust-source-invalid', verification.source))
      continue
    }
    if (base.analysis.region === null || head.analysis.region === null) {
      errors.push(finding('test-only-declared-module-missing', verification.source))
      continue
    }
    if (base.header !== head.header || !isCommentOnlyRustDifferential(base.projection, head.projection)) {
      errors.push(finding('test-only-production-projection-not-proven', verification.source))
    }
    for (const ignored of verification.ignored_gpu_tests) {
      if (!ignoredGpuTestExists(base.analysis, ignored.name)) {
        errors.push(finding('test-only-ignored-test-base-missing', verification.source))
      }
      if (!ignoredGpuTestExists(head.analysis, ignored.name)) {
        errors.push(finding('test-only-ignored-test-head-missing', verification.source))
      }
      if (!evidenceRead) {
        baseEvidence = readBaseFileSafely(readBaseFile, options.baseRevision, VALIDATION_EVIDENCE_PATH)
        evidenceRead = true
      }
      if (baseEvidence === null || baseEvidence === undefined) {
        errors.push(finding('test-only-ignored-evidence-read-failed', entry.id))
      } else if (!ignoredGpuEvidenceExists(baseEvidence, ignored.command)) {
        errors.push(finding('test-only-ignored-evidence-missing', ignored.name))
      }
    }
  }
}

function sha256(value) {
  try {
    return createHash('sha256').update(value).digest('hex')
  } catch {
    return null
  }
}

function validateIgnoredTestRelocationDifferential(entry, root, options, errors) {
  const readBaseFile = baseFileReader(root, options)
  const verification = entry.verification
  const baseSource = readBaseFileSafely(readBaseFile, entry.base_revision, verification.source)
  if (baseSource === null || baseSource === undefined) {
    errors.push(finding('ignored-test-relocation-base-read-failed', verification.source))
    return
  }
  if (sha256(baseSource) !== entry.base_source_sha256) {
    errors.push(finding('ignored-test-relocation-base-sha-mismatch', verification.source))
    return
  }
  const baseAnalysis = testOnlyModuleAnalysis(baseSource, verification.test_module)
  if (baseAnalysis === null) {
    errors.push(finding('ignored-test-relocation-base-source-invalid', verification.source))
    return
  }
  if (baseAnalysis.region === null) {
    errors.push(finding('ignored-test-relocation-base-module-missing', verification.source))
    return
  }
  const declaredBase = verification.relocation_imports.base.map(rustUseProjection)
  const baseImports = directUseProjections(baseAnalysis)
  if (baseImports === null || JSON.stringify(baseImports) !== JSON.stringify(declaredBase)) {
    errors.push(finding('ignored-test-relocation-imports-mismatch', verification.source))
  }

  const headSource = readRegularFileInsideRoot(root, verification.ignored_test_source)
  const headAnalysis = integrationTestAnalysis(headSource)
  if (headAnalysis === null) {
    errors.push(finding('ignored-test-relocation-head-source-invalid', verification.ignored_test_source))
    return
  }
  for (const ignored of verification.ignored_gpu_tests) {
    const base = ignoredGpuTestProjection(baseAnalysis, ignored.name)
    const head = ignoredGpuTestProjection(headAnalysis, ignored.name)
    if (base === null) {
      errors.push(finding('ignored-test-relocation-base-test-missing', ignored.name))
    }
    if (head === null) {
      errors.push(finding('ignored-test-relocation-head-test-missing', ignored.name))
    } else if (base !== null && !exactIgnoredTestRelocation(base, head, verification.package)) {
      errors.push(finding('ignored-test-relocation-not-proven', ignored.name))
    }
    const baseEvidence = readBaseFileSafely(
      readBaseFile,
      entry.base_revision,
      ignored.evidence,
    )
    if (baseEvidence === null || baseEvidence === undefined) {
      errors.push(finding('ignored-test-relocation-evidence-read-failed', ignored.name))
    } else if (!ignoredGpuEvidenceExists(baseEvidence, ignored.historical_command)) {
      errors.push(finding('ignored-test-relocation-evidence-missing', ignored.name))
    }
  }
}

function validateModuleExportGlueDifferential(entry, root, options, errors) {
  if (options.baseRevision === null || options.baseRevision === undefined) {
    errors.push(finding('module-export-glue-differential-base-required', entry.id))
    return
  }
  if (typeof options.baseRevision !== 'string' || !FULL_SHA.test(options.baseRevision)) {
    errors.push(finding('module-export-glue-differential-base-invalid', entry.id))
    return
  }
  const readBaseFile = baseFileReader(root, options)
  for (const file of entry.files) {
    const baseSource = readBaseFileSafely(readBaseFile, options.baseRevision, file)
    const headSource = readRegularFileInsideRoot(root, file)
    if (baseSource === null || baseSource === undefined) {
      errors.push(finding('module-export-glue-differential-base-read-failed', file))
    } else if (headSource === null) {
      errors.push(finding('module-export-glue-differential-head-read-failed', file))
    } else if (!isExactModuleExportGlueDifferential(baseSource, headSource, entry.declaration)) {
      errors.push(finding('module-export-glue-differential-not-proven', file))
    }
  }
}

function selectStaticAllEntries(map, root) {
  const checked = validateCoverageMap(map, root)
  if (!checked.ok) return { ok: false, state: 'BLOCKED', entries: [], errors: checked.errors }
  const localization = map.entries.find((entry) => entry.kind === LOCALIZATION_KIND)
  if (localization) {
    return {
      ok: false,
      state: 'BLOCKED',
      entries: [],
      errors: [finding('localization-differential-base-required', localization.id)],
    }
  }
  const entries = [...map.entries].sort((left, right) => left.id.localeCompare(right.id))
  return { ok: true, state: entries.length === 0 ? 'NO_CHANGE' : 'READY', entries, errors: [] }
}

export function selectCoverageEntries(map, changedPaths, root = ROOT, options = {}) {
  const checked = validateCoverageMap(map, root)
  if (!checked.ok) return { ok: false, state: 'BLOCKED', entries: [], errors: checked.errors }
  if (!Array.isArray(changedPaths)) return { ok: false, state: 'BLOCKED', entries: [], errors: [finding('changed-paths-invalid')] }
  const errors = []
  const businessFiles = []
  const integrationFiles = []
  for (const file of changedPaths) {
    if (!safeRelative(file)) {
      errors.push(finding('changed-path-unsafe'))
      continue
    }
    if (isBusinessRustPath(file)) businessFiles.push(file)
    else if (map.entries.some((entry) =>
      entry.kind === IGNORED_TEST_RELOCATION_KIND && entry.verification.ignored_test_source === file)) {
      integrationFiles.push(file)
    }
  }
  const selected = new Map()
  const validatedSpecialOwners = new Set()
  const validateSpecialOwner = (owner) => {
    if (validatedSpecialOwners.has(owner.id)) return
    if (owner.kind === LOCALIZATION_KIND) validateLocalizationDifferential(owner, root, options, errors)
    if (owner.kind === TEST_ONLY_LOCALIZATION_KIND) validateTestOnlyLocalizationDifferential(owner, root, options, errors)
    if (owner.kind === IGNORED_TEST_RELOCATION_KIND) {
      validateIgnoredTestRelocationDifferential(owner, root, options, errors)
    }
    if (owner.kind === MODULE_EXPORT_GLUE_KIND) validateModuleExportGlueDifferential(owner, root, options, errors)
    validatedSpecialOwners.add(owner.id)
  }
  for (const file of businessFiles) {
    const owners = map.entries.filter((entry) => entry.files.includes(file))
    if (owners.length === 0) {
      errors.push(finding('changed-rust-file-unmapped', file))
      continue
    }
    const specialOwners = owners.filter((entry) => entry.kind !== LINE_COVERAGE_KIND)
    const exactRelocationAndCoverage = owners.length === 2 && specialOwners.length === 1 &&
      specialOwners[0].kind === IGNORED_TEST_RELOCATION_KIND &&
      owners.filter((entry) => entry.kind === LINE_COVERAGE_KIND).length === 1
    if (specialOwners.length > 0 && owners.length !== 1 && !exactRelocationAndCoverage) {
      errors.push(finding('changed-rust-file-ambiguous-ownership', file))
      continue
    }
    for (const owner of owners) {
      if (owner.kind !== LINE_COVERAGE_KIND) validateSpecialOwner(owner)
      selected.set(owner.id, owner)
    }
  }
  for (const file of integrationFiles) {
    const owners = map.entries.filter((entry) =>
      entry.kind === IGNORED_TEST_RELOCATION_KIND && entry.verification.ignored_test_source === file)
    if (owners.length !== 1) {
      errors.push(finding('changed-rust-file-ambiguous-ownership', file))
      continue
    }
    validateSpecialOwner(owners[0])
    selected.set(owners[0].id, owners[0])
  }
  if (errors.length > 0) return { ok: false, state: 'BLOCKED', entries: [], errors: sortFindings(errors) }
  const entries = [...selected.values()].sort((left, right) => left.id.localeCompare(right.id))
  return { ok: true, state: entries.length === 0 ? 'NO_CHANGE' : 'READY', entries, errors: [] }
}

export function runCoveragePlan(entries, { root = ROOT, spawn = spawnSync } = {}) {
  const failures = []
  const structuralCommands = new Set()
  for (const entry of entries) {
    if (entry.kind === LINE_COVERAGE_KIND) {
      const result = spawn(entry.command[0], entry.command.slice(1), {
        cwd: root,
        shell: false,
        stdio: 'inherit',
      })
      if (result?.status !== 0) failures.push(finding('coverage-command-failed', entry.id))
    } else if (entry.kind === TEST_ONLY_LOCALIZATION_KIND) {
      for (const verification of entry.verifications) {
        const result = spawn(verification.cargo_test[0], verification.cargo_test.slice(1), {
          cwd: root,
          shell: false,
          stdio: 'inherit',
        })
        if (result?.status !== 0) failures.push(finding('test-only-package-test-failed', verification.source))
      }
    } else if (entry.kind === MODULE_EXPORT_GLUE_KIND) {
      const result = spawn(entry.cargo_test[0], entry.cargo_test.slice(1), {
        cwd: root,
        shell: false,
        stdio: 'inherit',
      })
      if (result?.status !== 0) failures.push(finding('module-export-glue-package-test-failed', entry.id))
    } else if (entry.kind === STRUCTURAL_KIND) {
      for (const verification of entry.verifications) {
        const key = JSON.stringify(verification.cargo_test)
        if (structuralCommands.has(key)) continue
        structuralCommands.add(key)
        const result = spawn(verification.cargo_test[0], verification.cargo_test.slice(1), {
          cwd: root,
          shell: false,
          stdio: 'inherit',
        })
        if (result?.status !== 0) failures.push(finding('structural-package-test-failed', verification.source))
      }
    }
  }
  return { ok: failures.length === 0, errors: failures }
}

function parseArgs(argv) {
  const options = { map: DEFAULT_MAP, changedFiles: null, all: false, run: false, baseRevision: null }
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]
    const value = () => {
      const next = argv[++index]
      if (next === undefined) throw new Error('coverage-argument-value-missing')
      return next
    }
    if (argument === '--map') options.map = value()
    else if (argument === '--changed-files') options.changedFiles = value()
    else if (argument === '--base-revision') options.baseRevision = value()
    else if (argument === '--all') options.all = true
    else if (argument === '--run') options.run = true
    else throw new Error('coverage-argument-invalid')
  }
  if (!safeRelative(options.map) || (options.changedFiles !== null && !safeRelative(options.changedFiles)) ||
      options.all === (options.changedFiles !== null) ||
      (options.baseRevision !== null && !FULL_SHA.test(options.baseRevision))) {
    throw new Error('coverage-arguments-invalid')
  }
  return options
}

function loadJson(file, root) {
  const buffer = readRegularFileInsideRoot(root, file)
  if (buffer === null) return null
  try {
    return JSON.parse(UTF8.decode(buffer))
  } catch {
    return null
  }
}

function loadChangedPaths(file, root) {
  const buffer = readRegularFileInsideRoot(root, file)
  if (buffer === null) return null
  try {
    const lines = UTF8.decode(buffer).split('\n')
    if (lines.at(-1) === '') lines.pop()
    return lines.map((item) => item.endsWith('\r') ? item.slice(0, -1) : item)
  } catch {
    return null
  }
}

export function main(argv = process.argv.slice(2), { root = ROOT, print = console.log, error = console.error, spawn = spawnSync } = {}) {
  let options
  try {
    options = parseArgs(argv)
  } catch {
    error('usage: plan-rust-slice-coverage.mjs [--map <relative.json>] (--changed-files <relative.txt> | --all) [--base-revision <full-sha>] [--run]')
    return 2
  }
  const map = loadJson(options.map, root)
  if (!map) {
    error('RUST_SLICE_COVERAGE_ERROR=coverage-map-read-failed')
    return 1
  }
  const changedPaths = options.all ? map.entries.flatMap((entry) => entry.files) : loadChangedPaths(options.changedFiles, root)
  if (!changedPaths) {
    error('RUST_SLICE_COVERAGE_ERROR=changed-paths-read-failed')
    return 1
  }
  const staticTestOnlyInspection = options.all && !options.run && options.baseRevision === null
  const selection = staticTestOnlyInspection
    ? selectStaticAllEntries(map, root)
    : selectCoverageEntries(map, changedPaths, root, { baseRevision: options.baseRevision })
  print(`RUST_SLICE_COVERAGE_STATUS=${selection.state}`)
  for (const item of selection.errors) error(`RUST_SLICE_COVERAGE_ERROR=${item.rule}`)
  if (!selection.ok) return 1
  for (const entry of selection.entries) {
    print(`RUST_SLICE_COVERAGE_ENTRY=${entry.id}`)
    if (entry.kind === WINDOWS_PLATFORM_KIND) print(`RUST_SLICE_PLATFORM_E2E_REQUIRED=${entry.id}`)
    if (entry.kind === LOCALIZATION_KIND) print(`RUST_SLICE_LOCALIZATION_DIFFERENTIAL_REQUIRED=${entry.id}`)
    if (entry.kind === TEST_ONLY_LOCALIZATION_KIND) {
      print(`RUST_SLICE_TEST_ONLY_LOCALIZATION_REQUIRED=${entry.id}`)
      if (staticTestOnlyInspection) print(`RUST_SLICE_TEST_ONLY_LOCALIZATION_BASE_PROOF_DEFERRED=${entry.id}`)
    }
    if (entry.kind === IGNORED_TEST_RELOCATION_KIND) {
      print(`RUST_SLICE_IGNORED_TEST_RELOCATION_REQUIRED=${entry.id}`)
    }
    if (entry.kind === MODULE_EXPORT_GLUE_KIND) print(`RUST_SLICE_MODULE_EXPORT_GLUE_DIFFERENTIAL_REQUIRED=${entry.id}`)
    if (entry.kind === STRUCTURAL_KIND) print(`RUST_SLICE_STRUCTURAL_CONTRACT_REQUIRED=${entry.id}`)
  }
  if (!options.run || selection.entries.length === 0) return 0
  const execution = runCoveragePlan(selection.entries, { root, spawn })
  for (const item of execution.errors) error(`RUST_SLICE_COVERAGE_ERROR=${item.rule}`)
  return execution.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
