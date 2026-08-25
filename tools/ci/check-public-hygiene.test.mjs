import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { deflateSync } from 'node:zlib'

import {
  classifyText,
  enumerateFiles,
  isSafeRepoPath,
  run,
  scanDocumentActivation,
  scanText,
} from './check-public-hygiene.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const CLI = path.join(ROOT, 'tools/ci/check-public-hygiene.mjs')
const AS_OF = new Date('2026-08-22T00:00:00Z')

function git(root, args) { return execFileSync('git', args, { cwd: root, encoding: 'utf8' }) }
function lineHash(line) { return createHash('sha256').update(`${line}\n`).digest('hex') }

function repo() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-public-hygiene-'))
  git(root, ['init', '-q'])
  git(root, ['config', 'user.name', 'RamShared Fixture'])
  git(root, ['config', 'user.email', ['fixture', 'example.invalid'].join('@')])
  writeFileSync(path.join(root, 'README.md'), '# Safe fixture\n')
  git(root, ['add', 'README.md'])
  git(root, ['commit', '-qm', 'fixture'])
  return root
}

function emptyRepo() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-public-hygiene-empty-'))
  git(root, ['init', '-q'])
  git(root, ['config', 'user.name', 'RamShared Fixture'])
  git(root, ['config', 'user.email', ['fixture', 'example.invalid'].join('@')])
  return root
}

function commitAll(root, message = 'fixture update') {
  git(root, ['add', '-A'])
  git(root, ['commit', '-qm', message])
}

function privateUnixPath() { return ['', 'home', 'private-user', 'workspace', 'artifact.txt'].join('/') }
function rawArtifactPath() { return ['C:', 'ramshared', 'artifacts', 'run-20260822-123456'].join('\\') }
function hyperVPath() { return ['E:', 'Hyper-V', 'Virtual Hard Disks', 'lab.vhdx'].join('\\') }
function rawSdDevice() { return ['/dev', 'sdc'].join('/') }
function rawNvmeDevice() { return ['/dev', 'nvme0n1p3'].join('/') }
function rawMapperDevice() { return ['/dev', 'mapper', 'cryptswap'].join('/') }
function rawPrincipal() { return ['win11-drill', 'operator'].join('\\') }
function rawPrivateIp() { return [192, 168, 50, 12].join('.') }
function rawRunId() { return ['3b13195c', '4e40', '4467', 'b079', '9ac228aa8bf1'].join('-') }
function timestampRun() { return ['pressure-run', '20260822-123456'].join('-') }
function knownTimestampRun(prefix) { return [prefix, '20260822-123456'].join('-') }
function rawArtifactRunPath() { return ['', 'tmp', 'ramshared', 'artifact-run'].join('/') }
function activationCommand() { return ['sudo', 'mkswap', 'SANITIZED_DEVICE_ORIGIN'].join(' ') }
function fenced(command, warning = '') { return [warning, '```bash', command, '```'].filter(Boolean).join('\n') }
function noExecutionWarning() { return '> **Historical non-current / no execution:** retained evidence only; do not execute.' }
function sanitizedActivationWarning() { return '> **Disabled staging only / no execution:** inert definition; not a current activation.' }
function unicode(codePoint) { return String.fromCodePoint(codePoint) }
function writePublicCandidate(root, file, text) {
  mkdirSync(path.dirname(path.join(root, file)), { recursive: true })
  writeFileSync(path.join(root, file), text)
}

const BINARY_DIGEST_SCHEMA = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $id: 'public-binary-digests.schema.json',
  title: 'RamShared reviewed public JPEG digest manifest',
  type: 'object',
  additionalProperties: false,
  required: ['$schema', 'schema_version', 'entries'],
  properties: {
    $schema: { const: './public-binary-digests.schema.json' },
    schema_version: { const: 'ramshared-public-binary-digests/v1' },
    entries: {
      type: 'array',
      maxItems: 256,
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['path', 'size', 'sha256'],
        properties: {
          path: { type: 'string', pattern: '^docs/.+\\.jpe?g$' },
          size: { type: 'integer', minimum: 1, maximum: 8388608 },
          sha256: { type: 'string', pattern: '^[0-9a-f]{64}$' },
        },
      },
    },
  },
}

function binaryDigestEntry(file, buffer) {
  return {
    path: file,
    size: buffer.length,
    sha256: createHash('sha256').update(buffer).digest('hex'),
  }
}

function writeBinaryDigestContract(root, entries, { schema = BINARY_DIGEST_SCHEMA } = {}) {
  for (const entry of entries) {
    try {
      readFileSync(path.join(root, entry.path))
      git(root, ['add', '--', entry.path])
    } catch {
      // Invalid-path manifest fixtures deliberately have no stageable asset.
    }
  }
  writePublicCandidate(root, 'docs/governance/public-binary-digests.schema.json', `${JSON.stringify(schema, null, 2)}\n`)
  writePublicCandidate(root, 'docs/governance/public-binary-digests.json', `${JSON.stringify({
    $schema: './public-binary-digests.schema.json',
    schema_version: 'ramshared-public-binary-digests/v1',
    entries,
  }, null, 2)}\n`)
  git(root, ['add', '--',
    'docs/governance/public-binary-digests.schema.json',
    'docs/governance/public-binary-digests.json',
  ])
}

function fixtureCrc32(buffer) {
  let crc = 0xffffffff
  for (const byte of buffer) {
    crc ^= byte
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1))
  }
  return (crc ^ 0xffffffff) >>> 0
}

function pngChunk(type, data = Buffer.alloc(0)) {
  const typeBytes = Buffer.from(type, 'ascii')
  const chunk = Buffer.alloc(12 + data.length)
  chunk.writeUInt32BE(data.length, 0)
  typeBytes.copy(chunk, 4)
  data.copy(chunk, 8)
  chunk.writeUInt32BE(fixtureCrc32(Buffer.concat([typeBytes, data])), 8 + data.length)
  return chunk
}

function pngHeader({
  width = 1,
  height = 1,
  bitDepth = 8,
  colorType = 6,
  compression = 0,
  filter = 0,
  interlace = 0,
} = {}) {
  const header = Buffer.alloc(13)
  header.writeUInt32BE(width, 0)
  header.writeUInt32BE(height, 4)
  header.set([bitDepth, colorType, compression, filter, interlace], 8)
  return header
}

function pngFixture({ header = pngHeader(), raw = Buffer.from([0, 0, 0, 0, 0]), idat, chunks } = {}) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
  const body = chunks ?? [pngChunk('IHDR', header), pngChunk('IDAT', idat ?? deflateSync(raw)), pngChunk('IEND')]
  return Buffer.concat([signature, ...body])
}

function jpegSegment(marker, data) {
  const segment = Buffer.alloc(4 + data.length)
  segment.set([0xff, marker], 0)
  segment.writeUInt16BE(data.length + 2, 2)
  data.copy(segment, 4)
  return segment
}

function minimalBaselineJpeg({
  entropy = Buffer.from([0x3f]),
  includeDqt = true,
  includeDht = true,
  frameMarker = 0xc0,
  precision = 8,
  scanComponent = 1,
  restartInterval = null,
  metadata = [],
} = {}) {
  const jfif = jpegSegment(0xe0, Buffer.from([
    0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
    0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
  ]))
  const dqt = jpegSegment(0xdb, Buffer.concat([Buffer.from([0x00]), Buffer.alloc(64, 1)]))
  const oneCode = Buffer.from([1, ...Array(15).fill(0)])
  const dht = jpegSegment(0xc4, Buffer.concat([
    Buffer.from([0x00]), oneCode, Buffer.from([0x00]),
    Buffer.from([0x10]), oneCode, Buffer.from([0x00]),
  ]))
  const sof = jpegSegment(frameMarker, Buffer.from([
    precision, 0x00, 0x01, 0x00, restartInterval === null ? 0x01 : 0x10,
    0x01, 0x01, 0x11, 0x00,
  ]))
  const sos = jpegSegment(0xda, Buffer.from([0x01, scanComponent, 0x00, 0x00, 0x3f, 0x00]))
  const segments = [Buffer.from([0xff, 0xd8]), jfif, ...metadata]
  if (includeDqt) segments.push(dqt)
  segments.push(sof)
  if (includeDht) segments.push(dht)
  if (restartInterval !== null) segments.push(jpegSegment(0xdd, Buffer.from([0x00, restartInterval])))
  segments.push(sos, entropy, Buffer.from([0xff, 0xd9]))
  return Buffer.concat(segments)
}

test('candidate_scans_nonignored_untracked_file', () => {
  const root = repo()
  writeFileSync(path.join(root, 'new.md'), `artifact=${privateUnixPath()}\n`)
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.some((item) => item.path === 'new.md' && item.rule === 'PRIVATE_UNIX_PATH'), true)
})

