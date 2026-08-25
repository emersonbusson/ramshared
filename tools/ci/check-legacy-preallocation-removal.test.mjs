import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import process from 'node:process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { isSafeRepoPath, run, scanText } from './check-legacy-preallocation-removal.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const CLI = path.join(ROOT, 'tools/ci/check-legacy-preallocation-removal.mjs')
const joinUnderscore = (...parts) => parts.join('_')
const joinEmpty = (...parts) => parts.join('')

function git(root, args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' })
}

function repo() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-legacy-removal-'))
  git(root, ['init', '-q'])
  git(root, ['config', 'user.name', 'RamShared Fixture'])
  git(root, ['config', 'user.email', 'fixture'])
  mkdirSync(path.join(root, 'crates', 'fixture'), { recursive: true })
  writeFileSync(path.join(root, 'crates', 'fixture', 'lib.rs'), 'pub fn clean() {}\n')
  git(root, ['add', 'crates/fixture/lib.rs'])
  git(root, ['commit', '-qm', 'fixture'])
  return root
}

function writeCandidate(root, file, content) {
  const target = path.join(root, file)
  mkdirSync(path.dirname(target), { recursive: true })
  writeFileSync(target, content)
}

test('legacy_preallocation_removed_before_day0_deadline', () => {
  const root = repo()
  const legacySelector = joinUnderscore('RAMSHARED', 'VRAM', 'PREALLOC', 'LEGACY')
  writeCandidate(root, 'validation.md', 'Historical command: export ' + legacySelector + '=1\n')
  writeCandidate(
    root,
    'docs/historical/rollback.md',
    'Historical rollback used ' + legacySelector + '=1 before the origin-cache design.\n',
  )
  writeCandidate(
    root,
    'docs/historical.md',
    'Historical evidence records that ' + legacySelector + ' selected the full-VRAM NBD backend.\n',
  )
  writeCandidate(
    root,
    'docs/specs/no-milestone/cascade-vram-ondemand/PRD.md',
    'Historical requirement: set ' + legacySelector + '=1 to enable full-VRAM NBD.\n',
  )
  writeCandidate(
    root,
    'docs/specs/no-milestone/cascade-vram-ondemand/IMPL.md',
    'Historical implementation: the legacy full-VRAM NBD backend remains available.\n',
  )
  writeCandidate(root, 'artifacts/legacy.txt', joinEmpty('Preallocate', 'Vram') + '\n')
  assert.deepEqual(run({ root }), { ok: true, findings: [] })

  writeCandidate(
    root,
    'docs/FAQ.md',
    'Set `' + legacySelector + '=1` to enable the full-VRAM NBD backend.\n',
  )
  let result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(
    result.findings.some((finding) => finding.rule === 'DOC_LEGACY_SELECTOR_AVAILABLE'),
    true,
  )

  writeCandidate(
    root,
    'docs/FAQ.md',
    'The `' + legacySelector + '` selector and its full-VRAM NBD composition were removed.\n',
  )
  assert.deepEqual(run({ root }), { ok: true, findings: [] })

  writeCandidate(
    root,
    'docs/FAQ.md',
    'The legacy full-VRAM NBD backend remains available as a fallback.\n',
  )
  result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(
    result.findings.some((finding) => finding.rule === 'DOC_LEGACY_BACKEND_AVAILABLE'),
    true,
  )

  writeCandidate(
    root,
    'README.pt-BR.md',
    'O backend legado de pré-alocação continua disponível como fallback.\n',
  )
  writeCandidate(root, 'docs/FAQ.md', 'The SSD-authoritative origin cache is the only NBD path.\n')
  result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(
    result.findings.some((finding) =>
      finding.path === 'README.pt-BR.md' && finding.rule === 'DOC_LEGACY_BACKEND_AVAILABLE'),
    true,
  )

  writeCandidate(
    root,
    'docs/FAQ.md',
    'The legacy full-VRAM NBD backend must be removed before the deadline.\n',
  )
  result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(
    result.findings.some((finding) => finding.rule === 'DOC_LEGACY_REMOVAL_PENDING'),
    true,
  )

  writeCandidate(root, 'docs/FAQ.md', 'The SSD-authoritative origin cache is the only NBD path.\n')
  writeCandidate(root, 'README.pt-BR.md', 'O cache com origem SSD é o único caminho NBD.\n')

  const forbidden = [
    ['LEGACY_ENV_SELECTOR', legacySelector],
    ['LEGACY_ENV_ALIAS', joinUnderscore('RAMSHARED', 'VRAM', 'PREALLOC')],
    ['SPARSE_EXPERIMENT_ALIAS', joinUnderscore('RAMSHARED', 'VRAM', 'SPARSE', 'EXPERIMENTAL')],
    ['LEGACY_POWERSHELL_SELECTOR', joinEmpty('Preallocate', 'Vram')],
    ['LEGACY_APP_ARGS_FIELD', joinUnderscore('legacy', 'prealloc')],
    ['LEGACY_NBD_PARAMETER', joinUnderscore('use', 'prealloc')],
    ['LEGACY_HELPER', joinUnderscore('prealloc', 'enabled')],
    ['LEGACY_HELPER_VALUE', joinUnderscore('prealloc', 'enabled', 'value')],
    ['LEGACY_GUARANTEED_PROFILES', joinUnderscore('GUARANTEED', 'PROFILES')],
    ['LEGACY_PROFILE_CANDIDATES', joinUnderscore('guaranteed', 'profile', 'candidates')],
    ['LEGACY_BE_PRE_REFERENCE', ['Be', 'Pre'].join('::')],
    ['LEGACY_BE_PRE_VARIANT', ['enum Be { ', 'Pre', '(VramBackend) }'].join('')],
  ]
  for (const [rule, content] of forbidden) {
    writeCandidate(root, 'scripts/fixture.txt', content + '\n')
    const result = run({ root })
    assert.equal(result.ok, false, rule)
    assert.equal(result.findings.some((finding) => finding.rule === rule), true, rule)
    writeCandidate(root, 'scripts/fixture.txt', 'clean\n')
  }

  writeCandidate(root, 'drivers/windows/fixture.c', legacySelector + '\n')
  result = run({ root })
  assert.equal(result.ok, false)
  assert.equal(
    result.findings.some((finding) => finding.rule === 'LEGACY_ENV_SELECTOR'),
    true,
  )
})

