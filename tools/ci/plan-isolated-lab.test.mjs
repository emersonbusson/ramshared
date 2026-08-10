import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { validateArtifactManifest } from './check-ci-artifacts.mjs'
import {
  buildLabPlan,
  main,
  validateLabPlanInput,
  writeLabPlan,
} from './plan-isolated-lab.mjs'

const VALID_INPUT = {
  kind: 'windows',
  mode: 'plan',
  target: 'isolated-lab',
  revision: '0123456789abcdef0123456789abcdef01234567',
  environment: 'protected-isolated-lab',
}

test('lab_dispatch_requires_protected_environment', () => {
  const result = validateLabPlanInput({ ...VALID_INPUT, environment: 'daily-host' })
  assert.equal(result.ok, false)
  assert.deepEqual(result.errors, ['lab-environment-invalid'])
})

test('lab_dispatch_rejects_daily_or_physical_target', () => {
  for (const target of ['daily-host', 'physical-host']) {
    const result = validateLabPlanInput({ ...VALID_INPUT, target })
    assert.equal(result.ok, false)
    assert.equal(result.errors.includes('lab-target-invalid'), true)
  }
})

test('lab_dispatch_rejects_untrusted_revision', () => {
  for (const revision of ['main', '0123', '0123456789abcdef0123456789abcdef0123456G']) {
    const result = validateLabPlanInput({ ...VALID_INPUT, revision })
    assert.equal(result.ok, false)
    assert.equal(result.errors.includes('lab-revision-invalid'), true)
  }
})

test('plan_only_dispatch_has_no_host_mutation', () => {
  const plan = buildLabPlan(VALID_INPUT)
  assert.equal(plan.host_action, 'none')
  assert.deepEqual(plan, {
    schema_version: 1,
    kind: 'windows',
    mode: 'plan',
    target: 'isolated-lab',
    revision: VALID_INPUT.revision,
    environment: 'protected-isolated-lab',
    host_action: 'none',
    terminal_status: 'PASS',
  })
  assert.doesNotMatch(JSON.stringify(plan), /driver|service|vm|gpu|disk|swap|shutdown|reboot/i)

  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-lab-plan-'))
  const written = writeLabPlan(VALID_INPUT, {
    outPath: path.join(root, 'plan.json'),
    manifestPath: path.join(root, 'artifact-manifest.json'),
  })
  assert.equal(validateArtifactManifest(written.manifest, { root }).ok, true)
  assert.deepEqual(JSON.parse(readFileSync(path.join(root, 'plan.json'), 'utf8')), plan)
})

test('lab_plan_builder_rejects_invalid_or_nonadjacent_outputs', () => {
  assert.throws(() => buildLabPlan({ ...VALID_INPUT, mode: 'execute' }), /lab-plan-input-invalid/)
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-lab-plan-output-'))
  assert.throws(() => writeLabPlan(VALID_INPUT, {
    outPath: path.join(root, 'plan.json'),
    manifestPath: path.join(root, 'nested', 'artifact-manifest.json'),
  }), /lab-plan-output-invalid/)
})

test('lab_plan_cli_writes_verified_plan_or_refuses_without_echoing_input', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-lab-plan-cli-'))
  const output = []
  assert.equal(main([
    '--kind', 'wsl2', '--out', 'plan.json', '--manifest', 'artifact-manifest.json',
  ], {
    env: { LAB_MODE: 'plan', LAB_TARGET: 'isolated-lab', LAB_REVISION: VALID_INPUT.revision, LAB_ENVIRONMENT: 'protected-isolated-lab' },
    cwd: root,
    print: (line) => output.push(line),
    error: () => assert.fail('valid plan must not emit an error'),
  }), 0)
  assert.equal(output.includes('LAB_PLAN_STATUS=PASS'), true)

  const errors = []
  const privateValue = 'daily-host-private'
  assert.equal(main(['--kind', 'windows', '--out', 'bad.json', '--manifest', 'bad-manifest.json'], {
    env: { LAB_MODE: 'plan', LAB_TARGET: privateValue, LAB_REVISION: 'main', LAB_ENVIRONMENT: 'daily-host' },
    cwd: root,
    print: () => {},
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(errors.every((line) => !line.includes(privateValue)), true)
  assert.equal(errors.includes('LAB_PLAN_ERROR=lab-target-invalid'), true)
  assert.equal(errors.includes('LAB_PLAN_ERROR=lab-revision-invalid'), true)
  assert.equal(errors.includes('LAB_PLAN_ERROR=lab-environment-invalid'), true)
})

test('lab_plan_cli_rejects_invalid_arguments_unsafe_outputs_and_write_failures', () => {
  const usage = []
  assert.equal(main([], { print: () => {}, error: (line) => usage.push(line) }), 2)
  assert.equal(usage[0].startsWith('usage:'), true)

  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-lab-plan-refusal-'))
  const unsafe = []
  assert.equal(main(['--kind', 'windows', '--out', '../plan.json', '--manifest', 'artifact-manifest.json'], {
    env: { LAB_MODE: 'plan', LAB_TARGET: 'isolated-lab', LAB_REVISION: VALID_INPUT.revision, LAB_ENVIRONMENT: 'protected-isolated-lab' },
    cwd: root,
    print: () => {},
    error: (line) => unsafe.push(line),
  }), 1)
  assert.deepEqual(unsafe, ['LAB_PLAN_ERROR=lab-output-path-invalid'])

  const writeFailure = []
  assert.equal(main(['--kind', 'windows', '--out', 'missing/plan.json', '--manifest', 'missing/artifact-manifest.json'], {
    env: { LAB_MODE: 'plan', LAB_TARGET: 'isolated-lab', LAB_REVISION: VALID_INPUT.revision, LAB_ENVIRONMENT: 'protected-isolated-lab' },
    cwd: root,
    print: () => {},
    error: (line) => writeFailure.push(line),
  }), 1)
  assert.deepEqual(writeFailure, ['LAB_PLAN_ERROR=lab-write-failed'])
})