test('staged_reads_index_blob_not_worktree', () => {
  const root = repo()
  writeFileSync(path.join(root, 'README.md'), `artifact=${privateUnixPath()}\n`)
  git(root, ['add', 'README.md'])
  writeFileSync(path.join(root, 'README.md'), '# Sanitized worktree copy\n')
  assert.equal(run({ root, mode: 'staged', asOf: AS_OF }).findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), true)
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).ok, true)
})

test('candidate_binds_one_head_oid_when_symbolic_head_moves_mid_run', () => {
  const root = emptyRepo()
  writePublicCandidate(root, 'docs/root-activation.md', `${activationCommand()}\n`)
  commitAll(root, 'root candidate')
  const originalHead = git(root, ['rev-parse', 'HEAD']).trim()
  let snapshotHookRan = false

  const result = run({
    root,
    mode: 'candidate',
    asOf: AS_OF,
    afterGitSnapshot() {
      git(root, ['commit', '--allow-empty', '-qm', 'move symbolic head'])
      snapshotHookRan = true
    },
  })

  assert.equal(snapshotHookRan, true)
  assert.notEqual(git(root, ['rev-parse', 'HEAD']).trim(), originalHead)
  assert.equal(result.findings.some((item) =>
    item.path === 'docs/root-activation.md' && item.rule === 'UNGUARDED_ACTIVATION'), true)
})

test('staged_index_snapshot_refuses_mid_run_index_move', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/index-snapshot.txt', 'portable staged value\n')
  git(root, ['add', '--', 'docs/index-snapshot.txt'])

  assert.throws(() => run({
    root,
    mode: 'staged',
    asOf: AS_OF,
    afterGitSnapshot() {
      writePublicCandidate(root, 'docs/index-snapshot.txt', `${privateUnixPath()}\n`)
      git(root, ['add', '--', 'docs/index-snapshot.txt'])
    },
  }), /git-index-snapshot-changed/)
})

test('candidate_path_rejections_precede_any_read_and_symlinks_never_expose_targets', () => {
  const root = repo()
  const outside = path.join(mkdtempSync(path.join(tmpdir(), 'ramshared-outside-')), 'secret.md')
  writeFileSync(outside, privateUnixPath())
  mkdirSync(path.join(root, 'docs'))
  symlinkSync(outside, path.join(root, 'docs', 'escaped.md'))
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.some((item) => item.rule === 'PUBLIC_SYMLINK_ESCAPE'), true)
  assert.equal(result.findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), false)
  assert.equal(isSafeRepoPath(root, 'docs/safe.md'), true)
  assert.equal(isSafeRepoPath(root, '../outside.md'), false)
  assert.equal(isSafeRepoPath(root, 'docs\\outside.md'), false)
  assert.equal(isSafeRepoPath(root, `/docs/bad${String.fromCharCode(7)}.md`), false)
  assert.equal(isSafeRepoPath(root, `/docs/bad${String.fromCharCode(127)}.md`), false)
  assert.equal(isSafeRepoPath(root, `docs/bad${String.fromCharCode(0x85)}.md`), false)
  assert.equal(isSafeRepoPath(root, `docs/bad\u202e.md`), false)
})

test('candidate_staged_and_tracked_symlinks_scan_publishable_link_text_without_following_targets', () => {
  const root = repo()
  mkdirSync(path.join(root, 'docs'), { recursive: true })
  writeFileSync(path.join(root, '.gitignore'), 'internal-target\n')
  writeFileSync(path.join(root, 'internal-target'), privateUnixPath())
  symlinkSync('../internal-target', path.join(root, 'docs', 'link.txt'))

  const candidate = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(candidate.ok, true, JSON.stringify(candidate.findings, null, 2))
  git(root, ['add', '.gitignore', 'docs/link.txt'])

  unlinkSync(path.join(root, 'docs', 'link.txt'))
  writeFileSync(path.join(root, 'docs', 'link.txt'), privateUnixPath())
  const staged = run({ root, mode: 'staged', asOf: AS_OF })
  assert.equal(staged.ok, true, JSON.stringify(staged.findings, null, 2))
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), true)

  unlinkSync(path.join(root, 'docs', 'link.txt'))
  symlinkSync('../internal-target', path.join(root, 'docs', 'link.txt'))
  commitAll(root, 'internal symlink')
  assert.equal(run({ root, mode: 'tracked', asOf: AS_OF }).ok, true)

  const externalRoot = repo()
  mkdirSync(path.join(externalRoot, 'docs'), { recursive: true })
  const outside = path.join(mkdtempSync(path.join(tmpdir(), 'ramshared-external-target-')), 'private.txt')
  writeFileSync(outside, privateUnixPath())
  symlinkSync(outside, path.join(externalRoot, 'docs', 'external.txt'))
  for (const mode of ['candidate', 'staged']) {
    if (mode === 'staged') git(externalRoot, ['add', 'docs/external.txt'])
    const result = run({ root: externalRoot, mode, asOf: AS_OF })
    assert.equal(result.findings.some((item) => item.rule === 'PUBLIC_SYMLINK_ESCAPE'), true, mode)
    assert.equal(result.findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), false, mode)
  }
  commitAll(externalRoot, 'external symlink')
  for (const mode of ['candidate', 'tracked']) {
    const result = run({ root: externalRoot, mode, asOf: AS_OF })
    assert.equal(result.findings.some((item) => item.rule === 'PUBLIC_SYMLINK_ESCAPE'), true, mode)
    assert.equal(result.findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), false, mode)
  }
})

test('literal_backslash_and_control_git_paths_are_deterministically_rejected', () => {
  const root = repo()
  writeFileSync(path.join(root, 'ambiguous\\name.md'), '# harmless\n')
  writeFileSync(path.join(root, `bell${String.fromCharCode(7)}.md`), '# harmless\n')
  writeFileSync(path.join(root, `del${String.fromCharCode(127)}.md`), '# harmless\n')
  writeFileSync(path.join(root, `c1${String.fromCharCode(0x85)}.md`), '# harmless\n')
  writeFileSync(path.join(root, 'bidi\u202e.md'), '# harmless\n')
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.filter((item) => item.rule === 'UNSAFE_PATH').length, 5)
  assert.equal(result.findings.every((item) => item.path === '<invalid-path>'), true)
})

test('extensionless_shebang_is_scanned_and_binary_blob_is_skipped', () => {
  const root = repo()
  writeFileSync(path.join(root, 'runner'), `#!/usr/bin/env bash\necho ${privateUnixPath()}\n`)
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).findings.some((item) => item.path === 'runner'), true)
  assert.equal(classifyText(Buffer.from([0x89, 0x50, 0x00, 0x47]), 'asset.bin'), false)
  writeFileSync(path.join(root, '.gitignore'), 'local-secret.txt\n')
  writeFileSync(path.join(root, 'local-secret.txt'), privateUnixPath())
  assert.equal(enumerateFiles(root, 'candidate').includes('local-secret.txt'), false)
})

test('candidate_public_content_controls_refuse_bidi_identity_bell_identity_activation_and_json', () => {
  const root = repo()
  const bidi = unicode(0x202e)
  const bell = unicode(0x0007)
  const splitLabIdentity = ['linux-', 'kernel-lab'].join(bidi)
  const splitActivation = ['mk', 'swap SANITIZED_DEVICE_ORIGIN'].join(bidi)
  writePublicCandidate(root, 'docs/bidi-identity.md', splitLabIdentity)
  writePublicCandidate(root, 'docs/bell-identity.md', `${timestampRun()}${bell}`)
  writePublicCandidate(root, 'docs/bidi-activation.md', splitActivation)
  writePublicCandidate(root, 'docs/catalog.json', JSON.stringify({ historical_identity: splitLabIdentity }))

  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.ok, false)
  assert.deepEqual(
    result.findings
      .filter((item) => item.rule === 'UNSAFE_UNICODE_CONTENT')
      .map((item) => [item.path, item.line, item.reason]),
    [
      ['docs/bell-identity.md', 1, 'unsafe-unicode-content-u+0007'],
      ['docs/bidi-activation.md', 1, 'unsafe-unicode-content-u+202e'],
      ['docs/bidi-identity.md', 1, 'unsafe-unicode-content-u+202e'],
      ['docs/catalog.json', 1, 'unsafe-unicode-content-u+202e'],
    ],
  )
})

test('candidate_public_content_allows_normal_line_tab_and_sanitized_text', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/ordinary.md', 'SANITIZED_VM_LAB\tretained history\n')
  writePublicCandidate(root, 'docs/catalog.json', '{"value":"SANITIZED_RUN_SLOT_A\\tretained"}\n')
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.ok, true, JSON.stringify(result.findings, null, 2))
})

