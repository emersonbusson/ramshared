import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  REQUIRED_POINTER,
  main,
  run,
  validateDocument,
  validatePointers,
} from './check-agent-orchestration.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const COMPLETE_RULE = readFileSync(path.join(ROOT, '.claude', 'rules', 'agent-orchestration.md'), 'utf8')
const POINTER_SOURCE = '- Its rendered policy and canonical typed records are the machine-checked source.'
const ROOT_SOL_INVARIANT = `- Root Sol is read-only and must not edit, self-approve, commit, push, merge,
  or run host or destructive actions.`

function validPointers() {
  return `${REQUIRED_POINTER}\n${POINTER_SOURCE}\n`
}

function fixtureRoot({ rule = COMPLETE_RULE, agents = validPointers(), claude = validPointers() } = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-agent-orchestration-'))
  mkdirSync(path.join(root, '.claude', 'rules'), { recursive: true })
  writeFileSync(path.join(root, '.claude', 'rules', 'agent-orchestration.md'), rule)
  writeFileSync(path.join(root, 'AGENTS.md'), agents)
  writeFileSync(path.join(root, 'CLAUDE.md'), claude)
  return root
}

function findingsFor(rule) {
  return validateDocument(rule)
}

function hasRule(findings, rule) {
  return findings.some((item) => item.rule === rule)
}

function concealRootSolInvariant(wrapper) {
  return COMPLETE_RULE.replace(ROOT_SOL_INVARIANT, wrapper(ROOT_SOL_INVARIANT))
}

function indentEveryLine(value, prefix) {
  return value.split('\n').map((line) => `${prefix}${line}`).join('\n')
}

test('accepts the rendered orchestration contract and synchronized pointers', () => {
  assert.deepEqual(validateDocument(COMPLETE_RULE), [])
  assert.deepEqual(validatePointers(validPointers(), validPointers()), [])
  const result = run({ root: fixtureRoot() })
  assert.equal(result.ok, true)
  assert.equal(result.counts.findings, 0)
})

test('rejects a missing schema marker and each route', () => {
  const invalid = COMPLETE_RULE
    .replace('<!-- agent-orchestration-schema-v1 -->', '')
    .replace('| R4 |', '| X4 |')
  const findings = findingsFor(invalid)
  assert.equal(hasRule(findings, 'schema-marker'), true)
  assert.equal(findings.some((item) => item.rule === 'route-missing' && item.value === 'R4'), true)
})

test('ignores non-rendered CommonMark text when checking required authority invariants', () => {
  const concealedForms = [
    (value) => `\`\`\`text\n${value}\n\`\`\``,
    (value) => `    ${indentEveryLine(value, '    ').trimStart()}`,
    (value) => indentEveryLine(value, '\t'),
    (value) => indentEveryLine(value, ' \t'),
    (value) => `before<!--${value}-->after`,
    (value) => `<pre>\n${value}\n</pre>`,
    (value) => `<script>\n${value}\n</script>`,
    (value) => `<style>\n${value}\n</style>`,
    (value) => `<textarea>\n${value}\n</textarea>`,
  ]
  for (const conceal of concealedForms) {
    assert.equal(hasRule(findingsFor(concealRootSolInvariant(conceal)), 'rendered-invariant-missing'), true)
  }

  const eofComment = COMPLETE_RULE
    .replace(ROOT_SOL_INVARIANT, '')
    .concat(`\n<!--${ROOT_SOL_INVARIANT}`)
  assert.equal(hasRule(findingsFor(eofComment), 'rendered-invariant-missing'), true)

  const malformedRawClose = concealRootSolInvariant((value) => `<pre>\nignored\n</pre >\n${value}`)
  assert.equal(hasRule(findingsFor(malformedRawClose), 'rendered-invariant-missing'), true)
})

test('uses CommonMark fence grammar for pointer visibility', () => {
  const fenced = `\`\`\`yaml\n${REQUIRED_POINTER}\n\`\`\`\n`
  const indented = ` \t${REQUIRED_POINTER}\n`
  const comment = `<!--${REQUIRED_POINTER}-->\n`
  const raw = `<textarea>\n${REQUIRED_POINTER}\n</textarea>\n`
  const malformedRawClose = `<pre>\n${REQUIRED_POINTER}\n</pre >\n`
  for (const agents of [fenced, indented, comment, raw, malformedRawClose]) {
    assert.equal(hasRule(validatePointers(agents, validPointers()), 'pointer-missing'), true)
  }

  const invalidBacktickInfo = `\`\`\`yaml \`not-an-info-string\`\n${REQUIRED_POINTER}\n`
  assert.equal(hasRule(validatePointers(invalidBacktickInfo, validPointers()), 'pointer-missing'), false)
})

