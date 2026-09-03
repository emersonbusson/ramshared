import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  classifyRetry,
  main,
  readRemoteControlObservation,
  releaseProducerManifestMatchesPublishedTarget,
  run,
  selectWholePullRequestGates,
  validateAggregate,
  validateContract,
  validateRemoteControlObservation,
  validateRemoteControlSchemaDefinition,
  validateWorkflowPolicy,
} from './check-ci-contract.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const RUSTSEC_SNAPSHOT_COMMIT = '6420e39260b3d771b049954cf5d52b57e2118da4'
const RUSTSEC_SNAPSHOT_UTC = '2026-09-02T09:13:32Z'
const REMOTE_OBSERVATION_NOW = Date.parse('2026-08-09T16:00:00Z')

function compliantRemoteObservation(overrides = {}) {
  return {
    schema_version: 1,
    repository: 'emersonbusson/ramshared',
    default_branch: 'main',
    observed_at_utc: '2026-08-09T15:30:00Z',
    source: 'github-rest-api',
    actions: {
      default_workflow_permissions: 'read',
      can_approve_pull_request_reviews: false,
      allowed_actions: 'selected',
      sha_pinning_required: true,
      artifact_and_log_retention_days: 30,
    },
    branch_protection: {
      strict: true,
      enforce_admins: true,
      required_conversation_resolution: true,
      required_status_checks: ['required-checks'],
    },
    environments: {
      'protected-isolated-lab': {
        required_reviewers: true,
        prevent_self_review: true,
        protected_branches: true,
      },
      'protected-release': {
        required_reviewers: true,
        prevent_self_review: true,
        protected_branches: true,
      },
    },
    ...overrides,
  }
}

function currentGate(overrides = {}) {
  return {
    id: 'current-gate',
    required: true,
    implementation: 'current',
    workflow: '.github/workflows/ci.yml',
    job: 'quality',
    context: 'quality',
    trust: 'pull-request',
    triggers: ['pull_request'],
    selection: { mode: 'always', paths: [] },
    policy: {
      timeout_minutes: 10,
      permissions: { contents: 'read' },
      permissions_scope: 'workflow',
      action_pinning: 'full-sha',
      continue_on_error: false,
      retry_class: 'none',
      concurrency: { cancel_in_progress: true },
    },
    required_commands: ['node --test'],
    open_gaps: [],
    ...overrides,
  }
}

function plannedGate(overrides = {}) {
  return {
    id: 'planned-gate',
    required: true,
    implementation: 'planned',
    workflow: '.github/workflows/planned.yml',
    job: 'planned',
    context: 'planned',
    trust: 'pull-request',
    triggers: ['pull_request'],
    selection: { mode: 'paths', paths: ['docs/governance/'] },
    policy: {
      timeout_minutes: 10,
      permissions: { contents: 'read' },
      permissions_scope: 'workflow',
      action_pinning: 'full-sha',
      continue_on_error: false,
      retry_class: 'none',
    },
    required_commands: ['node tools/ci/check-ci-contract.mjs --check'],
    open_gaps: ['workflow-absent'],
    ...overrides,
  }
}

function cargoAuditGate(overrides = {}) {
  return currentGate({
    id: 'cargo-audit',
    policy: {
      ...currentGate().policy,
      advisory_db: {
        url: 'https://github.com/RustSec/advisory-db.git',
        commit: RUSTSEC_SNAPSHOT_COMMIT,
        commit_utc: RUSTSEC_SNAPSHOT_UTC,
        max_age_days: 7,
        upstream_head_health: {
          mode: 'scheduled-curator',
          cannot_override_snapshot_age: true,
        },
      },
    },
    ...overrides,
  })
}

function observedRemoteGate(overrides = {}) {
  return {
    id: 'remote-controls',
    required: true,
    implementation: 'observed',
    context: 'remote-controls',
    trust: 'remote',
    triggers: [],
    selection: { mode: 'never', paths: [] },
    observation: 'docs/governance/remote-controls-observation.json',
    open_gaps: [],
    ...overrides,
  }
}

function contractFixture(overrides = {}) {
  const contract = {
    schema_version: 1,
    contract_state: 'PARTIAL',
    p0_requirements: [
      { id: 'policy', gate_ids: ['current-gate', 'planned-gate'] },
    ],
    gates: [currentGate(), plannedGate()],
    aggregate: {
      id: 'aggregate',
      implementation: 'planned',
      workflow: '.github/workflows/aggregate.yml',
      job: 'aggregate',
      open_gaps: ['workflow-absent', 'aggregate-absent'],
    },
    ...overrides,
  }
  return contract
}

function currentOnlyContract(gate = currentGate(), overrides = {}) {
  return contractFixture({
    p0_requirements: [{ id: 'policy', gate_ids: [gate.id] }],
    gates: [gate],
    ...overrides,
  })
}

function fixtureRoot(workflow = null, contract = contractFixture()) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-ci-contract-'))
  mkdirSync(path.join(root, '.github', 'workflows'), { recursive: true })
  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  if (workflow !== null) writeFileSync(path.join(root, '.github', 'workflows', 'ci.yml'), workflow)
  writeFileSync(path.join(root, 'docs', 'governance', 'ci-contract.json'), `${JSON.stringify(contract, null, 2)}\n`)
  return root
}

function validWorkflow(overrides = '', { concurrency = true } = {}) {
  return [
    'name: Fixture',
    'on:',
    '  pull_request:',
    ...(concurrency ? [
      'concurrency:',
      '  group: fixture-${{ github.ref }}',
      '  cancel-in-progress: true',
    ] : []),
    'permissions:',
    '  contents: read',
    'jobs:',
    '  quality:',
    '    name: quality',
    '    timeout-minutes: 10',
    '    steps:',
    '      - uses: owner/action@0123456789012345678901234567890123456789 # v1.0.0',
    '      - run: node --test',
    overrides,
  ].filter(Boolean).join('\n')
}

test('valid_contract_matches_workflows', () => {
  const root = fixtureRoot(validWorkflow())
  const contract = contractFixture()
  assert.equal(validateContract(contract).ok, true)
  const result = validateWorkflowPolicy(contract, root)
  assert.equal(result.status, 'PARTIAL')
  assert.deepEqual(result.errors, [])
  assert.deepEqual(result.gaps, [
    'aggregate:aggregate-absent',
    'aggregate:workflow-absent',
    'planned-gate:workflow-absent',
  ])
})

test('ci_contract_rejects_removed_required_gate', () => {
  const contract = contractFixture({ gates: [plannedGate()] })
  const result = validateContract(contract)
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'p0-gate-missing'), true)
})

test('contract_helpers_fail_closed_for_non_array_containers', () => {
  const malformed = {
    schema_version: 1,
    contract_state: 'PARTIAL',
    gates: {},
    p0_requirements: {},
    aggregate: null,
  }
  const validation = validateContract(malformed)
  assert.equal(validation.ok, false)
  assert.equal(validation.errors.some((item) => item.rule === 'gates-invalid'), true)
  assert.deepEqual(selectWholePullRequestGates(malformed, ['README.md']).selected, [])
  assert.equal(validateAggregate(malformed, ['unknown'], []).status, 'NO-GO')
})