test('leading_and_interior_bom_content_and_bom_prefixed_git_path_collision_refuse', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/leading.txt', `${unicode(0xfeff)}retained evidence\n`)
  writePublicCandidate(root, 'docs/interior.svg', `<svg><text>retained${unicode(0xfeff)} evidence</text></svg>\n`)
  const contentResult = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.deepEqual(
    contentResult.findings
      .filter((item) => item.rule === 'UNSAFE_UNICODE_CONTENT')
      .map((item) => [item.path, item.reason]),
    [
      ['docs/interior.svg', 'unsafe-unicode-content-u+feff'],
      ['docs/leading.txt', 'unsafe-unicode-content-u+feff'],
    ],
  )

  const pathRoot = emptyRepo()
  writeFileSync(path.join(pathRoot, `${unicode(0xfeff)}README.md`), '# collision candidate\n')
  commitAll(pathRoot, 'bom path')
  const pathResult = run({ root: pathRoot, mode: 'candidate', asOf: AS_OF })
  assert.deepEqual(pathResult.findings, [{
    path: '<invalid-path>', line: 1, rule: 'UNSAFE_PATH', reason: 'unsafe-git-path',
  }])
})

test('candidate_public_markdown_refuses_invalid_utf8_without_binary_skip', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/invalid.md', Buffer.from([0xc3, 0x28]))
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.deepEqual(result.findings, [{
    path: 'docs/invalid.md', line: 1,
    rule: 'UNSAFE_PUBLIC_TEXT_ENCODING', reason: 'public-text-invalid-utf8',
  }])
})

test('clean_checkout_committed_public_text_extensions_fail_closed_on_encoding_bidi_and_c0_controls', () => {
  const root = repo()
  const bidi = unicode(0x202e)
  const bell = unicode(0x0007)
  writePublicCandidate(root, 'docs/invalid.txt', Buffer.from([0xc3, 0x28]))
  writePublicCandidate(root, 'docs/invalid.svg', Buffer.from([0xc3, 0x28]))
  writePublicCandidate(root, 'docs/bidi.txt', ['linux-', 'kernel-lab'].join(bidi))
  writePublicCandidate(root, 'docs/bidi.svg', `<svg><text>${['mk', 'swap SANITIZED_DEVICE_ORIGIN'].join(bidi)}</text></svg>`)
  writePublicCandidate(root, 'docs/control.txt', `retained evidence${bell}`)
  writePublicCandidate(root, 'docs/control.svg', `<svg><text>retained evidence${bell}</text></svg>`)
  commitAll(root, 'unsafe public text candidates')

  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.ok, false)
  assert.deepEqual(
    result.findings.map((item) => [item.path, item.rule, item.reason]),
    [
      ['docs/bidi.svg', 'UNSAFE_UNICODE_CONTENT', 'unsafe-unicode-content-u+202e'],
      ['docs/bidi.txt', 'UNSAFE_UNICODE_CONTENT', 'unsafe-unicode-content-u+202e'],
      ['docs/control.svg', 'UNSAFE_UNICODE_CONTENT', 'unsafe-unicode-content-u+0007'],
      ['docs/control.txt', 'UNSAFE_UNICODE_CONTENT', 'unsafe-unicode-content-u+0007'],
      ['docs/invalid.svg', 'UNSAFE_PUBLIC_TEXT_ENCODING', 'public-text-invalid-utf8'],
      ['docs/invalid.txt', 'UNSAFE_PUBLIC_TEXT_ENCODING', 'public-text-invalid-utf8'],
    ],
  )
})

test('candidate_clean_root_commit_and_detached_head_scan_the_committed_tree_delta', () => {
  const root = emptyRepo()
  writePublicCandidate(root, 'docs/root-invalid.txt', Buffer.from([0xc3, 0x28]))
  commitAll(root, 'root candidate')
  for (const state of ['attached', 'detached']) {
    if (state === 'detached') git(root, ['checkout', '--detach', '-q'])
    const result = run({ root, mode: 'candidate', asOf: AS_OF })
    assert.equal(result.findings.some((item) =>
      item.path === 'docs/root-invalid.txt' && item.rule === 'UNSAFE_PUBLIC_TEXT_ENCODING'), true, state)
  }
})

test('candidate_clean_merge_commit_uses_the_first_parent_delta', () => {
  const root = repo()
  git(root, ['checkout', '-qb', 'unsafe-topic'])
  writePublicCandidate(root, 'docs/merged-invalid.svg', Buffer.from([0xc3, 0x28]))
  commitAll(root, 'topic public candidate')
  git(root, ['checkout', '-q', '-'])
  writeFileSync(path.join(root, 'main-only.txt'), 'safe main parent\n')
  commitAll(root, 'main parent')
  git(root, ['merge', '--no-ff', '-qm', 'merge fixture', 'unsafe-topic'])
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.some((item) =>
    item.path === 'docs/merged-invalid.svg' && item.rule === 'UNSAFE_PUBLIC_TEXT_ENCODING'), true)
})

test('candidate_rename_rejects_unsafe_source_or_target_paths_and_allows_safe_deletion', () => {
  const unsafeSourceRoot = repo()
  mkdirSync(path.join(unsafeSourceRoot, 'docs'), { recursive: true })
  const unsafeSource = `docs/${unicode(0x202e)}source.txt`
  writeFileSync(path.join(unsafeSourceRoot, unsafeSource), 'safe content\n')
  commitAll(unsafeSourceRoot, 'unsafe source fixture')
  git(unsafeSourceRoot, ['mv', unsafeSource, 'docs/safe-target.txt'])
  commitAll(unsafeSourceRoot, 'rename unsafe source')
  assert.equal(run({ root: unsafeSourceRoot, mode: 'candidate', asOf: AS_OF })
    .findings.some((item) => item.rule === 'UNSAFE_PATH' && item.reason === 'unsafe-git-path'), true)

  const unsafeTargetRoot = repo()
  mkdirSync(path.join(unsafeTargetRoot, 'docs'), { recursive: true })
  writeFileSync(path.join(unsafeTargetRoot, 'docs', 'safe-source.txt'), 'safe content\n')
  commitAll(unsafeTargetRoot, 'safe source fixture')
  git(unsafeTargetRoot, ['mv', 'docs/safe-source.txt', `docs/${unicode(0xfeff)}target.txt`])
  commitAll(unsafeTargetRoot, 'rename unsafe target')
  assert.equal(run({ root: unsafeTargetRoot, mode: 'candidate', asOf: AS_OF })
    .findings.some((item) => item.rule === 'UNSAFE_PATH' && item.reason === 'unsafe-git-path'), true)

  const deletionRoot = repo()
  writePublicCandidate(deletionRoot, 'docs/deleted-invalid.txt', Buffer.from([0xc3, 0x28]))
  commitAll(deletionRoot, 'historical invalid file')
  unlinkSync(path.join(deletionRoot, 'docs', 'deleted-invalid.txt'))
  assert.equal(run({ root: deletionRoot, mode: 'candidate', asOf: AS_OF }).ok, true)
  git(deletionRoot, ['add', '-u'])
  assert.equal(run({ root: deletionRoot, mode: 'staged', asOf: AS_OF }).ok, true)
  commitAll(deletionRoot, 'remove invalid file')
  assert.equal(run({ root: deletionRoot, mode: 'candidate', asOf: AS_OF }).ok, true)
})

test('candidate_and_staged_modes_separate_mixed_index_and_worktree_truth', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/index.txt', privateUnixPath())
  git(root, ['add', 'docs/index.txt'])
  writePublicCandidate(root, 'docs/index.txt', 'sanitized worktree\n')
  writePublicCandidate(root, 'docs/worktree.txt', privateUnixPath())

  const staged = run({ root, mode: 'staged', asOf: AS_OF })
  assert.equal(staged.findings.some((item) => item.path === 'docs/index.txt' && item.rule === 'PRIVATE_UNIX_PATH'), true)
  assert.equal(staged.findings.some((item) => item.path === 'docs/worktree.txt'), false)
  const candidate = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(candidate.findings.some((item) => item.path === 'docs/index.txt' && item.rule === 'PRIVATE_UNIX_PATH'), false)
  assert.equal(candidate.findings.some((item) => item.path === 'docs/worktree.txt' && item.rule === 'PRIVATE_UNIX_PATH'), true)
})