test('rejects rendered authority contradictions and ignores concealed contradictions', () => {
  const contradictions = [
    'Root Sol may edit a worker file.',
    'Root Sol may self-approve a Sol gate.',
    'Root Sol may commit the change.',
    'Root Sol may push the change.',
    'Root Sol may merge the change.',
    'Root Sol may run host actions.',
  ]
  for (const contradiction of contradictions) {
    assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\n${contradiction}\n`), 'root-sol-authority-grant'), true)
  }
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nA worker may spawn another agent.\n`), 'worker-spawn-grant'), true)
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nA worker may inherit a stale approval.\n`), 'stale-approval-grant'), true)
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nOne Sol result may satisfy both gates.\n`), 'sol-gates-reused'), true)
  const hidden = `${COMPLETE_RULE}\n<!-- Root Sol may commit the change. -->\n`
  assert.equal(hasRule(findingsFor(hidden), 'root-sol-authority-grant'), false)
})

test('distinguishes prohibited grants from rendered denials across equivalent wording', () => {
  assert.equal(
    hasRule(findingsFor(`${COMPLETE_RULE}\nRoot Sol is not allowed to edit a worker file.\n`), 'root-sol-authority-grant'),
    false
  )
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nRoot Sol may reboot the host.\n`), 'root-sol-authority-grant'), true)
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nWorkers may spawn agents.\n`), 'worker-spawn-grant'), true)
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nWorkers may use an inherited approval.\n`), 'stale-approval-grant'), true)
  assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\nA single Sol verdict may cover both gates.\n`), 'sol-gates-reused'), true)
})

test('validates dispatch card fields as one actual bounded record', () => {
  const invalidRecords = [
    COMPLETE_RULE.replace('schema: ramshared.dispatch.v1', 'schema: ramshared.dispatch.v2'),
    COMPLETE_RULE.replace('route: R3', 'route: R9'),
    COMPLETE_RULE.replace('model: gpt-5.6-terra', 'model: gpt-5.6-luna'),
    COMPLETE_RULE.replace('tier: medium', 'tier: imaginary'),
    COMPLETE_RULE.replace('owner: worker-agent-id', 'owner: [worker-a, worker-b]'),
    COMPLETE_RULE.replace('parent: root-agent-id', 'parent: worker-agent-id'),
    COMPLETE_RULE.replace('[tools/ci/check-agent-orchestration.mjs]', '[/tmp/unsafe.mjs]'),
    COMPLETE_RULE.replace('read_only: false', 'read_only: maybe'),
    COMPLETE_RULE.replace('approval: current-user-request', 'approval: none'),
    COMPLETE_RULE.replace('tests: [node --test tools/ci/check-agent-orchestration.test.mjs]', 'tests: []'),
    COMPLETE_RULE.replace('coverage: lines >= 80, branches >= 80, functions >= 80', 'coverage: lines >= 79, branches >= 80, functions >= 80'),
    COMPLETE_RULE.replace('rollback_trigger: checker-refusal-is-observable', 'rollback_trigger: '),
  ]
  for (const invalid of invalidRecords) {
    assert.equal(hasRule(findingsFor(invalid), 'dispatch-record-invalid'), true)
  }
})

test('reconciles the handoff identity, scope, status, and test result with dispatch', () => {
  const invalidHandoffs = [
    COMPLETE_RULE.replace('status: PARTIAL', 'status: DONE'),
    COMPLETE_RULE.replace('result: PASS', 'result: UNKNOWN'),
    COMPLETE_RULE.replace('changed_files: [tools/ci/check-agent-orchestration.mjs]', 'changed_files: [../escape.mjs]'),
    COMPLETE_RULE.replace('changed_files: [tools/ci/check-agent-orchestration.mjs]', 'changed_files: [tools/ci/check-ci-contract.mjs]'),
    COMPLETE_RULE.replace('schema: ramshared.handoff.v1\ndispatch_id: current-turn-unique-id', 'schema: ramshared.handoff.v1\ndispatch_id: another-turn-id'),
  ]
  for (const invalid of invalidHandoffs) {
    const findings = findingsFor(invalid)
    assert.equal(hasRule(findings, 'handoff-record-invalid') || hasRule(findings, 'handoff-reconciliation'), true)
  }
})

test('refuses root-owned mutating dispatch', () => {
  const rule = COMPLETE_RULE.replace('owner: worker-agent-id', 'owner: root-sol')
  assert.equal(hasRule(findingsFor(rule), 'dispatch-record-invalid'), true)
})