test('ci_contract_rejects_invalid_schema_policy_selection_and_remote_state', () => {
  const invalidGate = currentGate({
    id: 'invalid-gate',
    required: 'yes',
    implementation: 'unknown',
    workflow: '../unsafe.yml',
    job: '',
    context: '',
    trust: 'unknown',
    triggers: ['pull_request', 1],
    selection: { mode: 'paths', paths: [] },
    policy: {
      timeout_minutes: 0,
      permissions: { contents: 'owner' },
      permissions_scope: 'root',
      action_pinning: 'tag',
      continue_on_error: true,
      retry_class: 'unbounded',
      concurrency: { cancel_in_progress: 'yes' },
    },
    required_commands: [1],
    open_gaps: ['unknown-gap'],
    allowed_continue_on_error_steps: [1],
  })
  const remote = currentGate({
    id: 'remote-gate',
    implementation: 'env-bound',
    context: 'remote-gate',
    trust: 'remote',
    triggers: [],
    selection: { mode: 'never', paths: [] },
    open_gaps: [],
  })
  const missingPolicy = currentGate({ id: 'missing-policy', policy: null })
  const contract = contractFixture({
    contract_state: 'DONE',
    gates: [invalidGate, invalidGate, remote, missingPolicy],
    p0_requirements: [
      { id: 'duplicate', gate_ids: ['invalid-gate'] },
      { id: 'duplicate', gate_ids: ['missing-gate'] },
      { id: '', gate_ids: [] },
    ],
    aggregate: { id: '', implementation: 'unknown', workflow: '../unsafe.yml', job: '', open_gaps: ['unknown-gap'] },
  })
  const result = validateContract(contract)
  assert.equal(result.ok, false)
  for (const rule of [
    'state-invalid', 'required-invalid', 'implementation-invalid',
    'selection-paths-empty', 'open-gaps-invalid', 'workflow-path-invalid',
    'timeout-policy-invalid', 'permissions-policy-invalid',
    'permissions-scope-invalid', 'action-pinning-invalid',
    'continue-on-error-policy-invalid', 'retry-policy-invalid', 'concurrency-policy-invalid', 'policy-missing',
    'gate-id-duplicate', 'env-bound-gap-missing',
    'p0-requirement-duplicate', 'p0-gate-missing', 'p0-requirement-invalid',
    'aggregate-invalid',
  ]) assert.equal(result.errors.some((item) => item.rule === rule), true, rule)
})

test('ci_contract_rejects_continue_on_error_for_gate', () => {
  const root = fixtureRoot(validWorkflow('    continue-on-error: true'))
  const result = validateWorkflowPolicy(contractFixture(), root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'continue-on-error'), true)
})

test('ci_contract_requires_explicit_timeout', () => {
  const root = fixtureRoot(validWorkflow().replace('    timeout-minutes: 10\n', ''))
  const result = validateWorkflowPolicy(contractFixture(), root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'timeout-not-explicit'), true)
})

test('ci_contract_requires_current_gate_concurrency_policy', () => {
  const { concurrency, ...policyWithoutConcurrency } = currentGate().policy
  const result = validateContract(currentOnlyContract(currentGate({ policy: policyWithoutConcurrency })))
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'concurrency-policy-missing'), true)
})

test('ci_specific_policies_reject_malformed_coverage_and_cancellation_rules', () => {
  const rustCoverage = currentGate({
    id: 'rust-slice-coverage',
    context: 'rust-slice-coverage',
    policy: {
      ...currentGate().policy,
      rust_slice_coverage: {
        map: 'unsafe.json',
        planner: 'unsafe.mjs',
        llvm_cov_version: '0.0.0',
        min: 79,
      },
    },
  })
  const malformedCoverage = validateContract(currentOnlyContract(rustCoverage))
  assert.equal(malformedCoverage.ok, false)
  assert.equal(malformedCoverage.errors.some((item) => item.rule === 'rust-slice-coverage-policy-invalid'), true)

  const cancellation = currentGate({
    id: 'closed-pr-cancellation',
    context: 'closed-pr-cancellation',
    trust: 'maintenance',
    triggers: ['pull_request-closed'],
    selection: { mode: 'never', paths: [] },
    policy: {
      ...currentGate().policy,
      permissions: { actions: 'write', 'pull-requests': 'read' },
      closed_pr_cancellation: { merged_only: false, same_repository_only: true },
    },
  })
  const malformedCancellation = validateContract(currentOnlyContract(cancellation))
  assert.equal(malformedCancellation.ok, false)
  assert.equal(malformedCancellation.errors.some((item) => item.rule === 'closed-pr-cancellation-policy-invalid'), true)
})

test('ci_contract_rejects_stale_advisory_snapshot', () => {
  const result = validateContract(currentOnlyContract(cargoAuditGate()), {
    now: Date.parse('2026-09-09T09:13:33Z'),
  })
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'advisory-db-snapshot-stale'), true)
})

test('ci_contract_rejects_missing_or_mismatched_advisory_snapshot', () => {
  const { advisory_db, ...policyWithoutSnapshot } = cargoAuditGate().policy
  const missing = validateContract(currentOnlyContract(cargoAuditGate({ policy: policyWithoutSnapshot })))
  assert.equal(missing.ok, false)
  assert.equal(missing.errors.some((item) => item.rule === 'advisory-db-metadata-missing'), true)

  const gate = cargoAuditGate()
  const contract = currentOnlyContract(gate)
  const mismatchWorkflow = validWorkflow([
    '      - name: Audit wrong snapshot',
    '        env:',
    '          RUSTSEC_DB_URL: https://github.com/RustSec/advisory-db.git',
    `          RUSTSEC_DB_COMMIT: ${'0'.repeat(40)}`,
    `          RUSTSEC_DB_COMMIT_UTC: "${RUSTSEC_SNAPSHOT_UTC}"`,
    '          RUSTSEC_DB_MAX_AGE_DAYS: "7"',
    '        run: |',
    '          git -C "$RUSTSEC_DB_DIR" fetch --depth=1 origin "$RUSTSEC_DB_COMMIT"',
    '          cargo audit --db "$RUSTSEC_DB_DIR" --no-fetch',
    '          echo "RUSTSEC_UPSTREAM_HEAD_HEALTH=scheduled-curator-required"',
  ].join('\n'))
  const mismatch = validateWorkflowPolicy(contract, fixtureRoot(mismatchWorkflow, contract))
  assert.equal(mismatch.status, 'NO-GO')
  assert.equal(mismatch.errors.some((item) => item.rule === 'advisory-db-snapshot-mismatch'), true)
})

test('item3_current_contract_gates_declare_concurrency', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const currentGates = contract.gates.filter((gate) => gate.implementation === 'current')
  assert.equal(currentGates.every((gate) => typeof gate.policy?.concurrency?.cancel_in_progress === 'boolean'), true)
})

test('ci_contract_requires_explicit_workflow_concurrency', () => {
  const gate = currentGate({
    policy: {
      ...currentGate().policy,
      concurrency: { cancel_in_progress: true },
    },
  })
  const root = fixtureRoot(validWorkflow('', { concurrency: false }), currentOnlyContract(gate))
  const result = validateWorkflowPolicy(currentOnlyContract(gate), root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'concurrency-not-explicit'), true)
})

test('ci_contract_rejects_mutable_action_reference', () => {
  const root = fixtureRoot(validWorkflow().replace(/@[0-9a-f]{40}/, '@v1'))
  const result = validateWorkflowPolicy(contractFixture(), root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'mutable-action-reference'), true)
})

test('workflow_policy_rejects_target_context_permissions_and_missing_command', () => {
  const workflow = [
    'name: Unsafe fixture',
    'on:',
    '  pull_request_target:',
    'jobs:',
    '  quality:',
    '    name: wrong-context',
    '    timeout-minutes: 9',
    '    steps:',
    '      - uses: ./local-action',
  ].join('\n')
  const gate = currentGate({
    context: 'quality',
    triggers: ['push-main'],
    policy: { ...currentGate().policy, permissions_scope: 'job' },
    required_commands: ['required command'],
  })
  const root = fixtureRoot(workflow, currentOnlyContract(gate))
  const result = validateWorkflowPolicy(currentOnlyContract(gate), root)
  assert.equal(result.status, 'NO-GO')
  for (const rule of [
    'pull-request-target', 'trigger-absent', 'context-mismatch',
    'timeout-mismatch', 'permissions-not-explicit', 'required-command-absent',
  ]) assert.equal(result.errors.some((item) => item.rule === rule), true, rule)
})