test('strict_whole_artifact_controls_and_encoding_are_independent_of_added_line_scoping', () => {
  const root = repo()
  const bell = unicode(0x0007)
  writePublicCandidate(root, 'docs/history.txt', `${rawSdDevice()}${bell}\n`)
  writePublicCandidate(root, 'docs/history.svg', Buffer.from([0xc3, 0x28]))
  commitAll(root, 'historical public bytes')
  writePublicCandidate(root, 'docs/history.txt', `${rawSdDevice()}${bell}\nnew safe line\n`)
  writePublicCandidate(root, 'docs/history.svg', Buffer.from([0xc3, 0x28, 0x0a, 0x78, 0x0a]))
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.some((item) => item.path === 'docs/history.txt' && item.rule === 'UNSAFE_UNICODE_CONTENT'), true)
  assert.equal(result.findings.some((item) => item.path === 'docs/history.txt' && item.rule === 'RAW_DEVICE'), false)
  assert.equal(result.findings.some((item) => item.path === 'docs/history.svg' && item.rule === 'UNSAFE_PUBLIC_TEXT_ENCODING'), true)
})

test('candidate_git_topology_errors_fail_closed_instead_of_reporting_zero_files', () => {
  const root = emptyRepo()
  assert.throws(() => run({ root, mode: 'candidate', asOf: AS_OF }), /git-query-failed/)
  const cli = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  assert.equal(cli.status, 2)
  assert.match(cli.stderr, /PUBLIC_HYGIENE_ERROR=git-query-failed/)
})

test('candidate_public_binary_assets_require_bounded_extension_and_magic_contracts', () => {
  const root = repo()
  const minimalPng = pngFixture()
  const minimalJpeg = minimalBaselineJpeg()
  writePublicCandidate(root, 'docs/logo.png', minimalPng)
  writePublicCandidate(root, 'docs/photo.jpg', minimalJpeg)
  writeBinaryDigestContract(root, [binaryDigestEntry('docs/photo.jpg', minimalJpeg)])
  commitAll(root, 'reviewed binary assets')
  writeFileSync(path.join(root, 'README.md'), '# Safe fixture update\n')
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).ok, true)

  writePublicCandidate(root, 'docs/masquerade.png', Buffer.from('not a png\n'))
  writePublicCandidate(root, 'docs/bookended.png', Buffer.concat([
    minimalPng.subarray(0, 8), Buffer.from(privateUnixPath()), minimalPng.subarray(-12),
  ]))
  writePublicCandidate(root, 'docs/bookended.jpg', Buffer.concat([
    Buffer.from([0xff, 0xd8, 0xff]), Buffer.from(privateUnixPath()), Buffer.from([0xff, 0xd9]),
  ]))
  writePublicCandidate(root, 'docs/confused.jpg', minimalPng)
  writePublicCandidate(root, 'docs/confused.png', minimalJpeg)
  writePublicCandidate(root, 'docs/truncated.jpg', Buffer.from([0xff, 0xd8, 0xff]))
  writePublicCandidate(root, 'docs/truncated.png', Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
  writePublicCandidate(root, 'docs/oversize.jpg', Buffer.alloc(8 * 1024 * 1024 + 1))
  const findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  const reason = (file) => findings.find((item) => item.path === file)?.reason
  assert.equal(reason('docs/bookended.jpg'), 'public-jpeg-structure-invalid')
  assert.equal(reason('docs/bookended.png'), 'public-png-chunk-invalid')
  assert.equal(reason('docs/confused.jpg'), 'public-binary-signature-mismatch')
  assert.equal(reason('docs/confused.png'), 'public-binary-signature-mismatch')
  assert.equal(reason('docs/masquerade.png'), 'public-binary-signature-mismatch')
  assert.equal(reason('docs/oversize.jpg'), 'public-binary-size-limit')
  assert.equal(reason('docs/truncated.jpg'), 'public-binary-signature-mismatch')
  assert.equal(reason('docs/truncated.png'), 'public-binary-signature-mismatch')
})

test('png_requires_bounded_zlib_exact_scanlines_filters_and_consecutive_idat', () => {
  const root = repo()
  const compressed = deflateSync(Buffer.from([0, 1, 2, 3, 4]))
  const split = Math.floor(compressed.length / 2)
  writePublicCandidate(root, 'docs/valid.png', pngFixture())
  writePublicCandidate(root, 'docs/concatenated.png', pngFixture({
    chunks: [
      pngChunk('IHDR', pngHeader()),
      pngChunk('IDAT', compressed.subarray(0, split)),
      pngChunk('IDAT', compressed.subarray(split)),
      pngChunk('IEND'),
    ],
  }))
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).ok, true)

  const privateIdat = pngFixture({ idat: Buffer.from(privateUnixPath()) })
  writePublicCandidate(root, 'docs/raw-private-idat.png', privateIdat)
  writePublicCandidate(root, 'docs/invalid-zlib.png', pngFixture({ idat: Buffer.from([0x78, 0x9c, 0x00]) }))
  writePublicCandidate(root, 'docs/trailing-zlib.png', pngFixture({
    idat: Buffer.concat([deflateSync(Buffer.from([0, 0, 0, 0, 0])), Buffer.from('trailing')]),
  }))
  writePublicCandidate(root, 'docs/wrong-size.png', pngFixture({ raw: Buffer.from([0]) }))
  writePublicCandidate(root, 'docs/wrong-filter.png', pngFixture({ raw: Buffer.from([5, 0, 0, 0, 0]) }))
  writePublicCandidate(root, 'docs/bomb.png', pngFixture({ raw: Buffer.alloc(1024 * 1024) }))
  const findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  const reason = (file) => findings.find((item) => item.path === file)?.reason
  assert.equal(reason('docs/raw-private-idat.png'), 'public-png-zlib-invalid')
  assert.equal(reason('docs/invalid-zlib.png'), 'public-png-zlib-invalid')
  assert.equal(reason('docs/trailing-zlib.png'), 'public-png-zlib-invalid')
  assert.equal(reason('docs/wrong-size.png'), 'public-png-decoded-size-invalid')
  assert.equal(reason('docs/wrong-filter.png'), 'public-png-filter-invalid')
  assert.equal(reason('docs/bomb.png'), 'public-png-decompression-limit')
})

test('png_two_zlib_stream_fixture_is_explicit_and_refused', () => {
  const root = repo()
  const raw = Buffer.from([0, 0, 0, 0, 0])
  writePublicCandidate(root, 'docs/two-zlib-streams.png', pngFixture({
    idat: Buffer.concat([deflateSync(raw), deflateSync(Buffer.alloc(0))]),
  }))

  const finding = run({ root, mode: 'candidate', asOf: AS_OF }).findings.find((item) =>
    item.path === 'docs/two-zlib-streams.png')
  assert.equal(finding?.reason, 'public-png-zlib-invalid')

  const spec = readFileSync(path.join(
    ROOT,
    'docs/specs/no-milestone/public-repository-hygiene/SPEC.md',
  ), 'utf8')
  assert.match(spec, /`png_two_zlib_stream_fixture_is_explicit_and_refused`/)
  const evidence = JSON.parse(readFileSync(path.join(
    ROOT,
    'docs/specs/no-milestone/public-repository-hygiene/evidence-manifest.json',
  ), 'utf8'))
  assert.equal(evidence.tests.some((entry) =>
    entry.name === 'png_two_zlib_stream_fixture_is_explicit_and_refused'), true)
})