test('candidate_path_and_cli_fail_closed', () => {
  const root = repo()
  assert.equal(isSafeRepoPath(root, 'scripts/check.sh'), true)
  assert.equal(isSafeRepoPath(root, '../outside'), false)
  assert.equal(isSafeRepoPath(root, 'scripts\\outside'), false)
  assert.equal(isSafeRepoPath(root, 'scripts/control' + String.fromCharCode(7)), false)

  const outside = path.join(mkdtempSync(path.join(tmpdir(), 'ramshared-legacy-outside-')), 'legacy.rs')
  writeFileSync(outside, joinUnderscore('RAMSHARED', 'VRAM', 'PREALLOC'))
  mkdirSync(path.join(root, 'scripts'), { recursive: true })
  let symlinkCreated = false
  try {
    symlinkSync(outside, path.join(root, 'scripts', 'escaped.rs'))
    symlinkCreated = true
  } catch (error) {
    assert.equal(error.code, 'EPERM')
  }
  if (symlinkCreated) {
    const escaped = run({ root })
    assert.equal(escaped.ok, false)
    assert.equal(escaped.findings[0].rule, 'CANDIDATE_SYMLINK_ESCAPE')
  }

  assert.equal(scanText('fixture.rs', 'clean\n').length, 0)
  const bad = spawnSync(process.execPath, [CLI, '--wrong'], { cwd: root, encoding: 'utf8' })
  assert.equal(bad.status, 2)
})