test('workflow_policy_rejects_best_effort_steps even when a legacy allowlist names them', () => {
  const workflow = [
    'name: Scoped fixture',
    'on:',
    '  pull_request:',
    'concurrency:',
    '  group: fixture-${{ github.ref }}',
    '  cancel-in-progress: true',
    'jobs:',
    '  quality:',
    '    name: quality',
    '    timeout-minutes: 10',
    '    permissions:',
    '      contents: read',
    '    steps:',
    '      - uses: owner/action@0123456789012345678901234567890123456789 # v1.0.0',
    '      - name: Upload static result',
    '        continue-on-error: true',
    '      - run: node --test',
  ].join('\n')
  const gate = currentGate({
    policy: { ...currentGate().policy, permissions_scope: 'job' },
    allowed_continue_on_error_steps: ['Upload static result'],
  })
  const root = fixtureRoot(workflow, currentOnlyContract(gate))
  const result = validateWorkflowPolicy(currentOnlyContract(gate), root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'continue-on-error'), true)
})

test('required commands must be reachable standalone commands, not tolerated text', () => {
  const fixtures = [
    {
      name: 'job false literal',
      workflow: validWorkflow().replace('    name: quality', '    if: false\n    name: quality'),
      rule: 'required-command-unreachable',
    },
    {
      name: 'job quoted false literal',
      workflow: validWorkflow().replace('    name: quality', "    if: 'false'\n    name: quality"),
      rule: 'required-command-unreachable',
    },
    {
      name: 'step false expression',
      workflow: validWorkflow().replace('      - run: node --test', '      - if: ${{ false }}\n        run: node --test'),
      rule: 'required-command-unreachable',
    },
    {
      name: 'event exclusion',
      workflow: validWorkflow().replace('      - run: node --test', "      - if: github.event_name == 'push'\n        run: node --test"),
      rule: 'required-command-unreachable',
    },
    {
      name: 'step continuation expression',
      workflow: validWorkflow().replace('      - run: node --test', '      - continue-on-error: ${{ github.event_name == \'pull_request\' }}\n        run: node --test'),
      rule: 'required-command-continue-on-error',
    },
    {
      name: 'background command',
      workflow: validWorkflow().replace('      - run: node --test', '      - run: node --test &'),
      rule: 'required-command-failure-masked',
    },
    {
      name: 'or true',
      workflow: validWorkflow().replace('      - run: node --test', '      - run: node --test || true'),
      rule: 'required-command-failure-masked',
    },
    ...[
      ['pipeline', 'node --test | cat'],
      ['pipeline stderr', 'node --test |& cat'],
      ['or fallback', 'node --test || fallback'],
      ['trailing semicolon true', 'node --test; true'],
      ['trailing and true', 'node --test && true'],
      ['shell negation', '! node --test'],
      ['conditional context', 'if node --test; then echo done; fi'],
      ['command substitution fallback', 'echo "$(node --test)" || fallback'],
      ['subshell fallback', '(node --test) || fallback'],
    ].map(([name, run]) => ({
      name,
      workflow: validWorkflow().replace('      - run: node --test', `      - run: ${run}`),
      rule: 'required-command-failure-masked',
    })),
    {
      name: 'exit zero',
      workflow: validWorkflow().replace('      - run: node --test', '      - run: node --test; exit 0'),
      rule: 'required-command-failure-masked',
    },
    {
      name: 'heredoc only',
      workflow: validWorkflow().replace('      - run: node --test', [
        '      - run: |',
        "          cat <<'COMMAND'",
        '          node --test',
        '          COMMAND',
      ].join('\n')),
      rule: 'required-command-heredoc-only',
    },
    {
      name: 'shell comment only',
      workflow: validWorkflow().replace('      - run: node --test', '      - run: # node --test'),
      rule: 'required-command-not-executable',
    },
    {
      name: 'echo text only',
      workflow: validWorkflow().replace('      - run: node --test', '      - run: echo "node --test"'),
      rule: 'required-command-not-executable',
    },
    {
      name: 'custom step shell masks failure',
      workflow: validWorkflow().replace('      - run: node --test', '      - shell: bash {0}\n        run: node --test'),
      rule: 'required-command-shell-unsafe',
    },
    {
      name: 'job default shell masks failure',
      workflow: validWorkflow().replace('    steps:', '    defaults:\n      run:\n        shell: bash {0}\n    steps:'),
      rule: 'required-command-shell-unsafe',
    },
    {
      name: 'workflow default shell masks failure',
      workflow: validWorkflow().replace('permissions:', 'defaults:\n  run:\n    shell: bash {0}\n\npermissions:'),
      rule: 'required-command-shell-unsafe',
    },
  ]
  for (const fixture of fixtures) {
    const result = validateWorkflowPolicy(currentOnlyContract(), fixtureRoot(fixture.workflow, currentOnlyContract()))
    assert.equal(result.status, 'NO-GO', fixture.name)
    assert.equal(result.errors.some((item) => item.rule === fixture.rule), true, fixture.name)
  }
})

test('required command accepts a reachable default-shell and a matching event condition', () => {
  const workflow = validWorkflow().replace('      - run: node --test', "      - if: github.event_name == 'pull_request'\n        run: node --test")
  const result = validateWorkflowPolicy(currentOnlyContract(), fixtureRoot(workflow, currentOnlyContract()))
  assert.equal(result.status, 'PARTIAL')
  assert.equal(result.errors.some((item) => item.rule.startsWith('required-command-')), false)
})

test('workflow_policy_rejects_stale_gap_and_missing_current_workflow', () => {
  const staleGate = currentGate({ open_gaps: ['timeout-not-explicit'] })
  const staleRoot = fixtureRoot(validWorkflow(), currentOnlyContract(staleGate))
  const stale = validateWorkflowPolicy(currentOnlyContract(staleGate), staleRoot)
  assert.equal(stale.status, 'NO-GO')
  assert.equal(stale.errors.some((item) => item.rule === 'stale-open-gap'), true)

  const missing = validateWorkflowPolicy(currentOnlyContract(), fixtureRoot(null, currentOnlyContract()))
  assert.equal(missing.status, 'NO-GO')
  assert.equal(missing.errors.some((item) => item.rule === 'workflow-absent'), true)
})

test('whole_pull_request_change_remains_selected', () => {
  const contract = contractFixture()
  const complete = selectWholePullRequestGates(contract, [
    'docs/governance/ci-contract.json',
    'README.md',
  ])
  const lastPushOnly = selectWholePullRequestGates(contract, ['README.md'])
  assert.equal(complete.selected.includes('planned-gate'), true)
  assert.equal(lastPushOnly.selected.includes('planned-gate'), false)
})

test('selector_rejects_unsafe_changed_path_and_reports_no_change', () => {
  const result = selectWholePullRequestGates(contractFixture(), ['../unsafe'])
  assert.equal(result.errors.some((item) => item.rule === 'changed-path-unsafe'), true)
  assert.equal(result.no_change.includes('planned-gate'), true)
})

test('ci_result_aggregator_fails_on_cancelled_selected_job', () => {
  const contract = contractFixture({ gates: [currentGate()] })
  const result = validateAggregate(contract, ['current-gate'], [
    { id: 'current-gate', state: 'CANCELLED' },
  ])
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'cancelled-result'), true)
})

test('retry_policy_rejects_unknown_failure_class', () => {
  assert.deepEqual(classifyRetry('unknown-failure', 1), {
    retry: false,
    delay_seconds: 0,
    reason: 'retry-not-permitted',
  })
  assert.deepEqual(classifyRetry('dependency-fetch-transport', 1), {
    retry: true,
    delay_seconds: 15,
    reason: 'classified-transient',
  })
  assert.deepEqual(classifyRetry('dependency-fetch-transport', 2), {
    retry: false,
    delay_seconds: 0,
    reason: 'retry-budget-exhausted',
  })
})

test('aggregate_accepts_contractual_no_change_only', () => {
  const gate = currentGate({ selection: { mode: 'paths', paths: ['crates/'] } })
  const contract = contractFixture({ gates: [gate] })
  const accepted = validateAggregate(contract, ['current-gate'], [
    { id: 'current-gate', state: 'NO_CHANGE' },
  ])
  assert.equal(accepted.status, 'PASS')

  const rejected = validateAggregate(contractFixture({ gates: [currentGate()] }), ['current-gate'], [
    { id: 'current-gate', state: 'NO_CHANGE' },
  ])
  assert.equal(rejected.status, 'NO-GO')
  assert.equal(rejected.errors.some((item) => item.rule === 'unexpected-no-change'), true)
})