test('png_refuses_illegal_ihdr_palette_chunk_order_duplicates_and_huge_dimensions', () => {
  const root = repo()
  const compressed = deflateSync(Buffer.from([0, 0, 0, 0, 0]))
  const split = Math.floor(compressed.length / 2)
  writePublicCandidate(root, 'docs/indexed-valid.png', pngFixture({ chunks: [
    pngChunk('IHDR', pngHeader({ bitDepth: 1, colorType: 3 })),
    pngChunk('PLTE', Buffer.from([0, 0, 0])),
    pngChunk('IDAT', deflateSync(Buffer.from([0, 0]))),
    pngChunk('IEND'),
  ] }))
  const fixtures = {
    'illegal-depth.png': pngFixture({ header: pngHeader({ bitDepth: 1, colorType: 6 }) }),
    'illegal-compression.png': pngFixture({ header: pngHeader({ compression: 1 }) }),
    'illegal-filter-method.png': pngFixture({ header: pngHeader({ filter: 1 }) }),
    'interlaced.png': pngFixture({ header: pngHeader({ interlace: 1 }) }),
    'huge.png': pngFixture({ header: pngHeader({ width: 0xffffffff, height: 0xffffffff }) }),
    'indexed-no-palette.png': pngFixture({ header: pngHeader({ bitDepth: 1, colorType: 3 }), raw: Buffer.from([0, 0]) }),
    'grayscale-palette.png': pngFixture({ chunks: [
      pngChunk('IHDR', pngHeader({ colorType: 0 })),
      pngChunk('PLTE', Buffer.from([0, 0, 0])),
      pngChunk('IDAT', deflateSync(Buffer.from([0, 0]))),
      pngChunk('IEND'),
    ] }),
    'idat-before-header.png': pngFixture({ chunks: [
      pngChunk('IDAT', compressed), pngChunk('IHDR', pngHeader()), pngChunk('IEND'),
    ] }),
    'duplicate-header.png': pngFixture({ chunks: [
      pngChunk('IHDR', pngHeader()), pngChunk('IHDR', pngHeader()), pngChunk('IDAT', compressed), pngChunk('IEND'),
    ] }),
    'split-idat.png': pngFixture({ chunks: [
      pngChunk('IHDR', pngHeader()),
      pngChunk('IDAT', compressed.subarray(0, split)),
      pngChunk('tEXt', Buffer.from('Comment\0safe', 'latin1')),
      pngChunk('IDAT', compressed.subarray(split)),
      pngChunk('IEND'),
    ] }),
    'trailing-after-iend.png': Buffer.concat([pngFixture(), pngChunk('IEND')]),
  }
  for (const [name, buffer] of Object.entries(fixtures)) writePublicCandidate(root, `docs/${name}`, buffer)
  const findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  const reason = (file) => findings.find((item) => item.path === file)?.reason
  assert.equal(reason('docs/illegal-depth.png'), 'public-png-ihdr-invalid')
  assert.equal(reason('docs/illegal-compression.png'), 'public-png-ihdr-invalid')
  assert.equal(reason('docs/illegal-filter-method.png'), 'public-png-ihdr-invalid')
  assert.equal(reason('docs/interlaced.png'), 'public-png-ihdr-invalid')
  assert.equal(reason('docs/huge.png'), 'public-png-decoded-size-limit')
  assert.equal(reason('docs/indexed-no-palette.png'), 'public-png-palette-invalid')
  assert.equal(reason('docs/grayscale-palette.png'), 'public-png-palette-invalid')
  assert.equal(reason('docs/indexed-valid.png'), undefined)
  for (const file of ['idat-before-header.png', 'duplicate-header.png', 'split-idat.png', 'trailing-after-iend.png']) {
    assert.equal(reason(`docs/${file}`), 'public-png-chunk-order-invalid', file)
  }
})

test('png_indexed_background_requires_existing_palette_entry', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/indexed-background.png', pngFixture({ chunks: [
    pngChunk('IHDR', pngHeader({ bitDepth: 1, colorType: 3 })),
    pngChunk('PLTE', Buffer.from([0, 0, 0])),
    pngChunk('bKGD', Buffer.from([1])),
    pngChunk('IDAT', deflateSync(Buffer.from([0, 0]))),
    pngChunk('IEND'),
  ] }))

  const finding = run({ root, mode: 'candidate', asOf: AS_OF }).findings.find((item) =>
    item.path === 'docs/indexed-background.png')
  assert.equal(finding?.reason, 'public-png-ancillary-invalid')
})

test('png_ancillary_metadata_is_bounded_parsed_and_privacy_scanned', () => {
  const safe = repo()
  const pixels = deflateSync(Buffer.from([0, 0, 0, 0, 0]))
  const textData = Buffer.from('Comment\0portable metadata', 'latin1')
  const compressedText = Buffer.concat([
    Buffer.from('Comment\0\0', 'latin1'),
    deflateSync(Buffer.from('portable compressed metadata', 'latin1')),
  ])
  const internationalText = Buffer.concat([
    Buffer.from('Comment\0\0\0en\0Portable\0', 'utf8'),
    Buffer.from('portable international metadata', 'utf8'),
  ])
  const safeChunks = (metadata) => [
    pngChunk('IHDR', pngHeader()),
    pngChunk('cHRM', Buffer.alloc(32)),
    pngChunk('bKGD', Buffer.alloc(6)),
    pngChunk(metadata.type, metadata.data),
    pngChunk('IDAT', pixels),
    pngChunk('IEND'),
  ]
  writePublicCandidate(safe, 'docs/text.png', pngFixture({ chunks: safeChunks({ type: 'tEXt', data: textData }) }))
  writePublicCandidate(safe, 'docs/compressed.png', pngFixture({ chunks: safeChunks({ type: 'zTXt', data: compressedText }) }))
  writePublicCandidate(safe, 'docs/international.png', pngFixture({ chunks: safeChunks({ type: 'iTXt', data: internationalText }) }))
  assert.equal(run({ root: safe, mode: 'candidate', asOf: AS_OF }).ok, true)

  const unsafe = repo()
  const privateText = Buffer.from(`Comment\0${privateUnixPath()}`, 'latin1')
  const privateCompressed = Buffer.concat([
    Buffer.from('Comment\0\0', 'latin1'),
    deflateSync(Buffer.from(privateUnixPath(), 'latin1')),
  ])
  const privateInternational = Buffer.concat([
    Buffer.from('Comment\0\0\0en\0Private\0', 'utf8'),
    Buffer.from(privateUnixPath(), 'utf8'),
  ])
  const fixture = (type, data) => pngFixture({ chunks: [
    pngChunk('IHDR', pngHeader()),
    pngChunk(type, data),
    pngChunk('IDAT', pixels),
    pngChunk('IEND'),
  ] })
  writePublicCandidate(unsafe, 'docs/private-text.png', fixture('tEXt', privateText))
  writePublicCandidate(unsafe, 'docs/private-uuid.png', fixture(
    'tEXt',
    Buffer.from(`Comment\0${rawRunId()}`, 'latin1'),
  ))
  writePublicCandidate(unsafe, 'docs/private-compressed.png', fixture('zTXt', privateCompressed))
  writePublicCandidate(unsafe, 'docs/private-international.png', fixture('iTXt', privateInternational))
  writePublicCandidate(unsafe, 'docs/unknown-ancillary.png', fixture('raNd', Buffer.from(privateUnixPath())))
  writePublicCandidate(unsafe, 'docs/metadata-bomb.png', fixture('zTXt', Buffer.concat([
    Buffer.from('Comment\0\0', 'latin1'),
    deflateSync(Buffer.alloc(128 * 1024, 0x41)),
  ])))
  const findings = run({ root: unsafe, mode: 'candidate', asOf: AS_OF }).findings
  const reason = (file) => findings.find((item) => item.path === file)?.reason
  for (const file of ['private-text.png', 'private-uuid.png', 'private-compressed.png', 'private-international.png']) {
    assert.equal(reason(`docs/${file}`), 'public-png-ancillary-sensitive', file)
  }
  assert.equal(reason('docs/unknown-ancillary.png'), 'public-png-ancillary-unsupported')
  assert.equal(reason('docs/metadata-bomb.png'), 'public-png-ancillary-size-limit')
})

test('png_itxt_language_requires_exact_raw_ascii_bytes', () => {
  const root = repo()
  const malformed = Buffer.concat([
    Buffer.from('Comment\0\0\0', 'latin1'),
    Buffer.from([0xe5, 0x00]),
    Buffer.from('Portable\0portable text', 'utf8'),
  ])
  writePublicCandidate(root, 'docs/high-bit-language.png', pngFixture({ chunks: [
    pngChunk('IHDR', pngHeader()),
    pngChunk('iTXt', malformed),
    pngChunk('IDAT', deflateSync(Buffer.from([0, 0, 0, 0, 0]))),
    pngChunk('IEND'),
  ] }))

  const finding = run({ root, mode: 'candidate', asOf: AS_OF }).findings.find((item) =>
    item.path === 'docs/high-bit-language.png')
  assert.equal(finding?.reason, 'public-png-ancillary-invalid')
})

