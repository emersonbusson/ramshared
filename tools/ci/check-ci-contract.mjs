#!/usr/bin/env node
import { existsSync, lstatSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const CONTRACT_PATH = 'docs/governance/ci-contract.json'
const REMOTE_CONTROLS_OBSERVATION_PATH = 'docs/governance/remote-controls-observation.json'
const REMOTE_CONTROLS_SCHEMA_PATH = 'docs/governance/remote-controls-observation.schema.json'
const REMOTE_CONTROLS_REPOSITORY = 'emersonbusson/ramshared'
const REMOTE_CONTROLS_DEFAULT_BRANCH = 'main'
const REMOTE_CONTROLS_MAX_AGE_DAYS = 30
const REMOTE_CONTROLS_REQUIRED_CONTEXT = 'required-checks'
const REMOTE_CONTROLS_ENVIRONMENTS = ['protected-isolated-lab', 'protected-release']
const REMOTE_CONTROLS_UTC_PATTERN = '^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}Z$'
const REMOTE_CONTROLS_PUBLIC_NAME_PATTERN = '^[A-Za-z0-9][A-Za-z0-9 ._+:/()\\-]{0,127}$'
const REMOTE_CONTROLS_PUBLIC_NAME_REGEX = new RegExp(REMOTE_CONTROLS_PUBLIC_NAME_PATTERN)
const IMPLEMENTATION_STATES = new Set(['current', 'planned', 'env-bound', 'observed'])
const TRUST_LEVELS = new Set(['pull-request', 'isolated-lab', 'release', 'remote', 'maintenance'])
const SELECTION_MODES = new Set(['always', 'paths', 'never'])
const RETRY_CLASSES = new Set(['none', 'dependency-fetch-transport'])
const RUSTSEC_ADVISORY_DB_URL = 'https://github.com/RustSec/advisory-db.git'
const RUSTSEC_SNAPSHOT_MAX_AGE_DAYS = 7
const MILLISECONDS_PER_DAY = 24 * 60 * 60 * 1000
const LAB_PLAN_KINDS = new Set(['windows', 'wsl2'])
const LAB_PLAN_MODE = 'plan'
const LAB_PLAN_TARGET = 'isolated-lab'
const LAB_PLAN_ENVIRONMENT = 'protected-isolated-lab'
const RELEASE_ENVIRONMENT = 'protected-release'
const RELEASE_SBOM_GENERATOR = { name: 'cargo-cyclonedx', version: '0.5.9', spec_version: '1.5' }
const RELEASE_PROMOTION_POLICY = 'docs/governance/release-promotion.json'
const RELEASE_TARGET_TAG = 'v0.9.0-beta.1'
const RELEASE_BASELINE_VERSION = '0.8.0'
const RELEASE_INTEGRITY_ARTIFACT_RETENTION_DAYS = 14
const LOCAL_REUSABLE_AGGREGATE_KIND = 'local-reusable-needs-v1'
const RUST_SLICE_COVERAGE_MAP = 'docs/governance/rust-slice-coverage.json'
const RUST_SLICE_COVERAGE_PLANNER = 'tools/ci/plan-rust-slice-coverage.mjs'
const RUST_LLVM_COV_VERSION = '0.8.7'
const CLOSED_PR_CANCELLATION_CONDITION = "github.event.pull_request.merged == true && github.event.pull_request.head.repo.full_name == github.repository"
const GAP_RULES = new Set([
  'aggregate-absent',
  'mutable-action-reference',
  'overbroad-permissions',
  'permissions-not-explicit',
  'permissions-not-job-scoped',
  'remote-control-observation-absent',
  'required-command-absent',
  'timeout-not-explicit',
  'workflow-absent',
])
const TERMINAL_STATES = new Set(['PASS', 'FAIL', 'NO_CHANGE', 'BLOCKED', 'CANCELLED', 'SKIPPED'])

function finding(gate, rule, detail = '') {
  return { gate, rule, detail }
}

function sortFindings(items) {
  return [...items].sort((left, right) =>
    left.gate.localeCompare(right.gate) || left.rule.localeCompare(right.rule) || left.detail.localeCompare(right.detail)
  )
}

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function parseUtcTimestamp(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) return null
  const timestamp = Date.parse(value)
  return Number.isNaN(timestamp) || new Date(timestamp).toISOString().replace('.000Z', 'Z') !== value ? null : timestamp
}

function hasExactKeys(value, expected) {
  if (!isObject(value)) return false
  const actual = Object.keys(value).sort()
  return actual.length === expected.length && actual.every((key, index) => key === [...expected].sort()[index])
}

export function readRemoteControlObservation(root, relativePath, { lstat = lstatSync, read = readFileSync } = {}) {
  const observationPath = path.join(root, relativePath)
  try {
    const stat = lstat(observationPath)
    if (!stat.isFile() || stat.isSymbolicLink()) {
      return { status: 'error', error: finding('remote-controls', 'observation-read-failed') }
    }
  } catch (error) {
    if (error?.code === 'ENOENT') return { status: 'missing' }
    return { status: 'error', error: finding('remote-controls', 'observation-read-failed') }
  }
  try {
    return { status: 'present', observation: JSON.parse(read(observationPath, 'utf8')) }
  } catch {
    return { status: 'error', error: finding('remote-controls', 'observation-read-failed') }
  }
}

export function validateRemoteControlSchemaDefinition(schema) {
  const checks = schema?.properties?.branch_protection?.properties?.required_status_checks?.items
  const environmentContainer = schema?.properties?.environments
  const environmentDefinition = schema?.$defs?.environment
  const environments = environmentContainer?.properties
  const environmentReferencesValid = isObject(environments) && hasExactKeys(environments, REMOTE_CONTROLS_ENVIRONMENTS) &&
    Object.values(environments).every((item) => item?.$ref === '#/$defs/environment')
  const valid = isObject(schema) && schema.additionalProperties === false &&
    schema?.properties?.observed_at_utc?.pattern === REMOTE_CONTROLS_UTC_PATTERN &&
    checks?.type === 'string' && checks.maxLength === 128 && checks.pattern === REMOTE_CONTROLS_PUBLIC_NAME_PATTERN &&
    environmentContainer?.additionalProperties === false && environmentReferencesValid &&
    environmentDefinition?.additionalProperties === false &&
    hasExactKeys(environmentDefinition?.properties, ['prevent_self_review', 'protected_branches', 'required_reviewers'])
  return valid
    ? { ok: true, errors: [] }
    : { ok: false, errors: [finding('remote-controls', 'observation-schema-definition-invalid')] }
}

export function validateRemoteControlObservation(observation, { now = Date.now() } = {}) {
  const errors = []
  const topKeys = ['actions', 'branch_protection', 'default_branch', 'environments', 'observed_at_utc', 'repository', 'schema_version', 'source']
  const actionKeys = ['allowed_actions', 'artifact_and_log_retention_days', 'can_approve_pull_request_reviews', 'default_workflow_permissions', 'sha_pinning_required']
  const branchKeys = ['enforce_admins', 'required_conversation_resolution', 'required_status_checks', 'strict']
  const environmentKeys = ['prevent_self_review', 'protected_branches', 'required_reviewers']
  const actions = observation?.actions
  const branch = observation?.branch_protection
  const environments = observation?.environments
  const statusChecks = branch?.required_status_checks
  const environmentShapeValid = isObject(environments) && Object.values(environments).every((item) =>
    hasExactKeys(item, environmentKeys) && Object.values(item).every((value) => typeof value === 'boolean')) &&
    Object.keys(environments).every((name) => REMOTE_CONTROLS_ENVIRONMENTS.includes(name))
  const schemaValid = hasExactKeys(observation, topKeys) && observation.schema_version === 1 &&
    typeof observation.repository === 'string' && typeof observation.default_branch === 'string' &&
    typeof observation.observed_at_utc === 'string' && observation.source === 'github-rest-api' &&
    hasExactKeys(actions, actionKeys) && ['read', 'write'].includes(actions.default_workflow_permissions) &&
    typeof actions.can_approve_pull_request_reviews === 'boolean' && ['all', 'local_only', 'selected'].includes(actions.allowed_actions) &&
    typeof actions.sha_pinning_required === 'boolean' && Number.isInteger(actions.artifact_and_log_retention_days) &&
    actions.artifact_and_log_retention_days >= 1 &&
    hasExactKeys(branch, branchKeys) && typeof branch.strict === 'boolean' && typeof branch.enforce_admins === 'boolean' &&
    typeof branch.required_conversation_resolution === 'boolean' && Array.isArray(statusChecks) && statusChecks.length > 0 &&
    statusChecks.every((item) => typeof item === 'string' && REMOTE_CONTROLS_PUBLIC_NAME_REGEX.test(item)) &&
    new Set(statusChecks).size === statusChecks.length &&
    environmentShapeValid
  if (!schemaValid) return { ok: false, errors: [finding('remote-controls', 'observation-schema-invalid')] }

  if (observation.repository !== REMOTE_CONTROLS_REPOSITORY) errors.push(finding('remote-controls', 'repository-mismatch'))
  if (observation.default_branch !== REMOTE_CONTROLS_DEFAULT_BRANCH) errors.push(finding('remote-controls', 'default-branch-mismatch'))
  const observedAt = parseUtcTimestamp(observation.observed_at_utc)
  if (observedAt === null) errors.push(finding('remote-controls', 'observation-time-invalid'))
  else if (!Number.isFinite(now)) errors.push(finding('remote-controls', 'observation-time-invalid'))
  else if (observedAt > now) errors.push(finding('remote-controls', 'observation-future'))
  else if (now - observedAt > REMOTE_CONTROLS_MAX_AGE_DAYS * MILLISECONDS_PER_DAY) errors.push(finding('remote-controls', 'observation-stale'))

  if (actions.default_workflow_permissions !== 'read') errors.push(finding('remote-controls', 'default-workflow-permissions-unsafe'))
  if (actions.can_approve_pull_request_reviews !== false) errors.push(finding('remote-controls', 'actions-pr-approval-enabled'))
  if (actions.allowed_actions !== 'selected') errors.push(finding('remote-controls', 'allowed-actions-unsafe'))
  if (actions.sha_pinning_required !== true) errors.push(finding('remote-controls', 'sha-pinning-disabled'))
  if (actions.artifact_and_log_retention_days > 30) errors.push(finding('remote-controls', 'retention-too-long'))
  if (branch.strict !== true) errors.push(finding('remote-controls', 'branch-strict-disabled'))
  if (branch.enforce_admins !== true) errors.push(finding('remote-controls', 'admins-not-enforced'))
  if (branch.required_conversation_resolution !== true) errors.push(finding('remote-controls', 'conversation-resolution-disabled'))
  if (!statusChecks.includes(REMOTE_CONTROLS_REQUIRED_CONTEXT)) errors.push(finding('remote-controls', 'aggregate-context-missing'))
  for (const name of REMOTE_CONTROLS_ENVIRONMENTS) {
    const environment = environments[name]
    if (!environment) errors.push(finding('remote-controls', 'environment-missing'))
    else if (environment.required_reviewers !== true || environment.prevent_self_review !== true || environment.protected_branches !== true) {
      errors.push(finding('remote-controls', 'environment-protection-unsafe'))
    }
  }
  return { ok: errors.length === 0, errors: sortFindings(errors) }
}