test('aggregate_rejects_invalid_duplicate_missing_unknown_skipped_and_planned_pass', () => {
  const contract = contractFixture()
  const result = validateAggregate(contract, ['unknown', 'current-gate', 'planned-gate'], [
    { id: 'current-gate', state: 'PASS' },
    { id: 'current-gate', state: 'SKIPPED' },
    { id: 'planned-gate', state: 'PASS' },
    { id: 'invalid', state: 'NOT_A_STATE' },
  ])
  assert.equal(result.status, 'NO-GO')
  for (const rule of [
    'result-invalid', 'duplicate-result', 'selected-gate-unknown',
    'unexpected-skip', 'planned-gate-passed',
  ]) assert.equal(result.errors.some((item) => item.rule === rule), true, rule)

  const missing = validateAggregate(currentOnlyContract(), ['current-gate'], [])
  assert.equal(missing.errors.some((item) => item.rule === 'missing-result'), true)
})

test('ci_contract_local_gate_accepts_compliant_observed_remote_controls', () => {
  const result = run({ root: ROOT })
  assert.equal(result.status, 'PASS')
  assert.deepEqual(result.errors, [])
  assert.deepEqual(result.gaps, [])
  const output = []
  assert.equal(main(['--check-local'], { root: ROOT, print: (line) => output.push(line), error: () => {} }), 0)
  assert.equal(output.includes('CI_CONTRACT_STATUS=PASS'), true)
  assert.equal(output.includes('CI_CONTRACT_VERDICT=PASS'), true)
})

test('item3_hardened_workflows_clear_current_hosted_gaps', () => {
  const result = run({ root: ROOT })
  const hostedGaps = result.gaps.filter((item) => /^(?:rust-quality|docs-integrity|validation-schema|comment-language|pr-body|gitleaks|cargo-audit|cargo-deny|trivy|release-automation):/.test(item))
  assert.deepEqual(hostedGaps, [])
})

test('item4_windows_static_gate_is_current_and_fork_safe', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'windows-static')
  assert.equal(gate.implementation, 'current')
  assert.equal(gate.policy.permissions_scope, 'job')
  assert.deepEqual(gate.policy.permissions, { contents: 'read' })
  assert.equal(gate.policy.concurrency.cancel_in_progress, true)
  assert.deepEqual(gate.open_gaps, [])

  const workflow = readFileSync(path.join(ROOT, gate.workflow), 'utf8')
  assert.match(workflow, /runs-on: windows-latest/)
  assert.match(workflow, /cargo test -p ramshared-winbroker/)
  assert.match(workflow, /cargo test -p ramshared-winsvc/)
  assert.match(workflow, /Test-WindowsCiStatic\.ps1/)
  assert.doesNotMatch(workflow, /pull_request_target|Install-RamShared|Start-RamSharedLab|Restart-Computer|shutdown\.exe/i)

  const wrapper = readFileSync(path.join(ROOT, 'scripts', 'windows', 'Test-WindowsCiStatic.ps1'), 'utf8')
  for (const harness of [
    'Test-AutonomousBrokerStatic.ps1',
    'Test-ProductOnlineStatic.ps1',
    'Test-RamSharedInfIsolationStatic.ps1',
    'Test-WindowsDiskCounterAuditStatic.ps1',
    'Test-WindowsStorageMatrixStatic.ps1',
    'Test-Win11LabMediaContractStatic.ps1',
    'Test-Win11LabReadyStatic.ps1',
    'Test-Win11LabDriverTestFirmwareStatic.ps1',
    'Test-WinDriveIoctlValidationStatic.ps1',
    'Test-HostAutonomousLifecycleStatic.ps1',
    'Test-GuestExhaustiveStatic.ps1',
    'Test-RecoverGuestVerifierExactRunStatic.ps1',
  ]) assert.match(wrapper, new RegExp(harness.replace('.', '\\.')))

  const result = run({ root: ROOT })
  assert.equal(result.errors.some((item) => item.gate === 'windows-static'), false)
  assert.equal(result.gaps.some((item) => item.startsWith('windows-static:')), false)
})

test('ci_contract_requires_isolated_lab_plan_policy', () => {
  const gate = currentGate({
    id: 'windows-lab-plan',
    context: 'windows-lab-plan',
    workflow: '.github/workflows/windows-lab.yml',
    job: 'windows-lab-plan',
    trust: 'isolated-lab',
    triggers: ['workflow_dispatch'],
    selection: { mode: 'never', paths: [] },
  })
  const result = validateContract(currentOnlyContract(gate))
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'lab-plan-policy-missing'), true)
})

test('item5_protected_lab_workflows_are_current_and_plan_only', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  for (const [id, kind] of [['windows-lab-plan', 'windows'], ['wsl2-lab-plan', 'wsl2']]) {
    const gate = contract.gates.find((item) => item.id === id)
    assert.equal(gate.implementation, 'current')
    assert.equal(gate.trust, 'isolated-lab')
    assert.equal(gate.policy.permissions_scope, 'job')
    assert.deepEqual(gate.policy.permissions, { contents: 'read' })
    assert.deepEqual(gate.policy.lab_plan, {
      kind,
      mode: 'plan',
      target: 'isolated-lab',
      environment: 'protected-isolated-lab',
      host_action: 'none',
    })
    assert.deepEqual(gate.open_gaps, [])

    const workflow = readFileSync(path.join(ROOT, gate.workflow), 'utf8')
    assert.match(workflow, /workflow_dispatch:/)
    assert.match(workflow, /environment: protected-isolated-lab/)
    assert.match(workflow, /runs-on: ubuntu-latest/)
    assert.match(workflow, /LAB_MODE: \$\{\{ inputs\.mode \}\}/)
    assert.match(workflow, /LAB_TARGET: \$\{\{ inputs\.target \}\}/)
    assert.match(workflow, /LAB_REVISION: \$\{\{ github\.sha \}\}/)
    assert.match(workflow, new RegExp(`plan-isolated-lab\\.mjs --kind ${kind}`))
    assert.match(workflow, /check-ci-artifacts\.mjs --check/)
    assert.doesNotMatch(workflow, /self-hosted|Install-|Start-Service|Stop-Service|sc\.exe|New-VM|Start-VM|wsl\.exe|Restart-Computer|shutdown\.exe|diskpart|bcdedit|nvidia-smi/i)
  }

  const result = run({ root: ROOT })
  assert.equal(result.errors.some((item) => /^(?:windows-lab-plan|wsl2-lab-plan)$/.test(item.gate)), false)
  assert.equal(result.gaps.some((item) => /^(?:windows-lab-plan|wsl2-lab-plan):/.test(item)), false)
})

test('ci_contract_requires_release_integrity_policy', () => {
  const gate = currentGate({
    id: 'release-integrity',
    context: 'release-integrity',
    workflow: '.github/workflows/release-integrity.yml',
    job: 'release-integrity',
    trust: 'release',
    triggers: ['push-tag'],
    selection: { mode: 'never', paths: [] },
  })
  const result = validateContract(currentOnlyContract(gate))
  assert.equal(result.ok, false)
  assert.equal(result.errors.some((item) => item.rule === 'release-integrity-policy-missing'), true)
})