test('jpeg_requires_reviewed_digest_and_strict_baseline_standalone_entropy', () => {
  const validRoot = repo()
  const valid = minimalBaselineJpeg()
  writePublicCandidate(validRoot, 'docs/photo.jpg', valid)
  writeBinaryDigestContract(validRoot, [binaryDigestEntry('docs/photo.jpg', valid)])
  assert.equal(run({ root: validRoot, mode: 'tracked', asOf: AS_OF }).ok, true)
  assert.equal(run({ root: validRoot, mode: 'candidate', asOf: AS_OF }).findings.some((item) =>
    item.reason === 'public-jpeg-authority-path-set-mismatch'), true)

  const restartedRoot = repo()
  const restarted = minimalBaselineJpeg({
    restartInterval: 1,
    entropy: Buffer.from([0x3f, 0xff, 0xd0, 0x3f]),
  })
  writePublicCandidate(restartedRoot, 'docs/restarted.jpeg', restarted)
  writeBinaryDigestContract(restartedRoot, [binaryDigestEntry('docs/restarted.jpeg', restarted)])
  assert.equal(run({ root: restartedRoot, mode: 'tracked', asOf: AS_OF }).ok, true)

  const invalidJfif = Buffer.from(minimalBaselineJpeg())
  invalidJfif[6] = 0x00
  const cases = [
    ['invalid-jfif.jpg', invalidJfif, 'public-jpeg-jfif-invalid'],
    ['no-dqt.jpg', minimalBaselineJpeg({ includeDqt: false }), 'public-jpeg-table-invalid'],
    ['no-dht.jpg', minimalBaselineJpeg({ includeDht: false }), 'public-jpeg-table-invalid'],
    ['progressive.jpg', minimalBaselineJpeg({ frameMarker: 0xc2 }), 'public-jpeg-frame-invalid'],
    ['arithmetic.jpg', minimalBaselineJpeg({ frameMarker: 0xc9 }), 'public-jpeg-frame-invalid'],
    ['precision.jpg', minimalBaselineJpeg({ precision: 12 }), 'public-jpeg-frame-invalid'],
    ['bad-sos.jpg', minimalBaselineJpeg({ scanComponent: 2 }), 'public-jpeg-scan-invalid'],
    ['private-uuid-com.jpg', minimalBaselineJpeg({
      metadata: [jpegSegment(0xfe, Buffer.from(rawRunId(), 'latin1'))],
    }), 'public-jpeg-metadata-sensitive'],
    ['arbitrary-entropy.jpg', minimalBaselineJpeg({ entropy: Buffer.from([0x7f]) }), 'public-jpeg-entropy-invalid'],
    ['unstuffed-marker.jpg', minimalBaselineJpeg({ entropy: Buffer.from([0xff, 0x01]) }), 'public-jpeg-entropy-invalid'],
    ['restart-without-dri.jpg', minimalBaselineJpeg({ entropy: Buffer.from([0x3f, 0xff, 0xd0, 0x3f]) }), 'public-jpeg-entropy-invalid'],
    ['restart-out-of-order.jpg', minimalBaselineJpeg({
      restartInterval: 1,
      entropy: Buffer.from([0x3f, 0xff, 0xd1, 0x3f]),
    }), 'public-jpeg-entropy-invalid'],
  ]
  for (const [name, buffer, expected] of cases) {
    const root = repo()
    writePublicCandidate(root, `docs/${name}`, buffer)
    writeBinaryDigestContract(root, [binaryDigestEntry(`docs/${name}`, buffer)])
    const finding = run({ root, mode: 'candidate', asOf: AS_OF }).findings.find((item) => item.path === `docs/${name}`)
    assert.equal(finding?.reason, expected, name)
  }
})

test('jpeg_digest_contract_refuses_unlisted_digest_duplicate_path_schema_and_symlink_tamper', () => {
  const fixture = () => {
    const root = repo()
    const jpeg = minimalBaselineJpeg()
    writePublicCandidate(root, 'docs/photo.jpg', jpeg)
    writeBinaryDigestContract(root, [binaryDigestEntry('docs/photo.jpg', jpeg)])
    return { root, jpeg, entry: binaryDigestEntry('docs/photo.jpg', jpeg) }
  }
  const reason = (root) => run({ root, mode: 'candidate', asOf: AS_OF }).findings
    .find((item) => item.path === 'docs/governance/public-binary-digests.json')?.reason

  const unlisted = fixture()
  writeBinaryDigestContract(unlisted.root, [])
  assert.equal(reason(unlisted.root), 'public-jpeg-manifest-path-set-mismatch')

  const digest = fixture()
  writeBinaryDigestContract(digest.root, [{ ...digest.entry, sha256: '0'.repeat(64) }])
  assert.equal(reason(digest.root), 'public-jpeg-manifest-digest-mismatch')

  const duplicate = fixture()
  writeBinaryDigestContract(duplicate.root, [duplicate.entry, duplicate.entry])
  assert.equal(reason(duplicate.root), 'public-jpeg-manifest-entry-invalid')

  const duplicateDigest = fixture()
  writePublicCandidate(duplicateDigest.root, 'docs/photo-copy.jpg', duplicateDigest.jpeg)
  const copyEntry = binaryDigestEntry('docs/photo-copy.jpg', duplicateDigest.jpeg)
  writeBinaryDigestContract(duplicateDigest.root, [copyEntry, duplicateDigest.entry]
    .sort((left, right) => left.path.localeCompare(right.path)))
  assert.equal(reason(duplicateDigest.root), 'public-jpeg-manifest-entry-invalid')

  const unsafePath = fixture()
  writeBinaryDigestContract(unsafePath.root, [{ ...unsafePath.entry, path: '../docs/photo.jpg' }])
  assert.equal(reason(unsafePath.root), 'public-jpeg-manifest-entry-invalid')

  const schemaVersion = fixture()
  writePublicCandidate(schemaVersion.root, 'docs/governance/public-binary-digests.json', JSON.stringify({
    $schema: './public-binary-digests.schema.json',
    schema_version: 'ramshared-public-binary-digests/v2',
    entries: [schemaVersion.entry],
  }))
  assert.equal(reason(schemaVersion.root), 'public-jpeg-manifest-invalid')

  const invalidUtf8 = fixture()
  writePublicCandidate(invalidUtf8.root, 'docs/governance/public-binary-digests.json', Buffer.from([0xc3, 0x28]))
  assert.equal(reason(invalidUtf8.root), 'public-jpeg-manifest-invalid')

  const schema = fixture()
  writeBinaryDigestContract(schema.root, [schema.entry], { schema: { ...BINARY_DIGEST_SCHEMA, title: 'tampered' } })
  assert.equal(reason(schema.root), 'public-jpeg-schema-invalid')

  const manifestLink = fixture()
  const manifestPath = path.join(manifestLink.root, 'docs/governance/public-binary-digests.json')
  unlinkSync(manifestPath)
  writePublicCandidate(manifestLink.root, 'docs/governance/manifest-target.json', '{}')
  symlinkSync('manifest-target.json', manifestPath)
  assert.equal(reason(manifestLink.root), 'public-jpeg-manifest-not-regular')

  const assetLink = fixture()
  unlinkSync(path.join(assetLink.root, 'docs/photo.jpg'))
  writePublicCandidate(assetLink.root, 'docs/photo-target.bin', assetLink.jpeg)
  symlinkSync('photo-target.bin', path.join(assetLink.root, 'docs/photo.jpg'))
  assert.equal(reason(assetLink.root), 'public-jpeg-manifest-asset-not-regular')

  const untracked = repo()
  const untrackedJpeg = minimalBaselineJpeg()
  writePublicCandidate(untracked, 'docs/untracked.jpg', untrackedJpeg)
  writePublicCandidate(untracked, 'docs/governance/public-binary-digests.schema.json', `${JSON.stringify(BINARY_DIGEST_SCHEMA)}\n`)
  writePublicCandidate(untracked, 'docs/governance/public-binary-digests.json', JSON.stringify({
    $schema: './public-binary-digests.schema.json',
    schema_version: 'ramshared-public-binary-digests/v1',
    entries: [binaryDigestEntry('docs/untracked.jpg', untrackedJpeg)],
  }))
  assert.equal(reason(untracked), 'public-jpeg-manifest-asset-not-tracked')
})

test('jpeg_candidate_refuses_changed_bytes_mutable_manifest_and_private_metadata', () => {
  const root = repo()
  const original = minimalBaselineJpeg()
  writePublicCandidate(root, 'docs/photo.jpg', original)
  writeBinaryDigestContract(root, [binaryDigestEntry('docs/photo.jpg', original)])
  commitAll(root, 'reviewed jpeg')

  const replacement = minimalBaselineJpeg({
    restartInterval: 1,
    entropy: Buffer.from([0x3f, 0xff, 0xd0, 0x3f]),
    metadata: [jpegSegment(0xfe, Buffer.from(privateUnixPath(), 'latin1'))],
  })
  writePublicCandidate(root, 'docs/photo.jpg', replacement)
  let findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  assert.equal(findings.some((item) => item.reason === 'public-jpeg-manifest-digest-mismatch'), true)

  writeBinaryDigestContract(root, [binaryDigestEntry('docs/photo.jpg', replacement)])
  findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  assert.equal(findings.some((item) => item.reason === 'public-jpeg-manifest-authority-mismatch'), true)
  assert.equal(findings.some((item) =>
    item.path === 'docs/photo.jpg' && item.reason === 'public-jpeg-metadata-sensitive'), true)

  commitAll(root, 'unreviewed jpeg and manifest change')
  findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  assert.equal(findings.some((item) => item.reason === 'public-jpeg-manifest-authority-mismatch'), true)

  writePublicCandidate(root, 'docs/governance/public-binary-digests.json', JSON.stringify({
    $schema: './public-binary-digests.schema.json',
    schema_version: 'ramshared-public-binary-digests/v1',
    entries: [binaryDigestEntry('docs/photo.jpg', original)],
  }))
  findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  assert.equal(findings.some((item) => item.reason === 'public-jpeg-manifest-digest-mismatch'), true)
})

