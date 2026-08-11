import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  LanguageError,
  MAX_FILE_BYTES,
  RATCHET_BASELINE_PATH,
  checkFile,
  classifyPath,
  cleanCommentText,
  evaluateRatchetTransition,
  formatFinding,
  getDiffAddedLines,
  isCommentLine,
  main,
  run,
  runRatchet,
  scanBuffer,
  scanText,
  validatePathList,
} from './check-comment-language.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const CLI = path.join(ROOT, 'tools/ci/check-comment-language.mjs')

function text(...points) {
  return String.fromCodePoint(...points)
}

const HIGH = text(110, 97, 111)
const LOW_A = text(112, 97, 114, 97)
const LOW_B = text(99, 111, 109)
const SILENT_IO = { log() {}, error() {} }
const RATCHET_SCHEMA = './comment-language-baseline.schema.json'
const RATCHET_VERSION = 'ramshared-comment-language-ratchet/v1'
const RATCHET_APPROVAL = {
  approver_role: 'repository-maintainer',
  channel: 'pull-request-review',
}

function git(root, args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' })
}

function write(root, relative, contents) {
  const target = path.join(root, relative)
  mkdirSync(path.dirname(target), { recursive: true })
  writeFileSync(target, contents)
}

function repository(t, files) {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-comment-language-'))
  git(root, ['init', '-q'])
  git(root, ['config', 'user.name', 'RamShared Fixture'])
  git(root, ['config', 'user.email', ['fixture', 'example.invalid'].join('@')])
  for (const [relative, contents] of Object.entries(files)) write(root, relative, contents)
  git(root, ['add', '--all'])
  git(root, ['commit', '-qm', 'fixture'])
  t.after(() => rmSync(root, { recursive: true, force: true }))
  return root
}

function copy(value) {
  return JSON.parse(JSON.stringify(value))
}

function ratchetRecord(initial, current, revision = 0) {
  return {
    $schema: RATCHET_SCHEMA,
    schema: RATCHET_VERSION,
    revision,
    approval: copy(RATCHET_APPROVAL),
    initial: copy(initial),
    current: copy(current),
  }
}

function writeRatchet(root, record) {
  write(root, RATCHET_BASELINE_PATH, `${JSON.stringify(record, null, 2)}\n`)
}

function inventorySnapshot(root) {
  return run({ root, mode: 'all' }).snapshot
}

function commitRatchet(root, message) {
  git(root, ['add', '--all'])
  git(root, ['commit', '-qm', message])
}

function establishRatchetBase(root) {
  const snapshot = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(snapshot, snapshot))
  commitRatchet(root, 'ratchet-base')
  return snapshot
}

test('single_high_confidence_marker_is_reported', () => {
  const findings = scanText('src/example.rs', `// ${HIGH}\n`)
  assert.deepEqual(findings, [{
    path: 'src/example.rs',
    line: 1,
    rule: 'LANG-PT-001',
    scope: 'comment',
  }])
})

test('leading_and_trailing_comments_are_scanned', () => {
  const leading = scanText('src/example.rs', `// ${HIGH}\n`)
  const trailing = scanText('src/example.rs', `let value = 1; // ${LOW_A} ${LOW_B}\n`)
  const shell = scanText('scripts/example.sh', `echo ready # ${HIGH}\n`)
  assert.equal(leading.length, 1)
  assert.equal(trailing.length, 1)
  assert.equal(shell.length, 1)
  assert.equal(trailing[0].scope, 'comment')
})

test('multiline_comment_state_is_bounded', () => {
  const findings = scanText('src/example.c', [
    '/*',
    ` * ${HIGH}`,
    ' */',
    'const char *label = "ready";',
    '',
  ].join('\n'))
  assert.deepEqual(findings, [{
    path: 'src/example.c',
    line: 2,
    rule: 'LANG-PT-001',
    scope: 'comment',
  }])
  assert.deepEqual(scanText('scripts/example.ps1', [
    '<#',
    HIGH,
    '#>',
    '',
  ].join('\n')), [{
    path: 'scripts/example.ps1',
    line: 2,
    rule: 'LANG-PT-001',
    scope: 'comment',
  }])
})