test('item6_release_integrity_workflow_is_current_and_nonpublishing', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'release-integrity')
  assert.equal(gate.implementation, 'current')
  assert.equal(gate.trust, 'release')
  assert.deepEqual(gate.triggers, ['push-tag', 'workflow_dispatch'])
  assert.equal(gate.policy.permissions_scope, 'job')
  assert.deepEqual(gate.policy.permissions, { contents: 'read' })
  assert.deepEqual(gate.policy.release_integrity, {
    sbom_generator: { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.5' },
    publication: 'forbidden',
    promotion_policy: 'docs/governance/release-promotion.json',
    target_tag: 'v0.9.0-beta.1',
    integrity_artifact_retention_days: 14,
  })
  assert.deepEqual(gate.open_gaps, [])

  const workflow = readFileSync(path.join(ROOT, gate.workflow), 'utf8')
  assert.match(workflow, /push:/)
  assert.match(workflow, /- 'v0\.9\.0-beta\.1'/)
  assert.doesNotMatch(workflow, /^\s*environment:/m)
  assert.match(workflow, /git diff --quiet/)
  assert.match(workflow, /cargo install cargo-cyclonedx --locked --version 0\.5\.9/)
  assert.match(workflow, /cargo cyclonedx --manifest-path Cargo\.toml --format json --spec-version 1\.5 --override-filename ramshared-sbom\.cdx/)
  assert.match(workflow, /--input crates\/ramshared-cli\/ramshared-sbom\.cdx\.json/)
  assert.match(workflow, /--input crates\/ramshared-wsl2d\/ramshared-sbom\.cdx\.json/)
  assert.match(workflow, /--out artifacts\/release\/ramshared-sbom\.cdx\.json/)
  assert.match(workflow, /sha256sum "\$archive" > "\$archive\.sha256"/)
  assert.match(workflow, /--checksum "artifacts\/release\/ramshared-linux-\$RELEASE_TAG\.tar\.gz\.sha256"/)
  assert.match(workflow, /write-release-manifest\.mjs/)
  assert.match(workflow, /node "\$integrity_checker" --check/)
  assert.match(workflow, /actions\/upload-artifact@[0-9a-f]{40}/)
  assert.match(workflow, /name: release-integrity-\$\{\{ env\.RELEASE_TAG \}\}-\$\{\{ steps\.identity\.outputs\.revision \}\}/)
  assert.match(workflow, /retention-days: 14/)
  assert.doesNotMatch(workflow, /gh release|upload-release-asset|action-gh-release|create-release|contents:\s*write/i)

  const result = run({ root: ROOT })
  assert.equal(result.errors.some((item) => item.gate === 'release-integrity'), false)
  assert.equal(result.gaps.some((item) => item.startsWith('release-integrity:')), false)
})

test('clean_checkout_ci_enforces_committed_candidate_public_hygiene', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci.yml'), 'utf8')
  const docsCheck = readFileSync(path.join(ROOT, 'scripts', 'docs-check.sh'), 'utf8')
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'docs-integrity')
  assert.match(workflow, /name: Committed public artifact hygiene[\s\S]*node tools\/ci\/check-public-hygiene\.mjs --candidate/)
  assert.match(docsCheck, /^run_gate public-hygiene node tools\/ci\/check-public-hygiene\.mjs --candidate$/m)
  assert.ok(gate.required_commands.includes('node tools/ci/check-public-hygiene.mjs --candidate'))
  assert.match(readFileSync(path.join(ROOT, 'tools', 'ci', 'check-public-hygiene.test.mjs'), 'utf8'), /clean_checkout_committed_public_text_extensions_fail_closed/)
})

test('release_integrity_recovery_is_exact_tag_sha_read_only', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'release-integrity.yml'), 'utf8')
  assert.match(workflow, /^  workflow_dispatch:\s*$/m)
  assert.match(workflow, /^      tag:\s*$/m)
  assert.match(workflow, /^      source_sha:\s*$/m)
  assert.match(workflow, /format\('Release Integrity recovery \{0\} @ \{1\}', inputs\.tag, inputs\.source_sha\)/)
  assert.match(workflow, /if: github\.event_name == 'push' \|\| github\.ref == 'refs\/heads\/main'/)
  assert.match(workflow, /ref: \$\{\{ github\.event_name == 'workflow_dispatch' && inputs\.source_sha \|\| github\.sha \}\}/)
  assert.match(workflow, /test "\$GITHUB_REF" = refs\/heads\/main/)
  assert.match(workflow, /test "\$RELEASE_TAG" = "\$RELEASE_TARGET_TAG"/)
  assert.match(workflow, /test "\$event_revision" = "\$tag_revision"/)
  assert.match(workflow, /ref: \$\{\{ github\.event\.repository\.default_branch \}\}/)
  assert.match(workflow, /tmp\/recovery-policy\/tools\/ci\/merge-release-sboms\.mjs/)
  assert.match(workflow, /\$RUNNER_TEMP\/release-policy\/merge-release-sboms\.mjs/)
  for (const helper of ['write-release-manifest.mjs', 'check-release-integrity.mjs', 'check-ci-artifacts.mjs']) {
    const escaped = helper.replaceAll('.', '\\.')
    assert.match(workflow, new RegExp(`tmp/recovery-policy/tools/ci/${escaped}`))
    assert.match(workflow, new RegExp(`\\$RUNNER_TEMP/release-policy/${escaped}`))
  }
  assert.match(workflow, /manifest_writer="\$RUNNER_TEMP\/release-policy\/write-release-manifest\.mjs"/)
  assert.match(workflow, /integrity_checker="\$RUNNER_TEMP\/release-policy\/check-release-integrity\.mjs"/)
  assert.doesNotMatch(workflow, /environment:|contents:\s*write|gh release|upload-release-asset|create-release/i)
})

test('release_integrity_recovery_current_parser_accepts_v0_9_0_beta_1_without_publication', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-release-recovery-'))
  const releaseDir = path.join(root, 'artifacts', 'release')
  mkdirSync(releaseDir, { recursive: true })
  let revision
  let lockfile
  try {
    revision = execFileSync('git', ['rev-parse', 'v0.9.0-beta.1^{commit}'], { cwd: ROOT, encoding: 'utf8' }).trim()
    lockfile = execFileSync('git', ['show', 'v0.9.0-beta.1:Cargo.lock'], { cwd: ROOT })
  } catch {
    revision = '361427a63cbeb2a8b0ecafb224adeecb0539af9b'
    try {
      lockfile = execFileSync('git', ['show', `${revision}:Cargo.lock`], { cwd: ROOT })
    } catch {
      lockfile = readFileSync(path.join(ROOT, 'Cargo.lock'))
    }
  }
  writeFileSync(path.join(root, 'Cargo.lock'), lockfile)
  const bundleName = 'ramshared-linux-v0.9.0-beta.1.tar.gz'
  const bundle = Buffer.from('historical beta recovery fixture\n')
  writeFileSync(path.join(releaseDir, bundleName), bundle)
  const bundleHash = createHash('sha256').update(bundle).digest('hex')
  writeFileSync(path.join(releaseDir, `${bundleName}.sha256`), `${bundleHash}  ${bundleName}\n`)
  writeFileSync(path.join(releaseDir, 'ramshared-sbom.cdx.json'), `${JSON.stringify({
    bomFormat: 'CycloneDX',
    specVersion: '1.5',
    metadata: {
      tools: { components: [{ name: 'cargo-cyclonedx', version: '0.5.9' }] },
      component: {
        type: 'application',
        name: 'ramshared',
        version: '0.9.0-beta.1',
        components: [{ name: 'ramshared-cli' }, { name: 'ramshared-wsl2d' }],
      },
      properties: [
        { name: 'ramshared:release:tag', value: 'v0.9.0-beta.1' },
        { name: 'ramshared:source:revision', value: revision },
      ],
    },
  })}\n`)

  const manifestPath = 'artifacts/release/release-manifest.json'
  const writer = spawnSync(process.execPath, [
    path.join(ROOT, 'tools', 'ci', 'write-release-manifest.mjs'),
    '--tag', 'v0.9.0-beta.1',
    '--revision', revision,
    '--rust-version', '1.98.0',
    '--rust-commit', '1'.repeat(40),
    '--bundle', `artifacts/release/${bundleName}`,
    '--checksum', `artifacts/release/${bundleName}.sha256`,
    '--sbom', 'artifacts/release/ramshared-sbom.cdx.json',
    '--prior-release', 'none',
    '--rollback-trigger', 'bundle checksum mismatch',
    '--out', manifestPath,
    '--clean-tree',
  ], { cwd: root, encoding: 'utf8' })
  assert.equal(writer.status, 0, `${writer.stdout}${writer.stderr}`)
  const checker = spawnSync(process.execPath, [
    path.join(ROOT, 'tools', 'ci', 'check-release-integrity.mjs'),
    '--check', manifestPath,
    '--root', root,
    '--tag', 'v0.9.0-beta.1',
    '--revision', revision,
  ], { cwd: root, encoding: 'utf8' })
  assert.equal(checker.status, 0, `${checker.stdout}${checker.stderr}`)

  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'release-integrity.yml'), 'utf8')
  assert.doesNotMatch(workflow, /gh release|upload-release-asset|create-release|contents:\s*write/i)
})