test('jpeg_metadata_profiles_refuse_utf16_and_malformed_c2pa_app11', () => {
  const utf16Le = Buffer.from(privateUnixPath(), 'utf16le')
  const utf16Be = Buffer.from(utf16Le)
  utf16Be.swap16()
  const malformedC2pa = Buffer.concat([
    Buffer.from([0x4a, 0x50, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]),
    Buffer.from('c2pa-portable-but-not-jumbf', 'ascii'),
  ])
  const malformedC2paUuid = Buffer.concat([
    Buffer.from([0x4a, 0x50, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]),
    Buffer.from(`c2pa-${rawRunId()}`, 'ascii'),
  ])
  const cases = [
    ['utf16le-com.jpg', jpegSegment(0xfe, utf16Le), 'public-jpeg-metadata-sensitive'],
    ['utf16be-app1.jpg', jpegSegment(0xe1, utf16Be), 'public-jpeg-metadata-sensitive'],
    ['malformed-c2pa.jpg', jpegSegment(0xeb, malformedC2pa), 'public-jpeg-metadata-unsupported'],
    ['malformed-c2pa-uuid.jpg', jpegSegment(0xeb, malformedC2paUuid), 'public-jpeg-metadata-sensitive'],
  ]

  for (const [name, metadata, expected] of cases) {
    const root = repo()
    const jpeg = minimalBaselineJpeg({ metadata: [metadata] })
    writePublicCandidate(root, `docs/${name}`, jpeg)
    writeBinaryDigestContract(root, [binaryDigestEntry(`docs/${name}`, jpeg)])
    const finding = run({ root, mode: 'tracked', asOf: AS_OF }).findings.find((item) =>
      item.path === `docs/${name}`)
    assert.equal(finding?.reason, expected, name)
  }

  const structuredRoot = repo()
  const structured = Buffer.from(readFileSync(path.join(
    ROOT,
    'docs/marketing/benchmark-comparison.jpg',
  )))
  const actionsLabel = Buffer.from('c2pa.actions.v2\0', 'ascii')
  const leafHeader = structured.indexOf(actionsLabel) + actionsLabel.length
  assert.equal(structured.subarray(leafHeader + 4, leafHeader + 8).toString('ascii'), 'cbor')
  Buffer.from(privateUnixPath(), 'ascii').copy(structured, leafHeader + 8)
  writePublicCandidate(structuredRoot, 'docs/structured-private-c2pa.jpg', structured)
  writeBinaryDigestContract(structuredRoot, [binaryDigestEntry('docs/structured-private-c2pa.jpg', structured)])
  const structuredFinding = run({ root: structuredRoot, mode: 'tracked', asOf: AS_OF }).findings.find((item) =>
    item.path === 'docs/structured-private-c2pa.jpg')
  assert.equal(structuredFinding?.reason, 'public-jpeg-metadata-sensitive')
})

test('public_hygiene_cli_check_aliases_candidate_and_unknown_options_refuse', () => {
  const root = repo()
  writePublicCandidate(root, 'docs/unsafe.txt', privateUnixPath())
  const check = spawnSync(process.execPath, [CLI, '--check'], { cwd: root, encoding: 'utf8' })
  assert.equal(check.status, 1)
  assert.match(check.stdout, /^MODE=candidate$/m)
  assert.match(check.stderr, /docs\/unsafe\.txt:1 .* PRIVATE_UNIX_PATH/)
  assert.equal(spawnSync(process.execPath, [CLI, '--not-a-mode'], { cwd: root, encoding: 'utf8' }).status, 2)
})

test('unicode_content_diagnostic_encodes_controls_without_rendering_them', () => {
  const root = repo()
  const bidi = unicode(0x202e)
  writePublicCandidate(root, 'docs/unsafe.md', ['linux-', 'kernel-lab'].join(bidi))
  const proc = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  assert.equal(proc.status, 1)
  assert.match(`${proc.stdout}${proc.stderr}`, /docs\/unsafe\.md:1 .* UNSAFE_UNICODE_CONTENT: unsafe-unicode-content-u\+202e/)
  assert.doesNotMatch(`${proc.stdout}${proc.stderr}`, new RegExp(bidi))
})

test('rejects_private_profile_email_token_key_and_kernel_address', () => {
  const windowsPath = ['C:', 'Users', 'private-user', 'Desktop', 'note.txt'].join('\\')
  const email = ['private.user', 'example.invalid'].join('@')
  const token = ['ghp', '123456789012345678901234567890123456'].join('_')
  const key = ['-----BEGIN ', 'PRIVATE KEY-----'].join('')
  const address = ['ffff', '888012345678'].join('')
  const findings = scanText('fixture.md', [windowsPath, email, token, key, address].join('\n'), undefined, AS_OF)
  assert.deepEqual(new Set(findings.map((item) => item.rule)), new Set(['PRIVATE_WINDOWS_PATH', 'EMAIL', 'TOKEN', 'PRIVATE_KEY', 'KERNEL_ADDRESS']))
})

test('every_concrete_public_identity_class_fails_even_beside_historical_warning', () => {
  const text = [
    noExecutionWarning(), 'win11-wsl-lab', rawArtifactPath(), hyperVPath(), rawArtifactRunPath(), timestampRun(), rawRunId(),
    rawSdDevice(), rawNvmeDevice(), rawMapperDevice(), rawPrincipal(), rawPrivateIp(),
  ].join('\n')
  const findings = scanText('docs/unsafe.md', text, undefined, AS_OF)
  assert.deepEqual(new Set(findings.map((item) => item.rule)), new Set([
    'RAW_LAB_VM', 'PRIVATE_WINDOWS_ARTIFACT_RUN', 'RAW_ARTIFACT_RUN_PATH',
    'RAW_TIMESTAMP_RUN_ID', 'RAW_UUID', 'RAW_DEVICE', 'RAW_PRINCIPAL', 'PRIVATE_IP',
  ]))
  assert.equal(findings.filter((item) => item.rule === 'RAW_DEVICE').length, 3)
})