test('english_comment_and_identifier_are_ignored', () => {
  const findings = scanText('src/example.rs', [
    '// bounded English comment',
    `let ${HIGH} = true;`,
    'let endpoint = "https://example.invalid/ready";',
    '',
  ].join('\n'))
  assert.deepEqual(findings, [])
})

test('quoted_source_text_and_declared_comment_anchors_are_scanned', (t) => {
  const root = repository(t, { 'src/example.ps1': `Write-Error "${HIGH}"\n` })
  const findings = checkFile(path.join(root, 'src/example.ps1'), root)
  assert.deepEqual(findings, [{
    path: 'src/example.ps1',
    line: 1,
    rule: 'LANG-PT-001',
    scope: 'source-text',
  }])
  assert.equal(isCommentLine('# English comment', '.ps1'), true)
  assert.equal(isCommentLine('value # English comment', '.ps1'), false)
  assert.equal(cleanCommentText('/* English comment */', '.c'), 'English comment')
})

test('localized_allowlist_is_accepted', (t) => {
  const root = repository(t, {
    'README.pt-BR.md': `${HIGH}\n`,
    'docs/pt-BR/README.md': `${HIGH}\n`,
  })
  const result = run({ root, mode: 'all' })
  assert.equal(result.ok, true)
  assert.equal(result.counts.finding_count, 0)
})

test('forbidden_localized_path_is_rejected', (t) => {
  const root = repository(t, {
    'README.pt-BR.md': `${HIGH}\n`,
    'docs/example.pt-BR.md': `${HIGH}\n`,
  })
  const result = run({ root, mode: 'all' })
  assert.equal(result.ok, false)
  assert.deepEqual(result.findings, [{
    path: 'docs/example.pt-BR.md',
    line: 1,
    rule: 'LANG-PT-001',
    scope: 'document',
  }])
})

test('new_protected_history_line_is_rejected', (t) => {
  const root = repository(t, { 'validation.md': 'Historical English record\n' })
  write(root, 'validation.md', `Historical English record\n${HIGH}\n`)
  const result = run({ root, mode: 'diff', baseRef: 'HEAD' })
  assert.equal(result.ok, false)
  assert.equal(result.counts.protected_lines, 1)
  assert.deepEqual(result.findings, [{
    path: 'validation.md',
    line: 2,
    rule: 'LANG-PT-001',
    scope: 'document',
  }])
})

test('diff_reports_added_lines_only', (t) => {
  const root = repository(t, { 'src/example.rs': `// ${HIGH}\n` })
  assert.deepEqual(run({ root, mode: 'diff', baseRef: 'HEAD' }).findings, [])
  write(root, 'src/example.rs', `// ${HIGH}\n// English replacement\n`)
  const added = getDiffAddedLines(root, 'HEAD')
  assert.deepEqual([...added.entries()].map(([file, lines]) => [file, [...lines]]), [
    ['src/example.rs', [2]],
  ])
  assert.deepEqual(run({ root, mode: 'diff', baseRef: 'HEAD' }).findings, [])
  write(root, 'src/example.rs', `// ${HIGH}\n// ${HIGH}\n`)
  assert.deepEqual(run({ root, mode: 'diff', baseRef: 'HEAD' }).findings, [{
    path: 'src/example.rs',
    line: 2,
    rule: 'LANG-PT-001',
    scope: 'comment',
  }])
})

test('cli_diff_accepts_english_and_refuses_new_finding', (t) => {
  const root = repository(t, { 'src/example.rs': '// English baseline\n' })
  write(root, 'src/example.rs', '// English replacement\n')
  assert.equal(main(['--diff', 'HEAD'], root, SILENT_IO), 0)
  write(root, 'src/example.rs', `// ${HIGH}\n`)
  assert.equal(main(['--diff', 'HEAD'], root, SILENT_IO), 1)
})

test('localized_diff_is_accepted_by_the_exact_allowlist', (t) => {
  const root = repository(t, { 'README.pt-BR.md': 'Localized baseline\n' })
  write(root, 'README.pt-BR.md', `${HIGH}\n`)
  assert.equal(main(['--diff', 'HEAD'], root, SILENT_IO), 0)
})