test('release_integrity_refuses_any_deployment_environment', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'release-integrity')
  const workflow = readFileSync(path.join(ROOT, gate.workflow), 'utf8')
    .replace('    timeout-minutes: 45\n', '    timeout-minutes: 45\n    environment: protected-release\n')
  const isolated = currentOnlyContract(gate)
  const root = fixtureRoot(null, isolated)
  writeFileSync(path.join(root, gate.workflow), workflow)

  const result = validateWorkflowPolicy(isolated, root)
  assert.equal(result.errors.some((item) => item.rule === 'release-integrity-environment-mismatch'), true)
})

test('release_producer_requires_github_app_token_without_fallback', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'release.yml'), 'utf8')
  const config = JSON.parse(readFileSync(path.join(ROOT, 'release-please-config.json'), 'utf8'))
  const manifest = JSON.parse(readFileSync(path.join(ROOT, '.release-please-manifest.json'), 'utf8'))
  assert.match(workflow, /name: Require release GitHub App credentials/)
  assert.match(workflow, /test -n "\$RELEASE_APP_ID"/)
  assert.match(workflow, /test -n "\$RELEASE_APP_PRIVATE_KEY"/)
  assert.match(workflow, /uses: actions\/create-github-app-token@[0-9a-f]{40}/)
  assert.match(workflow, /token: \$\{\{ steps\.release-app-token\.outputs\.token \}\}/)
  assert.match(workflow, /git ls-remote --refs/)
  assert.match(workflow, /refs\/tags\/\$RELEASE_TARGET_TAG/)
  assert.match(workflow, /if: \$\{\{ steps\.target\.outputs\.run == 'true' \}\}/)
  assert.doesNotMatch(workflow, /RELEASE_PLEASE_TOKEN|GITHUB_TOKEN|\bPAT\b|\|\|/)
  assert.equal(config.$schema, 'https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json')
  assert.equal(config['release-type'], 'simple')
  assert.equal(config.versioning, 'prerelease')
  assert.equal(config['prerelease-type'], 'beta')
  assert.equal(config.prerelease, true)
  assert.equal(config.draft, true)
  assert.equal(config['force-tag-creation'], true)
  assert.equal(config['skip-github-release'], false)
  assert.equal(Object.hasOwn(config, 'release-as'), false)
  assert.equal(Object.hasOwn(config, 'last-release-sha'), false)
  assert.equal(releaseProducerManifestMatchesPublishedTarget(manifest, {
    target_tag: 'v0.9.0-beta.1',
  }), true)
  assert.match(config['pull-request-header'], /draft prerelease/i)
})

test('release_producer_accepts_only_exact_released_manifest', () => {
  const policy = {
    target_tag: 'v0.9.0-beta.1',
  }

  assert.equal(releaseProducerManifestMatchesPublishedTarget({ '.': '0.9.0-beta.1' }, policy), true)

  for (const version of ['0.8.0', '0.8.1', '0.9.0', '0.9.0-beta.2', '', null]) {
    assert.equal(releaseProducerManifestMatchesPublishedTarget({ '.': version }, policy), false)
  }
  assert.equal(releaseProducerManifestMatchesPublishedTarget({ '.': '0.8.0', other: '0.8.0' }, policy), false)
  assert.equal(releaseProducerManifestMatchesPublishedTarget({}, policy), false)
  assert.equal(releaseProducerManifestMatchesPublishedTarget(null, policy), false)

  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const producer = contract.gates.find((gate) => gate.id === 'release-automation').policy.release_producer
  assert.equal(Object.hasOwn(producer, 'release_as'), false)
  assert.equal(Object.hasOwn(producer, 'baseline_version'), false)
  assert.equal(Object.hasOwn(producer, 'last_release_sha'), false)
  for (const [key, value] of [
    ['release_as', '0.9.0-beta.1'],
    ['baseline_version', '0.8.0'],
    ['last_release_sha', '568e7b42b78b3c9edb8ea390cb4297142a37e412'],
  ]) {
    const mutated = structuredClone(contract)
    mutated.gates.find((gate) => gate.id === 'release-automation').policy.release_producer[key] = value
    const result = validateContract(mutated)
    assert.equal(result.ok, false)
    assert.equal(result.errors.some((item) => item.rule === 'release-producer-policy-invalid'), true)
  }
})

test('release_producer_removes_one_shot_recovery_after_publication', () => {
  const config = JSON.parse(readFileSync(path.join(ROOT, 'release-please-config.json'), 'utf8'))
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const producer = contract.gates.find((gate) => gate.id === 'release-automation').policy.release_producer
  assert.equal(Object.hasOwn(config, 'release-as'), false)
  assert.equal(Object.hasOwn(config, 'last-release-sha'), false)
  assert.equal(Object.hasOwn(producer, 'release_as'), false)
  assert.equal(Object.hasOwn(producer, 'baseline_version'), false)
  assert.equal(Object.hasOwn(producer, 'last_release_sha'), false)
  for (const heading of ['Resumo', 'Commits', 'Issue', 'Responsavel', 'Labels', 'Validacao', 'Rollback trigger']) {
    assert.match(config['pull-request-header'], new RegExp(`^## ${heading}$`, 'm'))
  }
  assert.match(config['pull-request-header'], /machine-readable release notes/i)
})

test('publication_workflow_is_protected_manual_exact_sha_only', () => {
  const publicationPath = path.join(ROOT, '.github', 'workflows', 'release-publication.yml')
  assert.equal(existsSync(publicationPath), true)
  const workflow = readFileSync(publicationPath, 'utf8')
  assert.match(workflow, /^  workflow_dispatch:\s*$/m)
  assert.match(workflow, /environment: protected-release/)
  assert.match(workflow, /target tag, source SHA, and integrity run ID/)
  assert.match(workflow, /actions\/download-artifact@[0-9a-f]{40}/)
  assert.match(workflow, /permission-contents: write/)
  assert.match(workflow, /^      actions: read\s*$/m)
  assert.doesNotMatch(workflow, /permission-actions:/)
  assert.match(workflow, /node tools\/ci\/check-release-publication\.mjs/)
  assert.match(workflow, /ref: \$\{\{ github\.event\.repository\.default_branch \}\}/)
  assert.match(workflow, /ref: \$\{\{ env\.SOURCE_SHA \}\}/)
  assert.match(workflow, /refs\/tags\/\$RELEASE_TAG:refs\/tags\/\$RELEASE_TAG/)
  assert.match(workflow, /run-id: \$\{\{ env\.INTEGRITY_RUN_ID \}\}/)
  assert.match(workflow, /\.path == "\.github\/workflows\/release-integrity\.yml"/)
  assert.match(workflow, /\.event == "workflow_dispatch"/)
  assert.match(workflow, /\.event == "workflow_dispatch" and \.name == \$recovery_title/)
  assert.match(workflow, /\.display_title == \$recovery_title/)
  assert.match(workflow, /\.head_branch == "main"/)
  assert.match(workflow, /gh api --paginate --slurp/)
  assert.match(workflow, /release-tag-cardinality/)
  assert.match(workflow, /releases\/\$release_id/)
  assert.match(workflow, /gh api --method PATCH/)
  assert.match(workflow, /--field draft=false/)
  assert.match(workflow, /gh release upload "\$RELEASE_TAG" "artifacts\/release\/\$asset"/)
  assert.doesNotMatch(workflow, /gh release edit/)
  assert.doesNotMatch(workflow, /workflow_run|pull_request|^  push:|--clobber/i)
})