test('changed_public_json_and_filenames_refuse_known_lab_and_timestamp_identities', () => {
  const root = repo()
  mkdirSync(path.join(root, 'docs'), { recursive: true })
  const knownLabels = ['linux-kernel-lab', 'gha-ubuntu-2404']
  const knownRuns = ['wsl-probe-vmcompute', 'guest-exhaustive', 'startio-probe'].map(knownTimestampRun)
  writeFileSync(path.join(root, 'docs', 'catalog.json'), JSON.stringify({ history: [noExecutionWarning(), ...knownLabels, ...knownRuns] }))
  writeFileSync(path.join(root, 'docs', `${knownTimestampRun('guest-exhaustive')}.md`), '# historical\n')
  const result = run({ root, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.findings.filter((item) => item.path === 'docs/catalog.json' && item.rule === 'RAW_LAB_VM').length, 2)
  assert.equal(result.findings.filter((item) => item.path === 'docs/catalog.json' && item.rule === 'RAW_TIMESTAMP_RUN_ID').length, 3)
  assert.equal(result.findings.some((item) => item.path.includes('guest-exhaustive') && item.rule === 'RAW_TIMESTAMP_RUN_ID'), true)
  const sanitized = JSON.stringify({ history: ['SANITIZED_VM_KERNEL_LAB', 'SANITIZED_RUN_GUEST_EXHAUSTIVE_DATE_20260822_SLOT_A'] })
  assert.deepEqual(scanText('docs/catalog.json', sanitized, undefined, AS_OF), [])
})

test('scanner_evaluates_all_matches_and_sanitized_controls_pass', () => {
  const rawTwice = [noExecutionWarning(), rawSdDevice(), 'later:', rawSdDevice()].join('\n')
  assert.equal(scanText('docs/history.md', rawTwice, undefined, AS_OF).filter((item) => item.rule === 'RAW_DEVICE').length, 2)
  const sanitized = [
    'SANITIZED_VM_WSL2_LAB', 'SANITIZED_PATH_HOST_PRIVATE_ARTIFACT', 'SANITIZED_RUN_ID',
    'SANITIZED_DEVICE_ORIGIN', 'SANITIZED_PRINCIPAL_WINDOWS_LAB', 'SANITIZED_IP_PRIVATE',
    sanitizedActivationWarning(), fenced(activationCommand()),
  ].join('\n')
  assert.deepEqual(scanText('docs/sanitized.md', sanitized, undefined, AS_OF), [])
  assert.deepEqual(scanDocumentActivation('docs/sanitized.md', sanitized), [])
})

test('activation_requires_an_adjacent_prior_warning_for_fenced_and_inline_commands', () => {
  const guarded = [sanitizedActivationWarning(), fenced(activationCommand())].join('\n')
  const warningAfter = [fenced(activationCommand()), noExecutionWarning()].join('\n')
  const warningTooFar = [noExecutionWarning(), 'historical context', 'more context', 'still not a boundary', fenced(activationCommand())].join('\n')
  const warningAcrossClosedFence = [noExecutionWarning(), '```text', 'closed evidence', '```', activationCommand()].join('\n')
  const inline = 'Run `wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated` now.'
  const prose = 'Run mkswap against the selected device now.'
  assert.deepEqual(scanDocumentActivation('docs/guarded.md', guarded), [])
  assert.equal(scanDocumentActivation('docs/after.md', warningAfter)[0].rule, 'UNGUARDED_ACTIVATION')
  assert.equal(scanDocumentActivation('docs/far.md', warningTooFar)[0].rule, 'UNGUARDED_ACTIVATION')
  assert.equal(scanDocumentActivation('docs/closed-fence.md', warningAcrossClosedFence)[0].rule, 'UNGUARDED_ACTIVATION')
  assert.equal(scanDocumentActivation('docs/inline.md', inline)[0].rule, 'UNGUARDED_ACTIVATION')
  assert.equal(scanDocumentActivation('docs/prose.md', prose)[0].rule, 'UNGUARDED_ACTIVATION')
})

test('activation_catches_bold_bullets_inline_same_line_negation_and_operational_forms', () => {
  const commands = [
    'mkswap SANITIZED_DEVICE_ORIGIN', 'swapon SANITIZED_DEVICE_ORIGIN', 'wslconfig-ctl.sh apply', 'modprobe ramshared',
    'wsl -d SANITIZED_DISTRO -- status', 'New-Partition -DiskNumber 1', 'Clear-Disk -Number 1',
    'Restart-Computer', 'systemctl start ramsharedd', 'wsl --shutdown',
    'mount SANITIZED_DEVICE_ORIGIN SANITIZED_MOUNTPOINT', 'mkfs.ext4 SANITIZED_DEVICE_ORIGIN',
    'Run the origin route now.', 'Invoke SharedWslPressureCampaign now.',
  ]
  for (const command of commands) {
    const findings = scanDocumentActivation('docs/operations.md', `- **Next action:** \`${command}\``)
    assert.equal(findings.length, 1, command)
  }
  assert.equal(scanDocumentActivation('docs/operations.md', 'Do not execute mkswap SANITIZED_DEVICE_ORIGIN.').length, 1)
  assert.equal(scanDocumentActivation('docs/operations.md', [noExecutionWarning(), '- **Historical evidence:** `modprobe ramshared`'].join('\n')).length, 0)
})

test('sanitized_hyperv_history_retains_nonidentifying_result_runtime_dns_and_memory_evidence', () => {
  const history = readFileSync(path.join(ROOT, 'docs/labs/HYPERV-VM-ACCESS.md'), 'utf8')
  assert.match(history, /267009/)
  assert.match(history, /2\.7\.10/)
  assert.match(history, /DNS/i)
  assert.match(history, /startup 2 GiB.*minimum 1 GiB.*maximum 8 GiB/is)
  assert.match(history, /Historical non-current \/ no execution/i)
})

test('redaction_audit_binds_superseded_and_replacement_lines_without_raw_value', () => {
  const root = repo()
  const rawLine = `Raw output: ${rawArtifactRunPath()}`
  writePublicCandidate(root, 'docs/BENCHMARKS.md', `${rawLine}\n`)
  git(root, ['add', 'docs/BENCHMARKS.md'])
  git(root, ['commit', '-qm', 'historical evidence'])
  const sourceRevision = git(root, ['rev-parse', 'HEAD']).trim()
  const replacementClass = 'SANITIZED_ARTIFACT_PATH_HISTORY'
  const replacementLine = `Raw output: ${replacementClass}`
  writePublicCandidate(root, 'docs/BENCHMARKS.md', `${replacementLine}\n`)
  const ledger = {
    schema_version: 'ramshared-public-redaction/v1',
    redaction_id: 'PHR-0001',
    applied_at: '2026-08-22T12:00:00Z',
    source_revision: sourceRevision,
    path: 'docs/BENCHMARKS.md',
    source_line: 1,
    replacement_line: 1,
    rule: 'RAW_ARTIFACT_RUN_PATH',
    replacement_class: replacementClass,
    supersedes_line_sha256: lineHash(rawLine),
    replacement_line_sha256: lineHash(replacementLine),
    reason: 'Remove a private artifact path while retaining historical context.',
  }
  writePublicCandidate(root, 'docs/governance/public-hygiene-redactions.jsonl', `${JSON.stringify(ledger)}\n`)
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).ok, true)

  ledger.replacement_line_sha256 = '0'.repeat(64)
  writePublicCandidate(root, 'docs/governance/public-hygiene-redactions.jsonl', `${JSON.stringify(ledger)}\n`)
  const findings = run({ root, mode: 'candidate', asOf: AS_OF }).findings
  assert.match(JSON.stringify(findings), /REDACTION_AUDIT.*replacement-line-digest-mismatch/)
  assert.doesNotMatch(JSON.stringify(findings), new RegExp(rawArtifactRunPath().replaceAll('/', '\\/')))
})

test('contact_allowlist_is_scoped_owned_and_expiring', () => {
  const email = ['maintainer', 'example.invalid'].join('@')
  const valid = { schema_version: 1, entries: [{ pattern: email, scope: ['docs/private/'], owner_role: 'maintainer', reason: 'public project contact', expires: '2099-12-31' }] }
  assert.deepEqual(scanText('docs/private/contact.md', email, valid, AS_OF), [])
  assert.equal(scanText('docs/private2/contact.md', email, valid, AS_OF)[0].rule, 'EMAIL')
  for (const scope of ['', '../docs/', 'docs\\', 'docs//']) {
    const invalid = { schema_version: 1, entries: [{ ...valid.entries[0], scope: [scope] }] }
    assert.throws(() => scanText('docs/contact.md', email, invalid, AS_OF))
  }
  const invalidDate = { schema_version: 1, entries: [{ ...valid.entries[0], expires: '2026-02-30' }] }
  assert.throws(() => scanText('docs/private/contact.md', email, invalidDate, AS_OF))
})

test('staged_allowlist_blob_overrides_a_different_worktree_allowlist', () => {
  const root = repo()
  const email = ['maintainer', 'example.invalid'].join('@')
  mkdirSync(path.join(root, 'docs', 'governance'), { recursive: true })
  const allowed = { schema_version: 1, entries: [{ pattern: email, scope: ['docs/'], owner_role: 'maintainer', reason: 'public contact', expires: '2099-12-31' }] }
  writeFileSync(path.join(root, 'docs', 'contact.md'), email)
  writeFileSync(path.join(root, 'docs', 'governance', 'public-hygiene-allowlist.json'), JSON.stringify(allowed))
  git(root, ['add', 'docs'])
  writeFileSync(path.join(root, 'docs', 'governance', 'public-hygiene-allowlist.json'), JSON.stringify({ schema_version: 1, entries: [] }))
  assert.equal(run({ root, mode: 'staged', asOf: AS_OF }).ok, true)
  assert.equal(run({ root, mode: 'candidate', asOf: AS_OF }).findings.some((item) => item.rule === 'EMAIL'), true)
})

test('diagnostic_never_contains_sensitive_match_and_cli_errors_are_stable', () => {
  const root = repo()
  const secret = privateUnixPath()
  writeFileSync(path.join(root, 'unsafe.md'), secret)
  const proc = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  assert.equal(proc.status, 1)
  assert.doesNotMatch(`${proc.stdout}${proc.stderr}`, new RegExp(secret.replaceAll('/', '\\/')))
  assert.match(`${proc.stdout}${proc.stderr}`, /unsafe\.md:1 .* PRIVATE_UNIX_PATH/)
  assert.equal(spawnSync(process.execPath, [CLI, '--unknown'], { cwd: ROOT, encoding: 'utf8' }).status, 2)
  assert.equal(spawnSync(process.execPath, [CLI, '--candidate'], { cwd: tmpdir(), encoding: 'utf8' }).status, 2)
})

test('identical_candidate_runs_are_deterministic', () => {
  const root = repo()
  writeFileSync(path.join(root, 'z.md'), privateUnixPath())
  writeFileSync(path.join(root, 'a.md'), privateUnixPath())
  const first = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  const second = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  assert.equal(first.status, second.status)
  assert.equal(first.stdout, second.stdout)
  assert.equal(first.stderr, second.stderr)
})

test('repository_candidate_is_clean', () => {
  const result = run({ root: ROOT, mode: 'candidate', asOf: AS_OF })
  assert.equal(result.ok, true, JSON.stringify(result.findings, null, 2))
})