test('sanitized_finding_omits_source_and_marker_text', (t) => {
  const secret = 'private-value-42'
  const root = repository(t, { 'unsafe.md': `${secret} ${HIGH}\n` })
  const result = run({ root, mode: 'all' })
  const serialized = JSON.stringify(result)
  const output = formatFinding(result.findings[0])
  assert.match(serialized, /LANG-PT-001/)
  assert.doesNotMatch(serialized, new RegExp(secret))
  assert.doesNotMatch(serialized, new RegExp(HIGH))
  assert.doesNotMatch(output, new RegExp(secret))
  assert.doesNotMatch(output, new RegExp(HIGH))
})

test('output_is_sorted_and_deterministic', (t) => {
  const root = repository(t, {
    'z.md': `${HIGH}\n`,
    'a.md': `${HIGH}\n`,
  })
  const before = readFileSync(path.join(root, 'a.md'), 'utf8')
  const first = run({ root, mode: 'all' })
  const second = run({ root, mode: 'all' })
  assert.deepEqual(first, second)
  assert.deepEqual(first.findings.map((item) => item.path), ['a.md', 'z.md'])
  assert.equal(readFileSync(path.join(root, 'a.md'), 'utf8'), before)
})

test('resource_bound_and_invalid_utf8_fail_closed', () => {
  assert.throws(
    () => scanBuffer('fixture.md', Buffer.from([0xff])),
    (error) => error instanceof LanguageError && error.message === 'invalid-utf8',
  )
  assert.throws(
    () => scanBuffer('fixture.md', Buffer.alloc(MAX_FILE_BYTES + 1, 0x61)),
    (error) => error instanceof LanguageError && error.message === 'file-size-limit',
  )
  assert.throws(
    () => validatePathList(Array.from({ length: 2001 }, (_, index) => `docs/${index}.md`)),
    (error) => error instanceof LanguageError && error.message === 'path-count-limit',
  )
})

test('opaque_protected_inventory_skips_invalid_utf8_but_diff_fails_closed', (t) => {
  const target = 'docs/specs/no-milestone/example/evidence/legacy.txt'
  const root = repository(t, { [target]: 'Historical English record\n' })
  write(root, target, Buffer.from([0xff]))
  assert.throws(
    () => run({ root, mode: 'diff', baseRef: 'HEAD' }),
    (error) => error instanceof LanguageError && error.message === 'invalid-utf8',
  )
  git(root, ['add', '--all'])
  git(root, ['commit', '-qm', 'opaque-fixture'])
  const result = run({ root, mode: 'all' })
  assert.equal(result.ok, true)
  assert.equal(result.counts.finding_count, 0)
})

test('opaque_protected_private_root_redaction_is_byte_exact', (t) => {
  const target = 'docs/specs/no-milestone/example/evidence/legacy.txt'
  const root = repository(t, { [target]: 'placeholder\n' })
  const privateLine = Buffer.from("'\\\\wsl.localhost\\Ubuntu-24.04\\home\\private-user\\repo'\r\n")
  const suffix = Buffer.from([0x43, 0x50, 0x2d, 0x31, 0x32, 0x35, 0x32, 0x3a, 0x20, 0xff, 0x0d, 0x0a])
  write(root, target, Buffer.concat([privateLine, suffix]))
  git(root, ['add', target])
  git(root, ['commit', '-qm', 'opaque-private-root'])

  write(root, target, Buffer.concat([Buffer.from("'<repo-root>'\n"), suffix]))
  assert.equal(run({ root, mode: 'diff', baseRef: 'HEAD' }).ok, true)

  write(root, target, Buffer.concat([Buffer.from("'<repo-root>'\n"), suffix, Buffer.from('changed')]))
  assert.throws(
    () => run({ root, mode: 'diff', baseRef: 'HEAD' }),
    (error) => error instanceof LanguageError && error.message === 'invalid-utf8',
  )
  write(root, target, Buffer.concat([Buffer.from("'<other-root>'\n"), suffix]))
  assert.throws(
    () => run({ root, mode: 'diff', baseRef: 'HEAD' }),
    (error) => error instanceof LanguageError && error.message === 'invalid-utf8',
  )
})

test('invalid_cli_argument_and_base_return_two', (t) => {
  const root = repository(t, { 'README.md': 'English fixture\n' })
  assert.equal(main(['--unknown'], root, SILENT_IO), 2)
  assert.equal(main(['--diff', '--unsafe'], root, SILENT_IO), 2)
  const unknown = spawnSync(process.execPath, [CLI, '--unknown'], { encoding: 'utf8' })
  assert.equal(unknown.status, 2)
})