test('release_publication_request_delegates_only_to_exact_app_actor', () => {
  const publicationPath = path.join(ROOT, '.github', 'workflows', 'release-publication.yml')
  const workflow = readFileSync(publicationPath, 'utf8')
  assert.match(workflow, /^  repository_dispatch:\s*$/m)
  assert.match(workflow, /^    types:\s*\[release-publication-app\]\s*$/m)
  assert.match(workflow, /^  release-publication-admission:\s*$/m)
  assert.match(workflow, /case "\$AUTHORIZATION_STAGE" in/)
  assert.match(workflow, /\*\) exit 1 ;;/)
  assert.match(workflow, /test "\$DISPATCH_ACTOR" = 'emersonbusson-ramshared-release\[bot\]'/)
  assert.match(workflow, /^  release-publication-request:\s*$/m)
  assert.equal(workflow.match(/needs: release-publication-admission/g)?.length, 2)
  assert.match(workflow, /github\.event_name == 'workflow_dispatch'/)
  assert.match(workflow, /permission-contents: write/)
  assert.doesNotMatch(workflow, /permission-actions: write/)
  assert.match(workflow, /gh api --method POST "repos\/\$GITHUB_REPOSITORY\/dispatches"/)
  assert.match(workflow, /event_type=release-publication-app/)
  assert.match(workflow, /github\.event_name == 'repository_dispatch'/)
  assert.match(workflow, /github\.event\.action == 'release-publication-app'/)
  assert.match(workflow, /github\.actor == 'emersonbusson-ramshared-release\[bot\]'/)
  assert.equal(workflow.match(/environment: protected-release/g)?.length, 1)
})

test('release_promotion_node_coverage_is_wired_into_the_canonical_pr_caller', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci-contract.yml'), 'utf8')
  for (const module of [
    'tools/ci/merge-release-sboms.mjs',
    'tools/ci/check-release-integrity.mjs',
    'tools/ci/write-release-manifest.mjs',
    'tools/ci/check-release-publication.mjs',
  ]) {
    assert.match(workflow, new RegExp(`--test-coverage-include=${module.replace(/[./-]/g, '\\$&')}`))
  }
  assert.match(workflow, /--test-coverage-lines=80 --test-coverage-branches=80/)
  assert.match(workflow, /--test-coverage-functions=80/)
})

test('ci_contract_requires_fail_closed_trivy_sarif_publication', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'trivy')
  const workflow = readFileSync(path.join(ROOT, gate.workflow), 'utf8')

  assert.deepEqual(gate.allowed_continue_on_error_steps ?? [], [])
  assert.ok(gate.required_commands.includes('test -s trivy-results.sarif'))
  assert.ok(gate.required_commands.includes("jq -e '.version == \"2.1.0\" and (.runs | type == \"array\")' trivy-results.sarif"))
  assert.match(workflow, /name: Validate Trivy SARIF[\s\S]*test -s trivy-results\.sarif/)
  assert.match(workflow, /uses: github\/codeql-action\/upload-sarif@[0-9a-f]{40}[\s\S]*sarif_file: trivy-results\.sarif/)
  assert.doesNotMatch(workflow, /name: Upload Trivy SARIF[\s\S]{0,300}continue-on-error:\s*true/)
})

test('closed_pull_request_cancellation_has_minimum_permission', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'cancel-closed-pr.yml'), 'utf8')
  assert.match(workflow, /pull_request:\n\s+types: \[closed\]/)
  assert.match(workflow, /timeout-minutes: 10/)
  assert.match(workflow, /actions: write/)
  assert.match(workflow, /pull-requests: read/)
  assert.match(workflow, /github\.rest\.actions\.cancelWorkflowRun/)
  assert.match(workflow, /if: github\.event\.pull_request\.merged == true && github\.event\.pull_request\.head\.repo\.full_name == github\.repository/)
  assert.doesNotMatch(workflow, /pull_request_target|actions:\s*write[\s\S]*contents:\s*write/i)

  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'closed-pr-cancellation')
  assert.equal(gate.implementation, 'current')
  assert.deepEqual(gate.policy.permissions, { actions: 'write', 'pull-requests': 'read' })
  assert.deepEqual(gate.open_gaps, [])
})

test('closed_pull_request_cancellation_refuses_unscoped_run_selection', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'cancel-closed-pr.yml'), 'utf8')
    .replace("event: 'pull_request',", "event: 'push',")
    .replace('runIds.delete(context.runId);', '')
  const root = fixtureRoot(null, contract)
  writeFileSync(path.join(root, '.github', 'workflows', 'cancel-closed-pr.yml'), workflow)

  const result = validateWorkflowPolicy(contract, root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) =>
    item.gate === 'closed-pr-cancellation' && item.rule === 'closed-pr-cancellation-scope-mismatch'
  ), true)
})

test('remote_controls_missing_evidence_is_blocked', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'remote-controls')
  assert.equal(gate.implementation, 'observed')
  assert.deepEqual(gate.open_gaps, [])

  const root = fixtureRoot(null, contract)
  const result = validateWorkflowPolicy(contract, root)
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) =>
    item.gate === 'remote-controls' && item.rule === 'remote-control-observation-absent'
  ), true)
  assert.deepEqual(result.gaps, [])
})

test('remote_controls_unsafe_observation_is_no_go', () => {
  const observation = compliantRemoteObservation({
    actions: {
      default_workflow_permissions: 'write',
      can_approve_pull_request_reviews: true,
      allowed_actions: 'all',
      sha_pinning_required: false,
      artifact_and_log_retention_days: 90,
    },
    branch_protection: {
      strict: true,
      enforce_admins: true,
      required_conversation_resolution: false,
      required_status_checks: ['fmt + clippy + test'],
    },
    environments: {},
  })
  const result = validateRemoteControlObservation(observation, { now: REMOTE_OBSERVATION_NOW })
  assert.equal(result.ok, false)
  for (const rule of [
    'default-workflow-permissions-unsafe',
    'actions-pr-approval-enabled',
    'allowed-actions-unsafe',
    'sha-pinning-disabled',
    'retention-too-long',
    'conversation-resolution-disabled',
    'aggregate-context-missing',
    'environment-missing',
  ]) assert.equal(result.errors.some((item) => item.rule === rule), true, rule)
})

test('remote_controls_compliant_observation_is_accepted', () => {
  const result = validateRemoteControlObservation(compliantRemoteObservation(), { now: REMOTE_OBSERVATION_NOW })
  assert.deepEqual(result, { ok: true, errors: [] })
})

test('remote_controls_observed_state_closes_only_the_exact_remote_gate', () => {
  const accepted = currentOnlyContract(observedRemoteGate(), { contract_state: 'PASS' })
  assert.deepEqual(validateContract(accepted), { ok: true, errors: [] })

  for (const invalid of [
    observedRemoteGate({ id: 'another-gate' }),
    observedRemoteGate({ trust: 'pull-request' }),
    observedRemoteGate({ triggers: ['pull_request'] }),
    observedRemoteGate({ selection: { mode: 'always', paths: [] } }),
    observedRemoteGate({ observation: 'docs/governance/other.json' }),
    observedRemoteGate({ open_gaps: ['remote-control-observation-absent'] }),
  ]) {
    const rejected = validateContract(currentOnlyContract(invalid, { contract_state: 'PASS' }))
    assert.equal(rejected.ok, false)
    assert.equal(rejected.errors.some((item) => item.rule === 'observed-gate-invalid'), true)
  }
})

test('remote_controls_foreign_or_stale_observation_is_no_go', () => {
  const foreign = validateRemoteControlObservation(compliantRemoteObservation({
    repository: 'someone/else',
    default_branch: 'trunk',
  }), { now: REMOTE_OBSERVATION_NOW })
  assert.equal(foreign.errors.some((item) => item.rule === 'repository-mismatch'), true)
  assert.equal(foreign.errors.some((item) => item.rule === 'default-branch-mismatch'), true)

  const stale = validateRemoteControlObservation(compliantRemoteObservation({
    observed_at_utc: '2026-07-01T00:00:00Z',
  }), { now: REMOTE_OBSERVATION_NOW })
  assert.equal(stale.errors.some((item) => item.rule === 'observation-stale'), true)
})