test('refuses read-only mutation, duplicate cards, and unbounded tests', () => {
  const cases = [
    ['read-only handoff with changed files', COMPLETE_RULE.replace('read_only: false', 'read_only: true'), 'handoff-reconciliation'],
    ['duplicate include', COMPLETE_RULE.replace('[tools/ci/check-agent-orchestration.mjs]', '[tools/ci/check-agent-orchestration.mjs, tools/ci/check-agent-orchestration.mjs]'), 'dispatch-record-invalid'],
    ['duplicate exclude', COMPLETE_RULE.replace('[scripts/safety/cascade-up.sh]', '[scripts/safety/cascade-up.sh, scripts/safety/cascade-up.sh]'), 'dispatch-record-invalid'],
    ['duplicate dispatch test', COMPLETE_RULE.replace('tests: [node --test tools/ci/check-agent-orchestration.test.mjs]', 'tests: [node --test tools/ci/check-agent-orchestration.test.mjs, node --test tools/ci/check-agent-orchestration.test.mjs]'), 'dispatch-record-invalid'],
    ['duplicate handoff test', COMPLETE_RULE.replace('{command: node --test tools/ci/check-agent-orchestration.test.mjs, result: PASS}', '{command: node --test tools/ci/check-agent-orchestration.test.mjs, result: PASS}, {command: node --test tools/ci/check-agent-orchestration.test.mjs, result: PASS}'), 'handoff-record-invalid'],
    ['unbounded while test', COMPLETE_RULE.replace('tests: [node --test tools/ci/check-agent-orchestration.test.mjs]', 'tests: [while true]').replace('{command: node --test tools/ci/check-agent-orchestration.test.mjs, result: PASS}', '{command: while true, result: PASS}'), 'dispatch-record-invalid'],
  ]
  for (const [name, rule, expected] of cases) {
    assert.equal(hasRule(findingsFor(rule), expected), true, name)
  }
})

test('refuses active unsafe authority grants while allowing negated canonical safety prose', () => {
  const actions = [
    'delete repository files',
    'format or wipe the disk',
    'stop or disable a host service',
    'run swapon or swapoff',
    'destroy or delete a device',
    'kill host processes',
    'mutate host, device, or destructive surfaces',
  ]
  for (const subject of ['Root Sol', 'Any worker']) {
    for (const action of actions) {
      const grant = `${subject} may ${action} without required fresh approval.`
      assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\n${grant}\n`), 'unsafe-authority-grant'), true, grant)
    }
  }
  for (const denial of ['Root Sol may not delete repository files.', 'Any worker cannot format or wipe the disk.']) {
    assert.equal(hasRule(findingsFor(`${COMPLETE_RULE}\n${denial}\n`), 'unsafe-authority-grant'), false, denial)
  }
})

test('refuses a separate active destructive grant even when a denial also exists', () => {
  const actions = [
    'delete repository files',
    'format or wipe the disk',
    'stop or disable a host service',
    'run swapon or swapoff',
    'destroy or delete a device',
    'kill host processes',
  ]
  for (const action of actions) {
    const denial = `Root Sol may not ${action}.`
    const grant = `Any worker may ${action} without required fresh approval.`
    const findings = findingsFor(`${COMPLETE_RULE}\n${denial}\n${grant}\n`)
    assert.equal(hasRule(findings, 'unsafe-authority-grant'), true, action)
  }
})

test('rejects duplicate typed records and unsafe or incomplete pointers', () => {
  const duplicate = `${COMPLETE_RULE}\n\`\`\`yaml\nschema: ramshared.dispatch.v1\n\`\`\`\n`
  assert.equal(hasRule(findingsFor(duplicate), 'typed-record-count'), true)

  const findings = validatePointers(
    `${REQUIRED_POINTER}\n${REQUIRED_POINTER}\n`,
    'Agent orchestration details are copied here.\n'
  )
  assert.equal(hasRule(findings, 'pointer-count'), true)
  assert.equal(hasRule(findings, 'pointer-missing'), true)
  assert.equal(hasRule(findings, 'pointer-not-concise'), true)
  const mismatched = validatePointers(validPointers(), `${REQUIRED_POINTER}\n${REQUIRED_POINTER}\n${POINTER_SOURCE}\n`)
  assert.equal(hasRule(mismatched, 'pointer-sync'), true)
})

test('run fails closed for missing files and malformed input', () => {
  const root = fixtureRoot({ rule: '', agents: '', claude: '' })
  const result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(result.counts.findings > 0, true)
  const missingRoot = mkdtempSync(path.join(tmpdir(), 'ramshared-agent-orchestration-missing-'))
  const missing = run({ root: missingRoot })
  assert.equal(missing.ok, false)
  assert.equal(missing.findings.some((item) => item.rule === 'file-missing'), true)
  const unreadableRoot = mkdtempSync(path.join(tmpdir(), 'ramshared-agent-orchestration-unreadable-'))
  mkdirSync(path.join(unreadableRoot, '.claude', 'rules'), { recursive: true })
  mkdirSync(path.join(unreadableRoot, 'AGENTS.md'))
  writeFileSync(path.join(unreadableRoot, '.claude', 'rules', 'agent-orchestration.md'), COMPLETE_RULE)
  writeFileSync(path.join(unreadableRoot, 'CLAUDE.md'), validPointers())
  const unreadable = run({ root: unreadableRoot })
  assert.equal(unreadable.findings.some((item) => item.rule === 'file-read'), true)
})

test('main accepts --check and rejects other invocations', () => {
  const root = fixtureRoot()
  assert.equal(main(['--check'], { root, print: () => {}, error: () => {} }), 0)
  assert.equal(main([], { root, print: () => {}, error: () => {} }), 2)
  assert.equal(main(['--check', '--extra'], { root, print: () => {}, error: () => {} }), 2)
})