test('repeated_scan_does_not_modify_fixture_bytes', (t) => {
  const root = repository(t, { 'docs/example.md': `${HIGH}\n` })
  const target = path.join(root, 'docs/example.md')
  const before = readFileSync(target)
  run({ root, mode: 'all' })
  run({ root, mode: 'all' })
  assert.deepEqual(readFileSync(target), before)
})

test('unsafe_path_and_machine_policy_data_are_not_inventory_inputs', () => {
  assert.throws(
    () => validatePathList(['../escape.md']),
    (error) => error instanceof LanguageError && error.message === 'unsafe-path',
  )
  assert.throws(
    () => validatePathList(['C:/escape.md']),
    (error) => error instanceof LanguageError && error.message === 'unsafe-path',
  )
  assert.equal(classifyPath('tools/ci/check-comment-language.mjs'), 'machine-policy-data')
  assert.equal(classifyPath('docs/specs/no-milestone/example/IMPL.md'), 'protected')
})

test('symlink_escape_fails_closed', (t) => {
  const root = repository(t, { 'README.md': 'English fixture\n' })
  const outside = mkdtempSync(path.join(tmpdir(), 'ramshared-comment-language-outside-'))
  t.after(() => rmSync(outside, { recursive: true, force: true }))
  write(outside, 'escape.md', `${HIGH}\n`)
  symlinkSync(path.join(outside, 'escape.md'), path.join(root, 'escape.md'))
  git(root, ['add', 'escape.md'])
  git(root, ['commit', '-qm', 'symlink-fixture'])
  assert.throws(
    () => run({ root, mode: 'all' }),
    (error) => error instanceof LanguageError && error.message === 'unsafe-symlink',
  )
})

test('ratchet_rejects_growth_and_accepts_strict_decrease', (t) => {
  const root = repository(t, {
    'src/example.rs': `// ${HIGH}\n// ${HIGH}\n`,
  })
  const base = establishRatchetBase(root)

  write(root, 'src/example.rs', `// ${HIGH}\n// ${HIGH}\n// ${HIGH}\n`)
  const grown = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(base, grown, 1))
  assert.deepEqual(runRatchet({ root, baseRef: 'HEAD' }), {
    ok: false,
    code: 'ratchet-mutable-growth',
    counts: grown,
    revision: 1,
    terminal: false,
  })

  write(root, 'src/example.rs', `// ${HIGH}\n`)
  const reduced = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(base, reduced, 1))
  assert.equal(runRatchet({ root, baseRef: 'HEAD' }).ok, true)
  assert.equal(main(['--ratchet', 'HEAD'], root, SILENT_IO), 0)
})

test('ratchet_uses_base_record_not_pr_record', (t) => {
  const root = repository(t, { 'src/example.rs': `// ${HIGH}\n` })
  const base = establishRatchetBase(root)
  write(root, 'src/example.rs', '// English replacement\n')
  const current = inventorySnapshot(root)
  const alteredInitial = copy(base)
  alteredInitial.mutable_lines = 0
  writeRatchet(root, ratchetRecord(alteredInitial, current, 1))
  assert.equal(runRatchet({ root, baseRef: 'HEAD' }).code, 'ratchet-initial-changed')
})

test('ratchet_refuses_protected_inventory_drift', (t) => {
  const root = repository(t, {
    'src/example.rs': `// ${HIGH}\n`,
    'validation.md': 'Historical English record\n',
  })
  const base = establishRatchetBase(root)
  write(root, 'src/example.rs', '// English replacement\n')
  write(root, 'validation.md', 'Changed English record\n')
  const current = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(base, current, 1))
  assert.equal(runRatchet({ root, baseRef: 'HEAD' }).code, 'ratchet-protected-drift')
})

test('ratchet_schema_rejects_suppression_capability', (t) => {
  const root = repository(t, { 'src/example.rs': `// ${HIGH}\n` })
  const snapshot = establishRatchetBase(root)
  const record = ratchetRecord(snapshot, snapshot)
  record.ignored_paths = []
  writeRatchet(root, record)
  assert.throws(
    () => runRatchet({ root, baseRef: 'HEAD' }),
    (error) => error instanceof LanguageError && error.message === 'invalid-ratchet-baseline',
  )
})