test('remote_controls_malformed_future_and_partial_protection_are_no_go', () => {
  const malformed = validateRemoteControlObservation({})
  assert.deepEqual(malformed.errors.map((item) => item.rule), ['observation-schema-invalid'])

  const invalidTime = validateRemoteControlObservation(compliantRemoteObservation({
    observed_at_utc: 'invalid',
  }), { now: REMOTE_OBSERVATION_NOW })
  assert.equal(invalidTime.errors.some((item) => item.rule === 'observation-time-invalid'), true)

  const invalidClock = validateRemoteControlObservation(compliantRemoteObservation(), { now: Number.NaN })
  assert.equal(invalidClock.errors.some((item) => item.rule === 'observation-time-invalid'), true)

  const future = validateRemoteControlObservation(compliantRemoteObservation({
    observed_at_utc: '2026-08-09T17:00:00Z',
  }), { now: REMOTE_OBSERVATION_NOW })
  assert.equal(future.errors.some((item) => item.rule === 'observation-future'), true)

  const partialProtection = compliantRemoteObservation()
  partialProtection.branch_protection.strict = false
  partialProtection.branch_protection.enforce_admins = false
  partialProtection.environments['protected-release'].prevent_self_review = false
  const protectedResult = validateRemoteControlObservation(partialProtection, { now: REMOTE_OBSERVATION_NOW })
  for (const rule of ['branch-strict-disabled', 'admins-not-enforced', 'environment-protection-unsafe']) {
    assert.equal(protectedResult.errors.some((item) => item.rule === rule), true, rule)
  }
})

test('remote_controls_unreadable_evidence_is_no_go', () => {
  const denied = new Error('permission denied')
  denied.code = 'EACCES'
  const result = readRemoteControlObservation('/fixture', 'observation.json', {
    lstat: () => ({ isFile: () => true, isSymbolicLink: () => false }),
    read: () => { throw denied },
  })
  assert.deepEqual(result, {
    status: 'error',
    error: { gate: 'remote-controls', rule: 'observation-read-failed', detail: '' },
  })
})

test('remote_controls_broken_symlink_is_no_go', () => {
  const missingTarget = new Error('target missing')
  missingTarget.code = 'ENOENT'
  const result = readRemoteControlObservation('/fixture', 'observation.json', {
    lstat: () => ({ isFile: () => false, isSymbolicLink: () => true }),
    read: () => { throw missingTarget },
  })
  assert.equal(result.status, 'error')
  assert.equal(result.error.rule, 'observation-read-failed')
})

test('remote_controls_schema_matches_runtime_contract', () => {
  const schema = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'remote-controls-observation.schema.json'), 'utf8'))
  assert.deepEqual(validateRemoteControlSchemaDefinition(schema), { ok: true, errors: [] })
  assert.equal(validateRemoteControlObservation(compliantRemoteObservation({
    observed_at_utc: '2026-08-09T15:30:00.000Z',
  }), { now: REMOTE_OBSERVATION_NOW }).errors.some((item) => item.rule === 'observation-time-invalid'), true)
})

test('remote_controls_sensitive_control_name_is_refused', () => {
  const unsafeContext = compliantRemoteObservation()
  unsafeContext.branch_protection.required_status_checks.push('invalid|context')
  const contextResult = validateRemoteControlObservation(unsafeContext, { now: REMOTE_OBSERVATION_NOW })
  assert.deepEqual(contextResult.errors.map((item) => item.rule), ['observation-schema-invalid'])

  const unsafeEnvironment = compliantRemoteObservation()
  unsafeEnvironment.environments['secret-token-value'] = {
    required_reviewers: true,
    prevent_self_review: true,
    protected_branches: true,
  }
  const environmentResult = validateRemoteControlObservation(unsafeEnvironment, { now: REMOTE_OBSERVATION_NOW })
  assert.deepEqual(environmentResult.errors.map((item) => item.rule), ['observation-schema-invalid'])
})

test('run_fails_closed_for_missing_contract_and_premature_pass', () => {
  const missingRoot = mkdtempSync(path.join(tmpdir(), 'ramshared-ci-contract-missing-'))
  const missing = run({ root: missingRoot })
  assert.equal(missing.status, 'NO-GO')
  assert.equal(missing.errors[0].rule, 'contract-read-failed')

  const premature = contractFixture({ contract_state: 'PASS' })
  const root = fixtureRoot(validWorkflow(), premature)
  const result = run({ root })
  assert.equal(result.status, 'NO-GO')
  assert.equal(result.errors.some((item) => item.rule === 'premature-pass'), true)
})

test('main_diagnostics_do_not_echo_malformed_contract_detail', () => {
  const contract = contractFixture({
    p0_requirements: [{ id: 'policy', gate_ids: ['private-value'] }],
  })
  const root = fixtureRoot(validWorkflow(), contract)
  const output = []
  const errors = []
  assert.equal(main(['--check'], {
    root,
    print: (line) => output.push(line),
    error: (line) => errors.push(line),
  }), 1)
  assert.equal(output.includes('CI_CONTRACT_VERDICT=NO-GO'), true)
  assert.equal(errors.some((line) => line.includes('CI_CONTRACT_ERROR=policy:p0-gate-missing')), true)
  assert.equal(errors.join('\n').includes('private-value'), false)
})

test('main_returns_zero_after_required_p0_items_exist', () => {
  assert.equal(main(['--check'], { root: ROOT, print: () => {}, error: () => {} }), 0)
  assert.equal(main(['--unknown'], { root: ROOT, print: () => {}, error: () => {} }), 2)
})

test('ci_contract_requires_serial_rust_test_execution', () => {
  const workflow = readFileSync(path.join(ROOT, '.github', 'workflows', 'ci.yml'), 'utf8')
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const gate = contract.gates.find((item) => item.id === 'rust-quality')
  const command = 'cargo test --workspace -- --test-threads=1'
  assert.equal(gate.required_commands.includes(command), true)
  assert.match(workflow, /^\s*run:\s*cargo test --workspace -- --test-threads=1\s*$/m)
})

test('canonical_reusable_workflow_rejects_undeclared_direct_trigger', () => {
  const gate = currentGate({ triggers: ['workflow_call'] })
  const canonical = validWorkflow().replace('  pull_request:', '  workflow_call:')
  const canonicalResult = validateWorkflowPolicy(currentOnlyContract(gate), fixtureRoot(canonical))
  assert.deepEqual(canonicalResult.errors, [])

  const duplicate = canonical.replace('  workflow_call:', '  workflow_call:\n  pull_request:')
  const duplicateResult = validateWorkflowPolicy(currentOnlyContract(gate), fixtureRoot(duplicate))
  assert.equal(duplicateResult.errors.some((item) => item.rule === 'undeclared-direct-trigger'), true)
})

test('aggregate_cli_accepts_complete_same_run_results_and_rejects_invalid_json', () => {
  const contract = JSON.parse(readFileSync(path.join(ROOT, 'docs', 'governance', 'ci-contract.json'), 'utf8'))
  const needs = Object.fromEntries(contract.aggregate.architecture.callers.map((caller) => [caller.job, { result: 'success' }]))
  const priorEvent = process.env.GITHUB_EVENT_NAME
  process.env.GITHUB_EVENT_NAME = 'pull_request'
  try {
    const output = []
    assert.equal(main(['--aggregate-needs', JSON.stringify(needs)], {
      root: ROOT,
      print: (line) => output.push(line),
      error: () => {},
    }), 0)
    assert.equal(output.includes('CI_AGGREGATE_VERDICT=PASS'), true)
    assert.equal(main(['--aggregate-needs', '{invalid'], { root: ROOT, print: () => {}, error: () => {} }), 2)
  } finally {
    if (priorEvent === undefined) delete process.env.GITHUB_EVENT_NAME
    else process.env.GITHUB_EVENT_NAME = priorEvent
  }
})