function validateAdvisoryDatabase(gate, policy, errors, now) {
  const requiresSnapshot = gate.id === 'cargo-audit' && gate.implementation === 'current'
  const advisory = policy.advisory_db
  if (advisory === undefined) {
    if (requiresSnapshot) errors.push(finding(gate.id, 'advisory-db-metadata-missing'))
    return
  }
  if (!isObject(advisory) || advisory.url !== RUSTSEC_ADVISORY_DB_URL ||
      typeof advisory.commit !== 'string' || !/^[0-9a-f]{40}$/.test(advisory.commit) ||
      !Number.isInteger(advisory.max_age_days) || advisory.max_age_days < 1 || advisory.max_age_days > RUSTSEC_SNAPSHOT_MAX_AGE_DAYS ||
      !isObject(advisory.upstream_head_health) || advisory.upstream_head_health.mode !== 'scheduled-curator' ||
      advisory.upstream_head_health.cannot_override_snapshot_age !== true) {
    errors.push(finding(gate.id, 'advisory-db-metadata-invalid'))
    return
  }
  const timestamp = parseUtcTimestamp(advisory.commit_utc)
  if (timestamp === null) {
    errors.push(finding(gate.id, 'advisory-db-commit-utc-invalid'))
    return
  }
  if (timestamp > now || now - timestamp > advisory.max_age_days * MILLISECONDS_PER_DAY) {
    errors.push(finding(gate.id, 'advisory-db-snapshot-stale'))
  }
}

function validateLabPlanPolicy(gate, policy, errors) {
  const requiresPlanPolicy = gate.trust === 'isolated-lab' && gate.implementation === 'current'
  const labPlan = policy.lab_plan
  if (labPlan === undefined) {
    if (requiresPlanPolicy) errors.push(finding(gate.id, 'lab-plan-policy-missing'))
    return
  }
  if (!isObject(labPlan) || !LAB_PLAN_KINDS.has(labPlan.kind) ||
      labPlan.mode !== LAB_PLAN_MODE || labPlan.target !== LAB_PLAN_TARGET ||
      labPlan.environment !== LAB_PLAN_ENVIRONMENT || labPlan.host_action !== 'none') {
    errors.push(finding(gate.id, 'lab-plan-policy-invalid'))
  }
}

function validateReleaseProducerPolicy(gate, policy, errors) {
  const required = gate.id === 'release-automation' && gate.implementation === 'current'
  const producer = policy.release_producer
  if (producer === undefined) {
    if (required) errors.push(finding(gate.id, 'release-producer-policy-missing'))
    return
  }
  if (!isObject(producer) || producer.target_tag !== RELEASE_TARGET_TAG ||
      producer.credential !== 'github-app-required' || producer.release_config !== 'release-please-config.json' ||
      producer.draft !== true || producer.prerelease !== true || producer.force_tag_creation !== true ||
      producer.skip_github_release !== false || producer.release_as !== RELEASE_TARGET_TAG.slice(1) ||
      producer.baseline_version !== RELEASE_BASELINE_VERSION) {
    errors.push(finding(gate.id, 'release-producer-policy-invalid'))
  }
}

function validateReleaseIntegrityPolicy(gate, policy, errors) {
  const requiresReleasePolicy = gate.id === 'release-integrity' && gate.implementation === 'current'
  const release = policy.release_integrity
  if (release === undefined) {
    if (requiresReleasePolicy) errors.push(finding(gate.id, 'release-integrity-policy-missing'))
    return
  }
  if (!isObject(release) || release.environment !== RELEASE_ENVIRONMENT ||
      !isObject(release.sbom_generator) || release.sbom_generator.name !== RELEASE_SBOM_GENERATOR.name ||
      release.sbom_generator.version !== RELEASE_SBOM_GENERATOR.version ||
      release.sbom_generator.spec_version !== RELEASE_SBOM_GENERATOR.spec_version ||
      release.publication !== 'forbidden' || release.promotion_policy !== RELEASE_PROMOTION_POLICY ||
      release.target_tag !== RELEASE_TARGET_TAG ||
      release.integrity_artifact_retention_days !== RELEASE_INTEGRITY_ARTIFACT_RETENTION_DAYS) {
    errors.push(finding(gate.id, 'release-integrity-policy-invalid'))
  }
}

function validateReleasePublicationPolicy(gate, policy, errors) {
  const required = gate.id === 'release-publication' && gate.implementation === 'current'
  const publication = policy.release_publication
  if (publication === undefined) {
    if (required) errors.push(finding(gate.id, 'release-publication-policy-missing'))
    return
  }
  if (!isObject(publication) || publication.environment !== RELEASE_ENVIRONMENT ||
      publication.promotion_policy !== RELEASE_PROMOTION_POLICY || publication.target_tag !== RELEASE_TARGET_TAG ||
      publication.manual_only !== true || publication.credential !== 'github-app-required' ||
      publication.draft_only !== true) {
    errors.push(finding(gate.id, 'release-publication-policy-invalid'))
  }
}

function validateRustSliceCoveragePolicy(gate, policy, errors) {
  const required = gate.id === 'rust-slice-coverage' && gate.implementation === 'current'
  const coverage = policy.rust_slice_coverage
  if (coverage === undefined) {
    if (required) errors.push(finding(gate.id, 'rust-slice-coverage-policy-missing'))
    return
  }
  if (!isObject(coverage) || coverage.map !== RUST_SLICE_COVERAGE_MAP || coverage.planner !== RUST_SLICE_COVERAGE_PLANNER ||
      coverage.llvm_cov_version !== RUST_LLVM_COV_VERSION || coverage.min !== 80) {
    errors.push(finding(gate.id, 'rust-slice-coverage-policy-invalid'))
  }
}

function validateClosedPrCancellationPolicy(gate, policy, errors) {
  const required = gate.id === 'closed-pr-cancellation' && gate.implementation === 'current'
  const cancellation = policy.closed_pr_cancellation
  if (cancellation === undefined) {
    if (required) errors.push(finding(gate.id, 'closed-pr-cancellation-policy-missing'))
    return
  }
  if (!isObject(cancellation) || cancellation.merged_only !== true || cancellation.same_repository_only !== true) {
    errors.push(finding(gate.id, 'closed-pr-cancellation-policy-invalid'))
  }
}

function validatePolicy(gate, errors, now) {
  if (!isObject(gate.policy)) {
    errors.push(finding(gate.id ?? 'unknown', 'policy-missing'))
    return
  }
  const policy = gate.policy
  if (!Number.isInteger(policy.timeout_minutes) || policy.timeout_minutes < 1 || policy.timeout_minutes > 60) {
    errors.push(finding(gate.id, 'timeout-policy-invalid'))
  }
  if (!isObject(policy.permissions) || Object.entries(policy.permissions).some(([, value]) => !['read', 'write', 'none'].includes(value))) {
    errors.push(finding(gate.id, 'permissions-policy-invalid'))
  }
  if (!['workflow', 'job'].includes(policy.permissions_scope)) errors.push(finding(gate.id, 'permissions-scope-invalid'))
  if (policy.action_pinning !== 'full-sha') errors.push(finding(gate.id, 'action-pinning-invalid'))
  if (policy.continue_on_error !== false) errors.push(finding(gate.id, 'continue-on-error-policy-invalid'))
  if (!RETRY_CLASSES.has(policy.retry_class)) errors.push(finding(gate.id, 'retry-policy-invalid'))
  if (gate.implementation === 'current' && policy.concurrency === undefined) {
    errors.push(finding(gate.id, 'concurrency-policy-missing'))
  } else if (policy.concurrency !== undefined && (!isObject(policy.concurrency) || typeof policy.concurrency.cancel_in_progress !== 'boolean')) {
    errors.push(finding(gate.id, 'concurrency-policy-invalid'))
  }
  validateAdvisoryDatabase(gate, policy, errors, now)
  validateLabPlanPolicy(gate, policy, errors)
  validateReleaseProducerPolicy(gate, policy, errors)
  validateReleaseIntegrityPolicy(gate, policy, errors)
  validateReleasePublicationPolicy(gate, policy, errors)
  validateRustSliceCoveragePolicy(gate, policy, errors)
  validateClosedPrCancellationPolicy(gate, policy, errors)
}

