import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const SOURCE = new URL('../../scripts/docs-check.sh', import.meta.url)

test('docs_check_reports_all_independent_failures', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-docs-check-'))
  const scripts = path.join(root, 'scripts')
  const bin = path.join(root, 'bin')
  const log = path.join(root, 'node-invocations.log')
  mkdirSync(scripts, { recursive: true })
  mkdirSync(bin, { recursive: true })
  const checker = path.join(scripts, 'docs-check.sh')
  writeFileSync(checker, readFileSync(SOURCE, 'utf8'))
  chmodSync(checker, 0o755)

  const fakeNode = path.join(bin, 'node')
  const fakeMarkdownlint = path.join(bin, 'markdownlint')
  writeFileSync(fakeMarkdownlint, ['#!/usr/bin/env bash', 'exit 0'].join('\n') + '\n')
  chmodSync(fakeMarkdownlint, 0o755)
  const toolsDir = path.join(root, 'tools')
  mkdirSync(toolsDir, { recursive: true })
  writeFileSync(path.join(toolsDir, 'check-broken-links.mjs'), '')
  writeFileSync(fakeNode, [
    '#!/usr/bin/env bash',
    'printf "%s\\n" "$*" >> "$DOCS_CHECK_TEST_LOG"',
    'case "$*" in',
    '  *check-documentation-governance.mjs*) exit 11 ;;',
    '  *check-documentation-localization.mjs*) exit 12 ;;',
    '  *) exit 0 ;;',
    'esac',
    '',
  ].join('\n'))
  chmodSync(fakeNode, 0o755)

  const result = spawnSync('bash', [checker], {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      PATH: `${bin}:${process.env.PATH ?? ''}`,
      DOCS_CHECK_TEST_LOG: log,
    },
  })
  const output = `${result.stdout}${result.stderr}`
  const invocations = readFileSync(log, 'utf8')
  assert.equal(result.status, 69, output)
  assert.match(output, /FAIL documentation-governance \(exit=11\)/)
  assert.match(output, /FAIL documentation-localization \(exit=12\)/)
  assert.match(output, /NO-GO \(2 independent failure\(s\)\)/)
  assert.match(invocations, /check-spec-evidence\.mjs --check/)
  assert.match(invocations, /check-docs-check\.test\.mjs/)
})

test('docs_check_enforces_fail_fast_guard_clauses', () => {
  const source = readFileSync(SOURCE, 'utf8')
  assert.match(source, /set\s+-e/)
  assert.match(source, /DOCS_CHECK_FAILURES/)
})

test('docs_check_uses_candidate_public_hygiene', () => {
  const source = readFileSync(SOURCE, 'utf8')
  assert.match(source, /^run_gate public-hygiene node tools\/ci\/check-public-hygiene\.mjs --candidate$/m)
  assert.doesNotMatch(source, /check-public-hygiene\.mjs --tracked/)
})
