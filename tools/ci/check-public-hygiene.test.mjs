import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  classifyText,
  enumerateFiles,
  run,
  scanText,
} from './check-public-hygiene.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const CLI = path.join(ROOT, 'tools/ci/check-public-hygiene.mjs')

function git(root, args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' })
}

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

function privateUnixPath() {
  return ['', 'home', 'private-user', 'workspace', 'artifact.txt'].join('/')
}

test('candidate_scans_nonignored_untracked_file', () => {
  const root = repo()
  writeFileSync(path.join(root, 'new.md'), `artifact=${privateUnixPath()}\n`)
  const result = run({ root, mode: 'candidate' })
  assert.equal(result.ok, false)
  assert.equal(result.findings.some((item) => item.path === 'new.md' && item.rule === 'PRIVATE_UNIX_PATH'), true)
})

test('staged_reads_index_blob_not_worktree', () => {
  const root = repo()
  writeFileSync(path.join(root, 'README.md'), `artifact=${privateUnixPath()}\n`)
  git(root, ['add', 'README.md'])
  writeFileSync(path.join(root, 'README.md'), '# Sanitized worktree copy\n')
  const staged = run({ root, mode: 'staged' })
  const candidate = run({ root, mode: 'candidate' })
  assert.equal(staged.findings.some((item) => item.rule === 'PRIVATE_UNIX_PATH'), true)
  assert.equal(candidate.ok, true)
})

test('ignored_file_is_not_a_candidate', () => {
  const root = repo()
  writeFileSync(path.join(root, '.gitignore'), 'local-secret.txt\n')
  writeFileSync(path.join(root, 'local-secret.txt'), privateUnixPath())
  assert.equal(enumerateFiles(root, 'candidate').includes('local-secret.txt'), false)
})

test('extensionless_shebang_is_scanned', () => {
  const root = repo()
  writeFileSync(path.join(root, 'runner'), `#!/usr/bin/env bash\necho ${privateUnixPath()}\n`)
  const result = run({ root, mode: 'candidate' })
  assert.equal(result.findings.some((item) => item.path === 'runner'), true)
})

test('binary_blob_is_skipped', () => {
  assert.equal(classifyText(Buffer.from([0x89, 0x50, 0x00, 0x47]), 'asset.bin'), false)
})

test('rejects_private_profile_email_token_key_and_kernel_address', () => {
  const windowsPath = ['C:', 'Users', 'private-user', 'Desktop', 'note.txt'].join('\\')
  const email = ['private.user', 'example.invalid'].join('@')
  const token = ['ghp', '123456789012345678901234567890123456'].join('_')
  const key = ['-----BEGIN ', 'PRIVATE KEY-----'].join('')
  const address = ['ffff', '888012345678'].join('')
  const findings = scanText('fixture.md', [windowsPath, email, token, key, address].join('\n'))
  assert.deepEqual(new Set(findings.map((item) => item.rule)), new Set([
    'PRIVATE_WINDOWS_PATH', 'EMAIL', 'TOKEN', 'PRIVATE_KEY', 'KERNEL_ADDRESS',
  ]))
})

test('contact_allowlist_is_scoped_owned_and_expiring', () => {
  const email = ['maintainer', 'example.invalid'].join('@')
  const valid = { schema_version: 1, entries: [{ pattern: email, scope: ['docs/contact.md'], owner_role: 'maintainer', reason: 'public project contact', expires: '2099-12-31' }] }
  assert.deepEqual(scanText('docs/contact.md', email, valid, new Date('2026-08-09T00:00:00Z')), [])
  assert.throws(() => scanText('docs/contact.md', email, { schema_version: 1, entries: [{ pattern: email, scope: ['docs/'] }] }))
  assert.equal(scanText('docs/other.md', email, valid, new Date('2026-08-09T00:00:00Z'))[0].rule, 'EMAIL')
})

test('diagnostic_never_contains_sensitive_match', () => {
  const root = repo()
  const secret = privateUnixPath()
  writeFileSync(path.join(root, 'unsafe.md'), secret)
  const proc = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: root, encoding: 'utf8' })
  assert.equal(proc.status, 1)
  assert.doesNotMatch(`${proc.stdout}${proc.stderr}`, new RegExp(secret.replaceAll('/', '\\/')))
  assert.match(`${proc.stdout}${proc.stderr}`, /unsafe\.md:1 .* PRIVATE_UNIX_PATH/)
})

test('invalid_mode_and_git_failure_exit_two', () => {
  const invalid = spawnSync(process.execPath, [CLI, '--unknown'], { cwd: ROOT, encoding: 'utf8' })
  const noGit = spawnSync(process.execPath, [CLI, '--candidate'], { cwd: tmpdir(), encoding: 'utf8' })
  assert.equal(invalid.status, 2)
  assert.equal(noGit.status, 2)
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

test('full_repository_candidate_is_clean', () => {
  const result = run({ root: ROOT, mode: 'candidate' })
  assert.equal(result.ok, true, JSON.stringify(result.findings, null, 2))
})

test('docs_check_uses_candidate_public_hygiene', () => {
  const text = readFileSync(path.join(ROOT, 'scripts/docs-check.sh'), 'utf8')
  assert.match(text, /check-public-hygiene\.mjs --candidate/)
})