function validateSelection(gate, errors) {
  const selection = gate.selection
  if (!isObject(selection) || !SELECTION_MODES.has(selection.mode) || !Array.isArray(selection.paths)) {
    errors.push(finding(gate.id ?? 'unknown', 'selection-invalid'))
    return
  }
  if (selection.mode === 'paths' && selection.paths.length === 0) errors.push(finding(gate.id, 'selection-paths-empty'))
  if (selection.paths.some((item) => !safeRelative(item))) errors.push(finding(gate.id, 'selection-path-unsafe'))
}

function validateGate(gate, errors, now) {
  if (!isObject(gate) || typeof gate.id !== 'string' || !gate.id) {
    errors.push(finding('unknown', 'gate-id-invalid'))
    return
  }
  if (typeof gate.required !== 'boolean') errors.push(finding(gate.id, 'required-invalid'))
  if (!IMPLEMENTATION_STATES.has(gate.implementation)) errors.push(finding(gate.id, 'implementation-invalid'))
  if (typeof gate.context !== 'string' || !gate.context) errors.push(finding(gate.id, 'context-invalid'))
  if (!TRUST_LEVELS.has(gate.trust)) errors.push(finding(gate.id, 'trust-invalid'))
  if (!Array.isArray(gate.triggers) || gate.triggers.some((item) => typeof item !== 'string')) errors.push(finding(gate.id, 'triggers-invalid'))
  validateSelection(gate, errors)
  const openGaps = Array.isArray(gate.open_gaps) ? gate.open_gaps : []
  if (!Array.isArray(gate.open_gaps) || openGaps.some((item) => !GAP_RULES.has(item))) errors.push(finding(gate.id, 'open-gaps-invalid'))
  else if (new Set(openGaps).size !== openGaps.length) errors.push(finding(gate.id, 'open-gaps-duplicate'))

  if (gate.implementation === 'env-bound') {
    if (!openGaps.includes('remote-control-observation-absent')) errors.push(finding(gate.id, 'env-bound-gap-missing'))
    if (gate.id === 'remote-controls' && gate.observation !== REMOTE_CONTROLS_OBSERVATION_PATH) {
      errors.push(finding(gate.id, 'observation-path-invalid'))
    }
    return
  }

  if (gate.implementation === 'observed') {
    const ownsLocalExecution = gate.workflow !== undefined || gate.job !== undefined ||
      gate.policy !== undefined || gate.required_commands !== undefined
    if (gate.id !== 'remote-controls' || gate.required !== true || gate.trust !== 'remote' ||
        gate.triggers.length !== 0 || gate.selection?.mode !== 'never' || gate.selection.paths.length !== 0 ||
        gate.observation !== REMOTE_CONTROLS_OBSERVATION_PATH || openGaps.length !== 0 || ownsLocalExecution) {
      errors.push(finding(gate.id, 'observed-gate-invalid'))
    }
    return
  }

  if (!safeRelative(gate.workflow)) errors.push(finding(gate.id, 'workflow-path-invalid'))
  if (typeof gate.job !== 'string' || !gate.job) errors.push(finding(gate.id, 'job-invalid'))
  validatePolicy(gate, errors, now)
  if (!Array.isArray(gate.required_commands) || gate.required_commands.some((item) => typeof item !== 'string' || !item)) {
    errors.push(finding(gate.id, 'required-commands-invalid'))
  }
  if (gate.allowed_continue_on_error_steps !== undefined && (!Array.isArray(gate.allowed_continue_on_error_steps) || gate.allowed_continue_on_error_steps.some((item) => typeof item !== 'string' || !item))) {
    errors.push(finding(gate.id, 'continue-on-error-allowlist-invalid'))
  }
}

function validJobId(value) {
  return typeof value === 'string' && /^[A-Za-z][A-Za-z0-9_-]*$/.test(value)
}

function validateAggregateModel(aggregate, gates, errors) {
  if (!isObject(aggregate) || aggregate.implementation !== 'current') return
  const architecture = aggregate.architecture
  if (!isObject(architecture) || architecture.kind !== LOCAL_REUSABLE_AGGREGATE_KIND || !Array.isArray(architecture.callers) || architecture.callers.length === 0) {
    errors.push(finding('aggregate', 'aggregate-architecture-invalid'))
    return
  }
  const currentPullRequestGates = gates.filter((gate) =>
    gate?.implementation === 'current' && gate.trust === 'pull-request' && gate.id !== aggregate.id)
  const expectedGateIds = new Set(currentPullRequestGates.map((gate) => gate.id))
  const seenGateIds = new Set()
  const seenCallerJobs = new Set()
  for (const caller of architecture.callers) {
    if (!isObject(caller) || !validJobId(caller.job) || !['direct', 'reusable'].includes(caller.kind) ||
        !Array.isArray(caller.gates) || caller.gates.length === 0 || caller.gates.some((id) => typeof id !== 'string')) {
      errors.push(finding('aggregate', 'aggregate-caller-invalid'))
      continue
    }
    if (seenCallerJobs.has(caller.job)) errors.push(finding('aggregate', 'aggregate-caller-duplicate', caller.job))
    seenCallerJobs.add(caller.job)
    if (caller.kind === 'direct') {
      if (caller.gates.length !== 1) errors.push(finding('aggregate', 'aggregate-direct-caller-invalid', caller.job))
      const gate = gates.find((item) => item.id === caller.gates[0])
      if (!gate || gate.workflow !== aggregate.workflow || gate.job !== caller.job) {
        errors.push(finding('aggregate', 'aggregate-direct-caller-invalid', caller.job))
      }
    } else if (!safeRelative(caller.workflow) || !validJobId(caller.summary_job) ||
        !Array.isArray(caller.summary_needs) || caller.summary_needs.length === 0 || caller.summary_needs.some((job) => !validJobId(job))) {
      errors.push(finding('aggregate', 'aggregate-reusable-caller-invalid', caller.job))
    }
    for (const gateId of caller.gates) {
      if (!expectedGateIds.has(gateId)) errors.push(finding('aggregate', 'aggregate-caller-gate-invalid', gateId))
      if (seenGateIds.has(gateId)) errors.push(finding('aggregate', 'aggregate-caller-gate-duplicate', gateId))
      seenGateIds.add(gateId)
    }
  }
  for (const gateId of expectedGateIds) if (!seenGateIds.has(gateId)) errors.push(finding('aggregate', 'aggregate-caller-gate-missing', gateId))
}

export function validateContract(contract, { now = Date.now() } = {}) {
  const errors = []
  if (!isObject(contract) || contract.schema_version !== 1) return { ok: false, errors: [finding('contract', 'schema-invalid')] }
  if (!['PASS', 'PARTIAL'].includes(contract.contract_state)) errors.push(finding('contract', 'state-invalid'))
  const gates = Array.isArray(contract.gates) ? contract.gates : []
  if (!Array.isArray(contract.gates) || gates.length === 0) errors.push(finding('contract', 'gates-invalid'))

  const ids = new Set()
  for (const gate of gates) {
    validateGate(gate, errors, now)
    if (gate?.id) {
      if (ids.has(gate.id)) errors.push(finding(gate.id, 'gate-id-duplicate'))
      ids.add(gate.id)
    }
  }

  const p0Requirements = Array.isArray(contract.p0_requirements) ? contract.p0_requirements : []
  if (!Array.isArray(contract.p0_requirements) || p0Requirements.length === 0) {
    errors.push(finding('contract', 'p0-requirements-invalid'))
  } else {
    const p0Ids = new Set()
    for (const item of p0Requirements) {
      if (!isObject(item) || typeof item.id !== 'string' || !item.id || !Array.isArray(item.gate_ids) || item.gate_ids.length === 0) {
        errors.push(finding('contract', 'p0-requirement-invalid'))
        continue
      }
      if (p0Ids.has(item.id)) errors.push(finding('contract', 'p0-requirement-duplicate', item.id))
      p0Ids.add(item.id)
      for (const gateId of item.gate_ids) if (!ids.has(gateId)) errors.push(finding(item.id, 'p0-gate-missing', gateId))
    }
  }

  const aggregate = contract.aggregate
  if (!isObject(aggregate) || typeof aggregate.id !== 'string' || !aggregate.id || !safeRelative(aggregate.workflow) ||
      typeof aggregate.job !== 'string' || !aggregate.job || !IMPLEMENTATION_STATES.has(aggregate.implementation) ||
      !Array.isArray(aggregate.open_gaps) || aggregate.open_gaps.some((item) => !GAP_RULES.has(item))) {
    errors.push(finding('aggregate', 'aggregate-invalid'))
  }

  validateAggregateModel(aggregate, gates, errors)

  if (contract.contract_state === 'PASS' && gates.some((gate) =>
    gate.required && !['current', 'observed'].includes(gate.implementation))) {
    errors.push(finding('contract', 'premature-pass'))
  }
  return { ok: errors.length === 0, errors: sortFindings(errors) }
}

function topLevelBlock(lines, key) {
  const start = lines.findIndex((line) => new RegExp(`^${key}:\\s*(?:#.*)?$`).test(line))
  if (start === -1) return null
  const block = []
  for (let index = start + 1; index < lines.length; index++) {
    if (/^\S/.test(lines[index])) break
    block.push(lines[index])
  }
  return block
}