test('ratchet_schema_file_is_strict_and_suppression_free', () => {
  const schema = JSON.parse(readFileSync(
    path.join(ROOT, 'tools/ci/comment-language-baseline.schema.json'),
    'utf8',
  ))
  assert.equal(schema.additionalProperties, false)
  assert.deepEqual(
    Object.keys(schema.properties).sort(),
    ['$schema', 'schema', 'revision', 'approval', 'initial', 'current'].sort(),
  )
  assert.equal(schema.$defs.approval.additionalProperties, false)
  assert.equal(schema.$defs.snapshot.additionalProperties, false)
  assert.equal(Object.hasOwn(schema.properties, 'ignored_paths'), false)
  assert.equal(Object.hasOwn(schema.$defs.snapshot.properties, 'paths'), false)
})

test('batch_limit_rejects_more_than_ten_files_or_hundred_lines', (t) => {
  const files = {}
  for (let index = 0; index < 11; index++) files[`src/${index}.rs`] = `// ${HIGH}\n`
  const root = repository(t, files)
  const fileBase = establishRatchetBase(root)
  for (let index = 0; index < 11; index++) write(root, `src/${index}.rs`, '// English replacement\n')
  const fileCurrent = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(fileBase, fileCurrent, 1))
  assert.equal(runRatchet({ root, baseRef: 'HEAD' }).code, 'ratchet-batch-file-limit')

  const lineRoot = repository(t, {
    'src/example.rs': Array.from({ length: 101 }, () => `// ${HIGH}`).join('\n'),
  })
  const lineBase = establishRatchetBase(lineRoot)
  write(lineRoot, 'src/example.rs', '// English replacement\n')
  const lineCurrent = inventorySnapshot(lineRoot)
  writeRatchet(lineRoot, ratchetRecord(lineBase, lineCurrent, 1))
  assert.equal(runRatchet({ root: lineRoot, baseRef: 'HEAD' }).code, 'ratchet-batch-line-limit')
})

test('zero_mutable_findings_is_terminal_state', (t) => {
  const root = repository(t, { 'src/example.rs': `// ${HIGH}\n` })
  const base = establishRatchetBase(root)
  write(root, 'src/example.rs', '// English replacement\n')
  const current = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(base, current, 1))
  const result = runRatchet({ root, baseRef: 'HEAD' })
  assert.equal(result.ok, true)
  assert.equal(result.terminal, true)
})

test('ratchet_requires_anchored_base_record_before_activation', (t) => {
  const root = repository(t, { 'src/example.rs': `// ${HIGH}\n` })
  const snapshot = inventorySnapshot(root)
  writeRatchet(root, ratchetRecord(snapshot, snapshot))
  assert.throws(
    () => runRatchet({ root, baseRef: 'HEAD' }),
    (error) => error instanceof LanguageError && error.message === 'ratchet-baseline-missing',
  )
  assert.equal(main(['--ratchet', 'HEAD'], root, SILENT_IO), 2)
})

test('bootstrap_requires_the_reviewed_snapshot_and_protocol', () => {
  const bootstrap = {
    mutable_files: 2,
    mutable_lines: 3,
    protected_files: 1,
    protected_lines: 2,
    protected_paths: 4,
    protected_inventory_sha256: 'a'.repeat(64),
  }
  const record = ratchetRecord(bootstrap, bootstrap)
  assert.throws(
    () => evaluateRatchetTransition({
      baseRecord: null,
      currentRecord: record,
      observed: bootstrap,
    }),
    (error) => error instanceof LanguageError && error.message === 'ratchet-bootstrap-not-authorized',
  )
  assert.equal(evaluateRatchetTransition({
    baseRecord: null,
    currentRecord: record,
    observed: bootstrap,
    bootstrapSnapshot: bootstrap,
  }).ok, true)
  record.current.mutable_lines = 2
  assert.equal(evaluateRatchetTransition({
    baseRecord: null,
    currentRecord: record,
    observed: bootstrap,
    bootstrapSnapshot: bootstrap,
  }).code, 'ratchet-snapshot-mismatch')
})
