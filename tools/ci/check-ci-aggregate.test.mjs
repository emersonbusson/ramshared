import assert from 'node:assert/strict'
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  validateAggregateNeeds,
  validateReusableAggregateArchitecture,
} from './check-ci-contract.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')

function contractFixture() {
  return {
    schema_version: 1,
    contract_state: 'PARTIAL',
    p0_requirements: [{ id: 'aggregate', gate_ids: ['ci-contract', 'aggregate'] }],
    gates: [
      {
        id: 'ci-contract',
        required: true,
        implementation: 'current',
        workflow: '.github/workflows/ci-contract.yml',
        job: 'contract',
        context: 'ci-contract',
        trust: 'pull-request',
        triggers: ['pull_request', 'push-main'],
        selection: { mode: 'always', paths: [] },
        policy: {
          timeout_minutes: 10,
          permissions: { contents: 'read' },
          permissions_scope: 'job',
          action_pinning: 'full-sha',
          continue_on_error: false,
          retry_class: 'none',
          concurrency: { cancel_in_progress: true },
        },
        required_commands: ['node tools/ci/check-ci-contract.mjs --check-local'],
        open_gaps: [],
      },
      {
        id: 'aggregate',
        required: true,
        implementation: 'current',
        workflow: '.github/workflows/ci-contract.yml',
        job: 'aggregate',
        context: 'required-checks',
        trust: 'pull-request',
        triggers: ['pull_request', 'push-main'],
        selection: { mode: 'always', paths: [] },
        policy: {
          timeout_minutes: 10,
          permissions: { contents: 'read' },
          permissions_scope: 'job',
          action_pinning: 'full-sha',
          continue_on_error: false,
          retry_class: 'none',
          concurrency: { cancel_in_progress: true },
        },
        required_commands: ['node tools/ci/check-ci-contract.mjs --aggregate-needs'],
        open_gaps: [],
      },
    ],
    aggregate: {
      id: 'aggregate',
      implementation: 'current',
      workflow: '.github/workflows/ci-contract.yml',
      job: 'aggregate',
      open_gaps: [],
      architecture: {
        kind: 'local-reusable-needs-v1',
        callers: [
          { job: 'contract', gates: ['ci-contract'], kind: 'direct' },
        ],
      },
    },
  }
}

test('aggregate_reusable_workflow_architecture_rejects_missing_summary_or_needs', () => {
  const workflow = [
    'name: CI Contract',
    'on:',
    '  pull_request:',
    'permissions: {}',
    'jobs:',
    '  contract:',
    '    name: ci-contract',
    '    runs-on: ubuntu-latest',
    '    timeout-minutes: 10',
    '    permissions:',
    '      contents: read',
    '    steps:',
    '      - run: node tools/ci/check-ci-contract.mjs --check-local',
    '  aggregate:',
    '    name: required-checks',
    '    runs-on: ubuntu-latest',
    '    timeout-minutes: 10',
    '    permissions:',
    '      contents: read',
    '    steps:',
    '      - run: node tools/ci/check-ci-contract.mjs --aggregate-needs',
  ].join('\n')
  const result = validateReusableAggregateArchitecture(contractFixture(), workflow)
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'aggregate-if-always-missing'), true)
  assert.equal(result.errors.some((item) => item.rule === 'aggregate-needs-mismatch'), true)
})

test('aggregate_needs_rejects_cancelled_or_skipped_caller', () => {
  const contract = contractFixture()
  for (const result of ['cancelled', 'skipped']) {
    const aggregate = validateAggregateNeeds(contract, {
      contract: { result },
    }, 'pull_request')
    assert.equal(aggregate.status, 'NO-GO')
    assert.equal(aggregate.errors.some((item) => item.rule === 'aggregate-caller-not-success'), true)
  }
})

test('aggregate_needs_accepts_only_active_success_and_rejects_missing_callers', () => {
  const contract = contractFixture()
  const accepted = validateAggregateNeeds(contract, { contract: { result: 'success' } }, 'pull_request')
  assert.equal(accepted.status, 'PASS')

  const missing = validateAggregateNeeds(contract, {}, 'pull_request')
  assert.equal(missing.status, 'NO-GO')
  assert.equal(missing.errors.some((item) => item.rule === 'aggregate-caller-missing'), true)

  const invalid = validateAggregateNeeds(contract, { contract: { result: 'success' } }, 'unsupported')
  assert.equal(invalid.status, 'NO-GO')
  assert.equal(invalid.errors.some((item) => item.rule === 'aggregate-needs-input-invalid'), true)
})

test('repository_aggregate_is_a_same_run_local_reusable_architecture', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci-contract.yml'), 'utf8')
  const result = validateReusableAggregateArchitecture(contract, workflow)
  assert.equal(result.ok, true)
})

test('canonical_reusable_callers_do_not_reintroduce_duplicate_automatic_triggers', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  for (const caller of contract.aggregate.architecture.callers.filter((item) => item.kind === 'reusable')) {
    const workflow = readFileSync(path.join(ROOT, caller.workflow), 'utf8')
    assert.match(workflow, /^  workflow_call:\s*$/m, caller.workflow)
    assert.doesNotMatch(workflow, /^  (?:pull_request|push):/m, caller.workflow)
  }
})

test('ci_topology_rejects_duplicate_direct_and_reusable_invocation', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const entrypoint = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci-contract.yml'), 'utf8')
  const fixtureRoot = mkdtempSync(path.join(tmpdir(), 'ramshared-ci-topology-'))
  const workflowRoot = path.join(fixtureRoot, '.github', 'workflows')
  mkdirSync(workflowRoot, { recursive: true })
  for (const caller of contract.aggregate.architecture.callers.filter((item) => item.kind === 'reusable')) {
    copyFileSync(path.join(ROOT, caller.workflow), path.join(fixtureRoot, caller.workflow))
  }
  const reusablePath = path.join(fixtureRoot, '.github', 'workflows', 'ci.yml')
  const duplicate = readFileSync(reusablePath, 'utf8').replace('  workflow_call:', '  workflow_call:\n  pull_request:')
  writeFileSync(reusablePath, duplicate)

  const result = validateReusableAggregateArchitecture(contract, entrypoint, { root: fixtureRoot })
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'aggregate-reusable-direct-trigger'), true)
})

test('canonical_entrypoint_revalidates_pull_request_edits', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci-contract.yml'), 'utf8')
  assert.match(
    workflow,
    /^  pull_request:\n    types: \[opened, edited, reopened, synchronize, ready_for_review\]$/m,
  )
})