function jobBlock(text, job) {
  const lines = text.split(/\r?\n/)
  const jobs = topLevelBlock(lines, 'jobs')
  if (jobs === null) return null
  const jobPattern = new RegExp(`^  ${job.replace(/[.*+?^${}()|[\\]\\]/g, '\\$&')}:\\s*(?:#.*)?$`)
  const start = jobs.findIndex((line) => jobPattern.test(line))
  if (start === -1) return null
  const block = []
  for (let index = start + 1; index < jobs.length; index++) {
    if (/^  [A-Za-z][A-Za-z0-9_-]*:\s*(?:#.*)?$/.test(jobs[index])) break
    block.push(jobs[index])
  }
  return block
}

function jobIds(text) {
  const jobs = topLevelBlock(text.split(/\r?\n/), 'jobs')
  if (jobs === null) return []
  return jobs
    .map((line) => line.match(/^  ([A-Za-z][A-Za-z0-9_-]*):\s*(?:#.*)?$/))
    .filter(Boolean)
    .map((match) => match[1])
}

function fieldValue(lines, field) {
  const expression = new RegExp(`^    ${field}:\\s*(.+?)\\s*$`)
  const line = lines.find((item) => expression.test(item))
  return line ? line.match(expression)[1] : null
}

function jobList(lines, field) {
  const inline = lines.find((line) => new RegExp(`^    ${field}:\\s*\\[(.*)\\]\\s*$`).test(line))
  if (inline) {
    const values = inline.match(/\[(.*)\]/)[1].split(',').map((item) => item.trim()).filter(Boolean)
    return values.every(validJobId) ? values : null
  }
  const start = lines.findIndex((line) => new RegExp(`^    ${field}:\\s*$`).test(line))
  if (start === -1) return null
  const values = []
  for (let index = start + 1; index < lines.length; index++) {
    const line = lines[index]
    if (/^    [A-Za-z][A-Za-z0-9_-]*:\s*/.test(line)) break
    if (line.trim() === '') continue
    const match = line.match(/^      -\s+([A-Za-z][A-Za-z0-9_-]*)\s*$/)
    if (!match) return null
    values.push(match[1])
  }
  return values.length > 0 ? values : null
}

function sameStringSet(left, right) {
  return Array.isArray(left) && Array.isArray(right) && left.length === right.length &&
    [...left].sort().every((item, index) => item === [...right].sort()[index])
}

function callerPermissions(contract, caller) {
  const permissions = {}
  for (const gateId of caller.gates) {
    const gate = contract.gates.find((item) => item.id === gateId)
    for (const [name, level] of Object.entries(gate?.policy?.permissions ?? {})) {
      if (level === 'write' || permissions[name] === undefined) permissions[name] = level
    }
  }
  return permissions
}

function samePermissions(left, right) {
  const leftEntries = Object.entries(left ?? {}).sort(([a], [b]) => a.localeCompare(b))
  const rightEntries = Object.entries(right ?? {}).sort(([a], [b]) => a.localeCompare(b))
  return JSON.stringify(leftEntries) === JSON.stringify(rightEntries)
}

function permissionsBlock(lines, indent) {
  const start = lines.findIndex((line) => new RegExp(`^${' '.repeat(indent)}permissions:\\s*$`).test(line))
  if (start === -1) return null
  const permissions = {}
  const entry = new RegExp(`^${' '.repeat(indent + 2)}([A-Za-z-]+):\\s*(read|write|none)\\s*$`)
  for (let index = start + 1; index < lines.length; index++) {
    if (lines[index].trim() === '') continue
    const match = lines[index].match(entry)
    if (!match) break
    permissions[match[1]] = match[2]
  }
  return permissions
}

function workflowTriggerNames(text) {
  const block = topLevelBlock(text.split(/\r?\n/), 'on')
  if (block === null) return []
  return block
    .map((line) => line.match(/^  ([A-Za-z][A-Za-z0-9_-]*):(?:\s|$)/))
    .filter(Boolean)
    .map((match) => match[1])
}

function hasTrigger(text, trigger) {
  if (trigger === 'pull_request') return /^\s*pull_request:\s*(?:#.*)?$/m.test(text)
  if (trigger === 'pull_request-closed') return /^\s*pull_request:\s*$/m.test(text) && /^\s*types:\s*\[closed\]\s*$/m.test(text)
  if (trigger === 'push-main') return /^\s*push:\s*(?:#.*)?$/m.test(text) && /^\s*branches:\s*\[main\]\s*$/m.test(text)
  if (trigger === 'workflow_dispatch') return /^\s*workflow_dispatch:\s*(?:#.*)?$/m.test(text)
  if (trigger === 'workflow_call') return workflowTriggerNames(text).includes('workflow_call')
  if (trigger === 'release') return /^\s*release:\s*(?:#.*)?$/m.test(text)
  if (trigger === 'push-tag') {
    return /^\s*push:\s*(?:#.*)?$/m.test(text) && /^\s*tags:\s*$/m.test(text) &&
      new RegExp(`^\\s*-\\s*['"]${RELEASE_TARGET_TAG.replace(/[.+?^${}()|[\]\\]/g, '\\$&')}['"]\\s*$`, 'm').test(text)
  }
  return false
}

function permitsDirectTrigger(declared, actual, text) {
  if (actual === 'pull_request') {
    return declared.has('pull_request') || (declared.has('pull_request-closed') && hasTrigger(text, 'pull_request-closed'))
  }
  if (actual === 'push') return declared.has('push-main') || declared.has('push-tag')
  return declared.has(actual)
}

function workflowConcurrency(text) {
  const block = topLevelBlock(text.split(/\r?\n/), 'concurrency')
  if (block === null) return null
  const groupLine = block.find((line) => /^  group:\s*\S/.test(line))
  const group = Boolean(groupLine)
  const cancel = block.find((line) => /^  cancel-in-progress:\s*(true|false)\s*$/.test(line))
  const group_value = groupLine?.replace(/^  group:\s*/, '') ?? null
  if (!cancel) return { group, group_value, cancel_in_progress: null }
  return { group, group_value, cancel_in_progress: /^  cancel-in-progress:\s*true\s*$/.test(cancel) }
}

function mutableActionReferences(lines) {
  const findings = []
  for (const line of lines) {
    const match = line.match(/^\s*(?:-\s*)?uses:\s*([^\s#]+)(?:\s+#\s+(.+))?\s*$/)
    if (!match) continue
    const reference = match[1]
    if (reference.startsWith('./')) continue
    if (!/^[^@]+@[0-9a-f]{40}$/i.test(reference) || !match[2]) findings.push('mutable-action-reference')
  }
  return findings
}

function continueOnErrorSteps(lines) {
  const steps = []
  let stepName = null
  for (const line of lines) {
    const name = line.match(/^\s*-\s*name:\s*(.+?)\s*$/)
    if (name) stepName = name[1]
    const continuation = line.match(/^\s*continue-on-error:\s*(.*?)\s*$/iu)
    if (continuation && continuation[1].toLowerCase() !== 'false') steps.push(stepName)
  }
  return steps
}

function scalarField(line, indent, key) {
  const expression = new RegExp(`^${' '.repeat(indent)}${key}:\\s*(.*?)\\s*$`, 'u')
  const match = line.match(expression)
  return match ? match[1] : null
}

function runValue(lines, start, raw) {
  if (!/^[>|][+-]?$/u.test(raw)) return { value: raw, next: start + 1 }
  const content = []
  let index = start + 1
  while (index < lines.length) {
    const line = lines[index]
    if (line.trim() === '') {
      content.push('')
      index += 1
      continue
    }
    if (!/^ {10,}/u.test(line)) break
    content.push(line.slice(10))
    index += 1
  }
  return { value: content.join('\n'), next: index }
}

function jobSteps(block) {
  const start = block.findIndex((line) => /^    steps:\s*$/u.test(line))
  if (start === -1) return []
  const steps = []
  let current = null
  let index = start + 1
  const addProperty = (step, key, raw, lineIndex) => {
    if (key === 'run') {
      const parsed = runValue(block, lineIndex, raw)
      step.run = parsed.value
      return parsed.next
    }
    step.properties[key] = raw
    return lineIndex + 1
  }
  while (index < block.length) {
    const line = block[index]
    if (/^    [A-Za-z][A-Za-z0-9_-]*:\s*/u.test(line)) break
    const header = line.match(/^      -\s*(.*?)\s*$/u)
    if (header) {
      if (current) steps.push(current)
      current = { properties: {}, run: null }
      const property = header[1].match(/^([A-Za-z][A-Za-z0-9_-]*):\s*(.*?)\s*$/u)
      index = property ? addProperty(current, property[1], property[2], index) : index + 1
      continue
    }
    if (!current) {
      index += 1
      continue
    }
    const property = line.match(/^        ([A-Za-z][A-Za-z0-9_-]*):\s*(.*?)\s*$/u)
    index = property ? addProperty(current, property[1], property[2], index) : index + 1
  }
  if (current) steps.push(current)
  return steps
}

function defaultRunShell(lines, indent) {
  const defaults = lines.findIndex((line) => new RegExp(`^${' '.repeat(indent)}defaults:\\s*$`, 'u').test(line))
  if (defaults === -1) return null
  let run = -1
  for (let index = defaults + 1; index < lines.length; index++) {
    if (new RegExp(`^ {0,${indent}}\\S`, 'u').test(lines[index])) break
    if (new RegExp(`^${' '.repeat(indent + 2)}run:\\s*$`, 'u').test(lines[index])) {
      run = index
      break
    }
  }
  if (run === -1) return null
  for (let index = run + 1; index < lines.length; index++) {
    if (new RegExp(`^ {0,${indent + 2}}\\S`, 'u').test(lines[index])) break
    const shell = scalarField(lines[index], indent + 4, 'shell')
    if (shell !== null) return shell
  }
  return null
}

function normalizedCondition(value) {
  if (typeof value !== 'string') return ''
  const trimmed = value.trim().replace(/^\$\{\{\s*/u, '').replace(/\s*\}\}$/u, '')
  const quoted = trimmed.match(/^(['"])(.*)\1$/u)
  return (quoted ? quoted[2] : trimmed).replace(/\s+/gu, ' ').toLowerCase()
}

function directEventNames(triggers) {
  if (triggers.includes('workflow_call')) return null
  const events = new Set()
  for (const trigger of triggers) {
    if (trigger === 'pull_request' || trigger === 'pull_request-closed') events.add('pull_request')
    else if (trigger === 'push-main' || trigger === 'push-tag') events.add('push')
    else if (trigger === 'workflow_dispatch') events.add('workflow_dispatch')
    else if (trigger === 'release') events.add('release')
  }
  return events.size > 0 ? events : null
}

function conditionCanRun(value, triggers) {
  const condition = normalizedCondition(value)
  if (!condition || condition === 'always()' || condition === 'success()' || condition === 'true') return true
  if (/^(?:false|0|!true|1\s*==\s*0)$/u.test(condition) || /(?:^|&&)\s*false(?:\s|$)/u.test(condition) ||
      /\b(?:failure|cancelled)\(\)/u.test(condition)) return false
  const events = directEventNames(triggers)
  if (!events) return true
  const equal = [...condition.matchAll(/github\.event_name\s*==\s*['"]([a-z_]+)['"]/gu)]
  if (equal.some((match) => !events.has(match[1]))) return false
  const unequal = [...condition.matchAll(/github\.event_name\s*!=\s*['"]([a-z_]+)['"]/gu)]
  return !unequal.some((match) => events.size === 1 && events.has(match[1]))
}

function heredocFreeScript(value) {
  const lines = value.split(/\r?\n/u)
  const active = []
  const executable = []
  let sawHeredoc = false
  for (const line of lines) {
    if (active.length > 0) {
      if (line.trim() === active[0]) active.shift()
      continue
    }
    executable.push(line)
    for (const match of line.matchAll(/<<-?\s*(?:'([^']+)'|"([^"]+)"|([A-Za-z_][A-Za-z0-9_]*))/gu)) {
      active.push(match[1] ?? match[2] ?? match[3])
      sawHeredoc = true
    }
  }
  return { text: executable.join('\n'), sawHeredoc }
}

function normalizeShellScript(value) {
  return value.replace(/\\\r?\n/gu, ' ').replace(/\s+/gu, ' ').trim()
}

function containsRequiredCommand(value, command) {
  return normalizeShellScript(value).includes(normalizeShellScript(command))
}

function shellSegments(line) {
  const segments = []
  let start = 0
  let quote = ''
  for (let index = 0; index < line.length; index++) {
    const character = line[index]
    if (character === '\\') {
      index += 1
      continue
    }
    if (quote) {
      if (character === quote) quote = '';
      continue
    }
    if (character === "'" || character === '"') {
      quote = character
      continue
    }
    if (character === ';' || character === '|') {
      segments.push(line.slice(start, index))
      if ((character === '|' || character === '&') && line[index + 1] === character) index += 1
      start = index + 1
    }
  }
  segments.push(line.slice(start))
  return segments
}

function containsExecutableCommand(value, command) {
  const textOnly = /^(?:echo|printf|:|true|false|cat|grep|sed|awk)\b/iu
  return value.split(/\r?\n/u).some((line) => {
    const segments = shellSegments(line)
    return segments.some((segment) => {
      const executable = segment.trim()
      return executable !== '' && !executable.startsWith('#') && !textOnly.test(executable) &&
        containsRequiredCommand(executable, command)
    })
  })
}

function requiredCommandContexts(value, command) {
  return value.split(/\r?\n/u).filter((line) => containsRequiredCommand(line, command))
}

function shellControlText(value) {
  let quote = ''
  let controls = ''
  for (let index = 0; index < value.length; index++) {
    const character = value[index]
    if (character === '\\') {
      controls += ' '
      index += 1
      continue
    }
    if (quote) {
      if (character === quote) quote = ''
      else if (quote === '"' && character === '$' && value[index + 1] === '(') controls += '$('
      else controls += ' '
      continue
    }
    if (character === "'" || character === '"') {
      quote = character
      continue
    }
    controls += character
  }
  return controls
}

function masksFailure(value, shell) {
  const controls = shellControlText(value)
  const powershell = /^(?:pwsh|powershell)\b/iu.test(shell ?? '')
  return (!powershell && /(^|[^&])&(?!&)/u.test(controls)) || /\|&?/u.test(controls) || /\|\|/u.test(controls) ||
    /;\s*(?:exit\s+0|true)(?:\s*;|\s*$)/u.test(controls) || /&&\s*true(?:\s*;|\s*$)/u.test(controls) ||
    /(?:^|[;&|]\s*)!\s*\S/u.test(controls) || /\b(?:if|while|until)\b/u.test(controls) || /\$\(/u.test(controls) ||
    /\([^\r\n]*\)/u.test(controls)
}

function unsafeShell(value) {
  if (!value) return false
  const shell = value.trim().toLowerCase()
  if (['bash', 'sh', 'pwsh', 'powershell', 'cmd'].includes(shell)) return false
  if (!shell.includes('{0}')) return false
  if (/\b(?:bash|sh)\b/u.test(shell)) return !/(?:\s-e(?:\s|$)|\berrexit\b)/u.test(shell)
  if (/\b(?:pwsh|powershell)\b/u.test(shell)) return !/erroractionpreference\s*=\s*['"]?stop/u.test(shell)
  return true
}

function requiredCommandFindings(gate, text, block) {
  const findings = []
  const steps = jobSteps(block)
  const jobIf = fieldValue(block, 'if')
  const jobContinuation = fieldValue(block, 'continue-on-error')
  const workflowShell = defaultRunShell(text.split(/\r?\n/u), 0)
  const jobShell = defaultRunShell(block, 4)
  for (const command of gate.required_commands) {
    let rawMatch = false
    let heredocOnly = false
    let unreachable = false
    let continued = false
    let masked = false
    let shellUnsafe = false
    let executable = false
    for (const step of steps) {
      if (typeof step.run !== 'string') continue
      if (containsRequiredCommand(step.run, command)) rawMatch = true
      const script = heredocFreeScript(step.run)
      const contexts = requiredCommandContexts(script.text, command)
      if (!containsExecutableCommand(script.text, command)) {
        if (containsRequiredCommand(step.run, command) && script.sawHeredoc) heredocOnly = true
        if (contexts.some((context) => masksFailure(context, step.properties.shell ?? jobShell ?? workflowShell))) masked = true
        continue
      }
      if (!conditionCanRun(jobIf, gate.triggers) || !conditionCanRun(step.properties.if, gate.triggers)) {
        unreachable = true
        continue
      }
      if ((typeof jobContinuation === 'string' && jobContinuation.trim().toLowerCase() !== 'false') ||
          (typeof step.properties['continue-on-error'] === 'string' && step.properties['continue-on-error'].trim().toLowerCase() !== 'false')) {
        continued = true
        continue
      }
      const shell = step.properties.shell ?? jobShell ?? workflowShell
      if (contexts.some((context) => masksFailure(context, shell))) {
        masked = true
        continue
      }
      if (unsafeShell(shell)) {
        shellUnsafe = true
        continue
      }
      executable = true
    }
    if (executable) continue
    if (heredocOnly && !unreachable && !continued && !masked && !shellUnsafe) findings.push('required-command-heredoc-only')
    else if (unreachable) findings.push('required-command-unreachable')
    else if (continued) findings.push('required-command-continue-on-error')
    else if (masked) findings.push('required-command-failure-masked')
    else if (shellUnsafe) findings.push('required-command-shell-unsafe')
    else if (!rawMatch) findings.push('required-command-absent')
    else findings.push('required-command-not-executable')
  }
  return findings
}

function workflowDispatchInputValues(text, input) {
  const lines = text.split(/\r?\n/)
  const start = lines.findIndex((line) => new RegExp(`^      ${input}:\\s*$`).test(line))
  if (start === -1) return null
  const values = []
  let isChoice = false
  for (let index = start + 1; index < lines.length; index++) {
    const line = lines[index]
    if (/^      [A-Za-z][A-Za-z0-9_-]*:\s*$/.test(line)) break
    if (/^        type:\s*choice\s*$/.test(line)) isChoice = true
    const value = line.match(/^          -\s+(.+?)\s*$/)
    if (value) values.push(value[1])
  }
  return { isChoice, values }
}

function workflowDispatchStringInput(text, input) {
  const lines = text.split(/\r?\n/)
  const start = lines.findIndex((line) => new RegExp(`^      ${input}:\\s*$`).test(line))
  if (start === -1) return false
  let required = false
  let type = false
  for (let index = start + 1; index < lines.length; index++) {
    const line = lines[index]
    if (/^      [A-Za-z][A-Za-z0-9_-]*:\s*$/.test(line)) break
    if (/^        required:\s*true\s*$/.test(line)) required = true
    if (/^        type:\s*string\s*$/.test(line)) type = true
  }
  return required && type
}

function labPlanWorkflowFindings(gate, text, block) {
  const policy = gate.policy.lab_plan
  if (!policy) return []
  const joined = block.join('\n')
  const observed = []
  if (fieldValue(block, 'runs-on') !== 'ubuntu-latest' || /runs-on:\s*self-hosted/i.test(joined)) {
    observed.push('lab-plan-runner-invalid')
  }
  if (fieldValue(block, 'environment') !== policy.environment) observed.push('lab-plan-environment-mismatch')

  for (const [input, expected] of [['mode', policy.mode], ['target', policy.target]]) {
    const values = workflowDispatchInputValues(text, input)
    if (!values?.isChoice || values.values.length !== 1 || values.values[0] !== expected) {
      observed.push('lab-plan-dispatch-input-invalid')
    }
  }

  const environmentMarkers = [
    'LAB_MODE: ${{ inputs.mode }}',
    'LAB_TARGET: ${{ inputs.target }}',
    'LAB_REVISION: ${{ github.sha }}',
    `LAB_ENVIRONMENT: ${policy.environment}`,
  ]
  if (!environmentMarkers.every((marker) => joined.includes(marker)) ||
      /github\.event\.inputs|\$\{\{\s*inputs\.(?!mode\s*\}\}|target\s*\}\})/i.test(joined)) {
    observed.push('lab-plan-input-binding-mismatch')
  }

  const expectedRuns = new Set([
    'mkdir -p tmp/ci-lab-plan',
    `node tools/ci/plan-isolated-lab.mjs --kind ${policy.kind} --out tmp/ci-lab-plan/plan.json --manifest tmp/ci-lab-plan/artifact-manifest.json`,
    'node tools/ci/check-ci-artifacts.mjs --check tmp/ci-lab-plan/artifact-manifest.json --root tmp/ci-lab-plan',
  ])
  const runCommands = block
    .map((line) => line.match(/^\s*run:\s*(\S.*)$/))
    .filter(Boolean)
    .map((match) => match[1])
  if (runCommands.length !== expectedRuns.size || runCommands.some((command) => !expectedRuns.has(command))) {
    observed.push('lab-plan-host-action')
  }

  const allowedActions = ['actions/checkout@', 'actions/setup-node@', 'actions/upload-artifact@']
  const actionReferences = [...joined.matchAll(/\buses:\s*([^\s#]+)/g)].map((match) => match[1])
  if (actionReferences.length !== allowedActions.length ||
      actionReferences.some((reference) => !allowedActions.some((allowed) => reference.startsWith(allowed)))) {
    observed.push('lab-plan-action-invalid')
  }
  if (!joined.includes('retention-days: 14') || !joined.includes('path: tmp/ci-lab-plan')) {
    observed.push('lab-plan-artifact-invalid')
  }

  const forbidden = /\b(?:Install-[A-Za-z]|Start-Service|Stop-Service|Set-Service|sc\.exe|New-VM|Start-VM|wsl\.exe|Restart-Computer|shutdown\.exe|diskpart|bcdedit|nvidia-smi|gpu)\b/i
  if (forbidden.test(joined)) observed.push('lab-plan-host-action')
  return observed
}

export function releaseProducerManifestMatchesTransition(manifest, policy) {
  if (!isObject(manifest) || !isObject(policy) || Object.keys(manifest).length !== 1) return false
  const version = manifest['.']
  return typeof version === 'string' &&
    (version === policy.baseline_version || version === policy.release_as)
}

function releaseProducerWorkflowFindings(gate, text, block, root) {
  const policy = gate.policy.release_producer
  if (!policy) return []
  const observed = []
  const joined = block.join('\n')
  const appTokenStart = joined.indexOf('id: release-app-token')
  const appTokenUse = joined.indexOf('uses: actions/create-github-app-token@', appTokenStart)
  if (!joined.includes('name: Require release GitHub App credentials') ||
      !joined.includes('test -n "$RELEASE_APP_ID"') || !joined.includes('test -n "$RELEASE_APP_PRIVATE_KEY"') ||
      appTokenStart === -1 || appTokenUse === -1 || /\bif:\s*/.test(joined.slice(appTokenStart, appTokenUse)) ||
      !joined.includes('token: ${{ steps.release-app-token.outputs.token }}') ||
      !joined.includes('git ls-remote --refs') || !joined.includes('refs/tags/$RELEASE_TARGET_TAG') ||
      !joined.includes("if: ${{ steps.target.outputs.run == 'true' }}") ||
      /RELEASE_PLEASE_TOKEN|GITHUB_TOKEN|\bPAT\b|\|\|/.test(joined)) {
    observed.push('release-producer-credential-invalid')
  }
  try {
    const config = JSON.parse(readFileSync(path.join(root, policy.release_config), 'utf8'))
    const manifest = JSON.parse(readFileSync(path.join(root, '.release-please-manifest.json'), 'utf8'))
    if (config['release-type'] !== 'simple' || config.versioning !== 'prerelease' ||
        config['prerelease-type'] !== 'beta' || config.prerelease !== true || config.draft !== true ||
        config['force-tag-creation'] !== true || config['skip-github-release'] !== false ||
        config['release-as'] !== policy.target_tag.slice(1) ||
        !releaseProducerManifestMatchesTransition(manifest, policy)) {
      observed.push('release-producer-config-invalid')
    }
  } catch {
    observed.push('release-producer-config-invalid')
  }
  return observed
}

function releaseIntegrityWorkflowFindings(gate, text, block) {
  const policy = gate.policy.release_integrity
  if (!policy) return []
  const joined = block.join('\n')
  const observed = []
  if (fieldValue(block, 'runs-on') !== 'ubuntu-latest' || /runs-on:\s*self-hosted/i.test(joined)) {
    observed.push('release-integrity-runner-invalid')
  }
  if (fieldValue(block, 'environment') !== policy.environment) observed.push('release-integrity-environment-mismatch')
  const markers = [
    'git diff --quiet',
    'cargo install cargo-cyclonedx --locked --version 0.5.9',
    'cargo cyclonedx --manifest-path Cargo.toml --format json --spec-version 1.5 --override-filename ramshared-sbom',
    'test "$RELEASE_TAG" = "$RELEASE_TARGET_TAG"',
    'sha256sum "$archive" > "$archive.sha256"',
    'node tools/ci/write-release-manifest.mjs',
    '--checksum "artifacts/release/ramshared-linux-$RELEASE_TAG.tar.gz.sha256"',
    'node tools/ci/check-release-integrity.mjs --check',
    'actions/upload-artifact@',
    `retention-days: ${policy.integrity_artifact_retention_days}`,
    'release-integrity-${{ env.RELEASE_TAG }}-${{ steps.identity.outputs.revision }}',
  ]
  if (!markers.every((marker) => joined.includes(marker)) || !text.includes(`- '${policy.target_tag}'`)) {
    observed.push('release-integrity-command-mismatch')
  }
  const publishing = /workflow_dispatch|gh\s+release|upload-release-asset|action-gh-release|create-release|release-please/i
  if (publishing.test(text)) observed.push('release-integrity-publication-reachable')
  return observed
}

function releasePublicationWorkflowFindings(gate, text, block) {
  const policy = gate.policy.release_publication
  if (!policy) return []
  const joined = block.join('\n')
  const observed = []
  if (fieldValue(block, 'runs-on') !== 'ubuntu-latest' || /runs-on:\s*self-hosted/i.test(joined)) {
    observed.push('release-publication-runner-invalid')
  }
  if (fieldValue(block, 'environment') !== policy.environment) observed.push('release-publication-environment-mismatch')
  if (!['tag', 'source_sha', 'integrity_run_id'].every((input) => workflowDispatchStringInput(text, input))) {
    observed.push('release-publication-dispatch-input-invalid')
  }
  const markers = [
    "if: github.ref == 'refs/heads/main'",
    'ref: ${{ github.event.repository.default_branch }}',
    'RELEASE_TAG: ${{ inputs.tag }}',
    'SOURCE_SHA: ${{ inputs.source_sha }}',
    'INTEGRITY_RUN_ID: ${{ inputs.integrity_run_id }}',
    'node tools/ci/check-release-publication.mjs',
    `--policy ${policy.promotion_policy}`,
    'node tools/ci/check-release-integrity.mjs',
    '--check artifacts/release/release-manifest.json',
    'name: Require publication GitHub App credentials',
    'permission-contents: write',
    'permission-actions: read',
    'ref: ${{ inputs.source_sha }}',
    'refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG',
    'actions/download-artifact@',
    'name: release-integrity-${{ inputs.tag }}-${{ inputs.source_sha }}',
    'run-id: ${{ inputs.integrity_run_id }}',
    'gh release upload "$RELEASE_TAG" "artifacts/release/$asset"',
    'gh release edit "$RELEASE_TAG" --draft=false --prerelease',
  ]
  if (!markers.every((marker) => joined.includes(marker)) ||
      joined.indexOf('Validate exact protected dispatch identity') > joined.indexOf('Create publication GitHub App token')) {
    observed.push('release-publication-command-mismatch')
  }
  const forbidden = /workflow_run|^\s*push:|^\s*pull_request:|--clobber|gh\s+release\s+create|gh\s+api\s+--method\s+POST/i
  if (forbidden.test(text)) observed.push('release-publication-mutation-topology-invalid')
  return observed
}

function closedPrCancellationWorkflowFindings(gate, block) {
  if (!gate.policy.closed_pr_cancellation) return []
  const joined = block.join('\n')
  const requiredMarkers = [
    'github.paginate(github.rest.actions.listWorkflowRunsForRepo',
    "event: 'pull_request'",
    'head_sha: headSha',
    "for (const status of ['queued', 'in_progress'])",
    'runIds.delete(context.runId);',
    'github.rest.actions.cancelWorkflowRun',
    'error.status !== 409 && error.status !== 422',
  ]
  if (!requiredMarkers.every((marker) => joined.includes(marker)) ||
      /\bactions\/checkout@/.test(joined) || /^\s*-\s*run:\s*/m.test(joined)) {
    return ['closed-pr-cancellation-scope-mismatch']
  }
  return []
}

function policyFindings(gate, text, block, root = ROOT) {
  const observed = []
  if (/^\s*pull_request_target:\s*(?:#.*)?$/m.test(text)) observed.push('pull-request-target')
  for (const trigger of gate.triggers) if (!hasTrigger(text, trigger)) observed.push('trigger-absent')
  const declaredTriggers = new Set(gate.triggers)
  for (const trigger of workflowTriggerNames(text)) {
    if (trigger !== 'workflow_call' && !permitsDirectTrigger(declaredTriggers, trigger, text)) {
      observed.push('undeclared-direct-trigger')
    }
  }

  const observedName = fieldValue(block, 'name') ?? gate.job
  if (observedName !== gate.context) observed.push('context-mismatch')
  const timeout = fieldValue(block, 'timeout-minutes')
  if (!timeout) observed.push('timeout-not-explicit')
  else if (Number.parseInt(timeout, 10) !== gate.policy.timeout_minutes) observed.push('timeout-mismatch')
  if (gate.policy.concurrency) {
    const concurrency = workflowConcurrency(text)
    if (!concurrency?.group || concurrency.cancel_in_progress === null) observed.push('concurrency-not-explicit')
    else if (concurrency.cancel_in_progress !== gate.policy.concurrency.cancel_in_progress) observed.push('concurrency-cancellation-mismatch')
  }
  if (gate.policy.advisory_db) {
    const advisory = gate.policy.advisory_db
    const snapshotMarkers = [
      `RUSTSEC_DB_URL: ${advisory.url}`,
      `RUSTSEC_DB_COMMIT: ${advisory.commit}`,
      `RUSTSEC_DB_COMMIT_UTC: "${advisory.commit_utc}"`,
      `RUSTSEC_DB_MAX_AGE_DAYS: "${advisory.max_age_days}"`,
      'fetch --depth=1 origin "$RUSTSEC_DB_COMMIT"',
      'cargo audit --db "$RUSTSEC_DB_DIR" --no-fetch',
      'RUSTSEC_UPSTREAM_HEAD_HEALTH=scheduled-curator-required',
    ]
    if (!snapshotMarkers.every((marker) => block.join('\n').includes(marker))) observed.push('advisory-db-snapshot-mismatch')
  }
  if (gate.policy.rust_slice_coverage) {
    const coverage = gate.policy.rust_slice_coverage
    const coverageMarkers = [
      `cargo install cargo-llvm-cov --locked --version ${coverage.llvm_cov_version}`,
      `node ${coverage.planner}`,
    ]
    if (!coverageMarkers.every((marker) => block.join('\n').includes(marker))) observed.push('rust-slice-coverage-command-mismatch')
  }
  if (gate.policy.closed_pr_cancellation && fieldValue(block, 'if') !== CLOSED_PR_CANCELLATION_CONDITION) {
    observed.push('closed-pr-cancellation-condition-mismatch')
  }
  observed.push(...closedPrCancellationWorkflowFindings(gate, block))
  observed.push(...labPlanWorkflowFindings(gate, text, block))
  observed.push(...releaseProducerWorkflowFindings(gate, text, block, root))
  observed.push(...releaseIntegrityWorkflowFindings(gate, text, block))
  observed.push(...releasePublicationWorkflowFindings(gate, text, block))

  const rootPermissions = permissionsBlock(text.split(/\r?\n/), 0)
  const scopedPermissions = permissionsBlock(block, 4)
  const effectivePermissions = scopedPermissions ?? rootPermissions
  if (!effectivePermissions) observed.push('permissions-not-explicit')
  else {
    if (gate.policy.permissions_scope === 'job' && !scopedPermissions) observed.push('permissions-not-job-scoped')
    for (const [name, level] of Object.entries(gate.policy.permissions)) {
      if (effectivePermissions[name] !== level) observed.push('permissions-mismatch')
    }
    for (const [name, level] of Object.entries(effectivePermissions)) {
      if (gate.policy.permissions[name] !== level && level === 'write') observed.push('overbroad-permissions')
    }
  }

  if (gate.policy.action_pinning === 'full-sha') observed.push(...mutableActionReferences(block))
  if (gate.policy.continue_on_error === false) {
    for (const step of continueOnErrorSteps(block)) {
      void step
      observed.push('continue-on-error')
    }
  }
  observed.push(...requiredCommandFindings(gate, text, block))
  return [...new Set(observed)].sort()
}

function reusableCallerFindings(contract, caller, root, entrypointText) {
  const findings = []
  const block = jobBlock(entrypointText, caller.job)
  if (!block) return ['aggregate-caller-job-absent']
  const joined = block.join('\n')
  if (fieldValue(block, 'needs') !== null || jobList(block, 'needs') !== null) findings.push('aggregate-caller-has-needs')
  const scopedPermissions = permissionsBlock(block, 4)
  if (!samePermissions(scopedPermissions, callerPermissions(contract, caller))) findings.push('aggregate-caller-permissions-mismatch')
  if (caller.kind === 'direct') {
    if (fieldValue(block, 'uses') !== null || fieldValue(block, 'runs-on') === null) findings.push('aggregate-direct-caller-shape-invalid')
    return findings
  }
  if (fieldValue(block, 'uses') !== caller.workflow || fieldValue(block, 'runs-on') !== null) {
    findings.push('aggregate-reusable-caller-shape-invalid')
    return findings
  }
  const childPath = path.join(root, caller.workflow)
  if (!existsSync(childPath)) {
    findings.push('aggregate-reusable-workflow-absent')
    return findings
  }
  const childText = readFileSync(childPath, 'utf8')
  if (!/^\s*workflow_call:\s*(?:#.*)?$/m.test(childText)) findings.push('aggregate-reusable-workflow-call-absent')
  if (workflowTriggerNames(childText).some((trigger) => trigger === 'pull_request' || trigger === 'push')) {
    findings.push('aggregate-reusable-direct-trigger')
  }
  if (!workflowConcurrency(childText)?.group_value?.includes('${{ github.workflow }}')) findings.push('aggregate-reusable-concurrency-unscoped')
  if (!sameStringSet(jobIds(childText), [...caller.summary_needs, caller.summary_job])) findings.push('aggregate-reusable-child-jobs-mismatch')
  const summary = jobBlock(childText, caller.summary_job)
  if (!summary) {
    findings.push('aggregate-summary-absent')
    return findings
  }
  if (fieldValue(summary, 'if') !== 'always()') findings.push('aggregate-summary-if-always-missing')
  if (!sameStringSet(jobList(summary, 'needs'), caller.summary_needs)) findings.push('aggregate-summary-needs-mismatch')
  const summaryText = summary.join('\n')
  if (!caller.summary_needs.every((job) => summaryText.includes(`needs.${job}.result`)) || !summaryText.includes('exit 1')) {
    findings.push('aggregate-summary-fail-closed-missing')
  }
  if (continueOnErrorSteps(summary).length > 0) findings.push('aggregate-summary-continue-on-error')
  return findings
}

export function validateReusableAggregateArchitecture(contract, entrypointText, { root = ROOT } = {}) {
  const errors = []
  const checked = validateContract(contract)
  if (!checked.ok) return { ok: false, errors: checked.errors }
  const aggregate = contract.aggregate
  const architecture = aggregate?.architecture
  if (aggregate?.implementation !== 'current' || architecture?.kind !== LOCAL_REUSABLE_AGGREGATE_KIND) {
    return { ok: false, errors: [finding('aggregate', 'aggregate-architecture-invalid')] }
  }
  const block = jobBlock(entrypointText, aggregate.job)
  if (!block) return { ok: false, errors: [finding('aggregate', 'aggregate-absent')] }
  if (fieldValue(block, 'if') !== 'always()') errors.push(finding('aggregate', 'aggregate-if-always-missing'))
  if (!sameStringSet(jobList(block, 'needs'), architecture.callers.map((caller) => caller.job))) {
    errors.push(finding('aggregate', 'aggregate-needs-mismatch'))
  }
  if (!block.join('\n').includes('node tools/ci/check-ci-contract.mjs --aggregate-needs')) {
    errors.push(finding('aggregate', 'aggregate-command-missing'))
  }
  if (continueOnErrorSteps(block).length > 0) errors.push(finding('aggregate', 'aggregate-continue-on-error'))
  for (const caller of architecture.callers) {
    for (const rule of reusableCallerFindings(contract, caller, root, entrypointText)) errors.push(finding('aggregate', rule, caller.job))
  }
  return { ok: errors.length === 0, errors: sortFindings(errors) }
}

function eventTrigger(event) {
  if (event === 'pull_request') return 'pull_request'
  if (event === 'push-main') return 'push-main'
  return null
}

export function validateAggregateNeeds(contract, needs, event) {
  const checked = validateContract(contract)
  if (!checked.ok) return { status: 'NO-GO', errors: checked.errors, gaps: [] }
  const trigger = eventTrigger(event)
  if (!trigger || !isObject(needs)) return { status: 'NO-GO', errors: [finding('aggregate', 'aggregate-needs-input-invalid')], gaps: [] }
  const errors = []
  const architecture = contract.aggregate.architecture
  for (const caller of architecture.callers) {
    const active = caller.gates.some((id) => {
      const gate = contract.gates.find((item) => item.id === id)
      return gate?.triggers.includes(trigger)
    })
    if (!active) continue
    const result = needs[caller.job]?.result
    if (result === undefined) errors.push(finding('aggregate', 'aggregate-caller-missing', caller.job))
    else if (result !== 'success') errors.push(finding('aggregate', 'aggregate-caller-not-success', caller.job))
  }
  return { status: errors.length === 0 ? 'PASS' : 'NO-GO', errors: sortFindings(errors), gaps: [] }
}

function settleGateFindings(gate, observed, errors, gaps) {
  const expected = new Set(gate.open_gaps)
  for (const rule of observed) {
    if (expected.delete(rule)) gaps.push(`${gate.id}:${rule}`)
    else errors.push(finding(gate.id, rule))
  }
  for (const rule of expected) errors.push(finding(gate.id, 'stale-open-gap', rule))
}

function inspectGate(gate, root, errors, gaps) {
  if (gate.implementation === 'env-bound' || gate.implementation === 'observed') {
    const observed = readRemoteControlObservation(root, gate.observation ?? REMOTE_CONTROLS_OBSERVATION_PATH)
    if (observed.status === 'missing') {
      if (gate.implementation === 'env-bound') {
        settleGateFindings(gate, ['remote-control-observation-absent'], errors, gaps)
      } else {
        errors.push(finding(gate.id, 'remote-control-observation-absent'))
      }
      return
    }
    if (observed.status === 'error') {
      errors.push(observed.error)
      return
    }
    try {
      const schema = JSON.parse(readFileSync(path.join(root, REMOTE_CONTROLS_SCHEMA_PATH), 'utf8'))
      const schemaResult = validateRemoteControlSchemaDefinition(schema)
      if (!schemaResult.ok) {
        errors.push(...schemaResult.errors)
        return
      }
    } catch {
      errors.push(finding(gate.id, 'observation-schema-read-failed'))
      return
    }
    errors.push(...validateRemoteControlObservation(observed.observation).errors)
    return
  }
  const workflow = path.join(root, gate.workflow)
  if (!existsSync(workflow)) {
    if (gate.implementation === 'planned') settleGateFindings(gate, ['workflow-absent'], errors, gaps)
    else errors.push(finding(gate.id, 'workflow-absent'))
    return
  }
  const text = readFileSync(workflow, 'utf8')
  const block = jobBlock(text, gate.job)
  if (!block) {
    errors.push(finding(gate.id, 'job-absent'))
    return
  }
  if (gate.implementation === 'planned') {
    const observed = gate.required_commands.filter((command) => !block.join('\n').includes(command)).length > 0
      ? ['required-command-absent']
      : []
    settleGateFindings(gate, observed, errors, gaps)
    return
  }
  settleGateFindings(gate, policyFindings(gate, text, block, root), errors, gaps)
}

export function validateWorkflowPolicy(contract, root = ROOT) {
  const checked = validateContract(contract)
  if (!checked.ok) return { status: 'NO-GO', errors: checked.errors, gaps: [] }
  const errors = []
  const gaps = []
  for (const gate of contract.gates) inspectGate(gate, root, errors, gaps)

  const aggregate = contract.aggregate
  if (!existsSync(path.join(root, aggregate.workflow))) {
    settleGateFindings({ id: aggregate.id, open_gaps: aggregate.open_gaps }, ['workflow-absent', 'aggregate-absent'], errors, gaps)
  } else {
    const text = readFileSync(path.join(root, aggregate.workflow), 'utf8')
    const block = jobBlock(text, aggregate.job)
    if (aggregate.implementation === 'current') {
      if (!block) {
        errors.push(finding(aggregate.id, 'aggregate-absent'))
      } else {
        const architecture = validateReusableAggregateArchitecture(contract, text, { root })
        errors.push(...architecture.errors)
      }
    } else {
      settleGateFindings({ id: aggregate.id, open_gaps: aggregate.open_gaps }, block ? [] : ['aggregate-absent'], errors, gaps)
    }
  }
  return {
    status: errors.length > 0 ? 'NO-GO' : gaps.length > 0 ? 'PARTIAL' : 'PASS',
    errors: sortFindings(errors),
    gaps: [...new Set(gaps)].sort(),
  }
}

function pathMatchesPrefix(file, prefix) {
  return file === prefix || prefix.endsWith('/') && file.startsWith(prefix)
}

export function selectWholePullRequestGates(contract, changedPaths) {
  const errors = []
  const selected = []
  const noChange = []
  const paths = Array.isArray(changedPaths) ? changedPaths : []
  if (paths.some((item) => !safeRelative(item))) errors.push(finding('selector', 'changed-path-unsafe'))
  const gates = Array.isArray(contract?.gates) ? contract.gates : []
  for (const gate of gates) {
    if (!gate.required || gate.selection?.mode === 'never') continue
    if (gate.selection?.mode === 'always') selected.push(gate.id)
    else if (gate.selection?.mode === 'paths' && paths.some((file) => gate.selection.paths.some((prefix) => pathMatchesPrefix(file, prefix)))) selected.push(gate.id)
    else noChange.push(gate.id)
  }
  return { selected: selected.sort(), no_change: noChange.sort(), errors: sortFindings(errors) }
}

export function classifyRetry(failureClass, attempt) {
  if (failureClass !== 'dependency-fetch-transport') return { retry: false, delay_seconds: 0, reason: 'retry-not-permitted' }
  if (!Number.isInteger(attempt) || attempt < 1 || attempt >= 2) return { retry: false, delay_seconds: 0, reason: 'retry-budget-exhausted' }
  return { retry: true, delay_seconds: 15, reason: 'classified-transient' }
}

export function validateAggregate(contract, selectedGateIds, results) {
  const errors = []
  const gaps = []
  const contractGates = Array.isArray(contract?.gates) ? contract.gates : []
  const gates = new Map(contractGates.map((gate) => [gate.id, gate]))
  const records = new Map()
  const resultRecords = Array.isArray(results) ? results : []
  for (const record of resultRecords) {
    if (!record || typeof record.id !== 'string' || !TERMINAL_STATES.has(record.state)) {
      errors.push(finding('aggregate', 'result-invalid'))
      continue
    }
    if (records.has(record.id)) errors.push(finding(record.id, 'duplicate-result'))
    records.set(record.id, record)
  }
  const selected = Array.isArray(selectedGateIds) ? selectedGateIds : []
  for (const id of selected) {
    const gate = gates.get(id)
    if (!gate) {
      errors.push(finding(id, 'selected-gate-unknown'))
      continue
    }
    if (gate.implementation !== 'current') {
      if (records.get(id)?.state === 'PASS') errors.push(finding(id, 'planned-gate-passed'))
      else gaps.push(`${id}:gate-not-current`)
      continue
    }
    const record = records.get(id)
    if (!record) {
      errors.push(finding(id, 'missing-result'))
      continue
    }
    if (record.state === 'PASS') continue
    if (record.state === 'NO_CHANGE' && gate.selection?.mode === 'paths') continue
    if (record.state === 'NO_CHANGE') errors.push(finding(id, 'unexpected-no-change'))
    else if (record.state === 'CANCELLED') errors.push(finding(id, 'cancelled-result'))
    else if (record.state === 'SKIPPED') errors.push(finding(id, 'unexpected-skip'))
    else errors.push(finding(id, 'failed-result', record.state))
  }
  return {
    status: errors.length > 0 ? 'NO-GO' : gaps.length > 0 ? 'PARTIAL' : 'PASS',
    errors: sortFindings(errors),
    gaps: [...gaps].sort(),
  }
}

function loadContract(root) {
  try {
    return { contract: JSON.parse(readFileSync(path.join(root, CONTRACT_PATH), 'utf8')) }
  } catch {
    return { error: finding('contract', 'contract-read-failed') }
  }
}

export function run({ root = ROOT } = {}) {
  const loaded = loadContract(root)
  if (loaded.error) return { status: 'NO-GO', errors: [loaded.error], gaps: [] }
  const result = validateWorkflowPolicy(loaded.contract, root)
  if (result.status === 'NO-GO') return result
  if (result.status === 'PASS' && loaded.contract.contract_state !== 'PASS') {
    return { status: 'PARTIAL', gaps: ['contract:declared-state-stale'] }
  }
  if (result.status !== 'PASS' && loaded.contract.contract_state === 'PASS') {
    return { status: 'NO-GO', errors: [finding('contract', 'premature-pass')], gaps: result.gaps }
  }
  return result.status === 'PARTIAL' ? { status: 'PARTIAL', gaps: result.gaps } : result
}

export function runLocal({ root = ROOT } = {}) {
  const result = run({ root })
  if (result.status === 'PASS') return result
  const onlyRemoteGap = result.status === 'PARTIAL' && Array.isArray(result.gaps) && result.gaps.length === 1 &&
    result.gaps[0] === 'remote-controls:remote-control-observation-absent'
  if (onlyRemoteGap) return { status: 'PARTIAL', gaps: result.gaps, local_ok: true }
  return { ...result, local_ok: false }
}

export function main(argv = process.argv.slice(2), { root = ROOT, print = console.log, error = console.error } = {}) {
  if (argv.length === 2 && argv[0] === '--aggregate-needs') {
    let needs
    try {
      needs = JSON.parse(argv[1])
    } catch {
      error('CI_CONTRACT_ERROR=aggregate:aggregate-needs-json-invalid')
      return 2
    }
    const loaded = loadContract(root)
    if (loaded.error) {
      error(`CI_CONTRACT_ERROR=${loaded.error.gate}:${loaded.error.rule}`)
      return 1
    }
    const event = process.env.GITHUB_EVENT_NAME === 'pull_request' ? 'pull_request' :
      process.env.GITHUB_EVENT_NAME === 'push' ? 'push-main' : ''
    const result = validateAggregateNeeds(loaded.contract, needs, event)
    print(`CI_AGGREGATE_STATUS=${result.status}`)
    for (const item of result.errors ?? []) error(`CI_AGGREGATE_ERROR=${item.gate}:${item.rule}`)
    print(`CI_AGGREGATE_VERDICT=${result.status === 'PASS' ? 'PASS' : 'NO-GO'}`)
    return result.status === 'PASS' ? 0 : 1
  }
  if (!(argv.length === 1 && (argv[0] === '--check' || argv[0] === '--check-local'))) {
    error('usage: check-ci-contract.mjs --check | --check-local | --aggregate-needs <json>')
    return 2
  }
  const local = argv[0] === '--check-local'
  const result = local ? runLocal({ root }) : run({ root })
  print(`CI_CONTRACT_STATUS=${result.status}`)
  for (const item of result.gaps ?? []) print(`CI_CONTRACT_GAP=${item}`)
  for (const item of result.errors ?? []) error(`CI_CONTRACT_ERROR=${item.gate}:${item.rule}`)
  const localPass = local && result.local_ok === true
  print(`CI_CONTRACT_VERDICT=${result.status === 'PASS' ? 'PASS' : localPass ? 'PARTIAL' : 'NO-GO'}`)
  return result.status === 'PASS' || localPass ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main()
