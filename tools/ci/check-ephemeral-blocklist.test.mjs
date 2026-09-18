#!/usr/bin/env node
import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { globToRegex, parseBlocklist, findViolations, main } from './check-ephemeral-blocklist.mjs'
import { writeFileSync, mkdirSync, rmSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const TMP_DIR = path.join(__dirname, '..', '..', 'target', 'test-ephemeral-blocklist')

function setup() {
  mkdirSync(TMP_DIR, { recursive: true })
}

function teardown() {
  rmSync(TMP_DIR, { recursive: true, force: true })
}


describe('globToRegex', () => {
  it('matches exact file name', () => {
    const re = globToRegex('scripts/upstream/submit-wsl-upstream.sh')
    assert.ok(re.test('scripts/upstream/submit-wsl-upstream.sh'))
    assert.ok(!re.test('scripts/upstream/other.sh'))
  })

  it('matches single-star wildcard', () => {
    const re = globToRegex('scripts/upstream/*.sh')
    assert.ok(re.test('scripts/upstream/submit.sh'))
    assert.ok(re.test('scripts/upstream/dispatch-issues.sh'))
    assert.ok(!re.test('scripts/upstream/nested/deep.sh'))
    assert.ok(!re.test('scripts/other.sh'))
  })

  it('matches double-star recursive wildcard', () => {
    const re = globToRegex('**/submit-*.sh')
    assert.ok(re.test('submit-wsl.sh'))
    assert.ok(re.test('scripts/upstream/submit-wsl.sh'))
    assert.ok(re.test('a/b/c/submit-lkml.sh'))
    assert.ok(!re.test('scripts/run.sh'))
  })

  it('matches directory prefix with double-star', () => {
    const re = globToRegex('scripts/ad-hoc/**')
    assert.ok(re.test('scripts/ad-hoc/test.py'))
    assert.ok(re.test('scripts/ad-hoc/sub/dir/file.sh'))
    assert.ok(!re.test('scripts/ci/test.py'))
  })

  it('matches question mark single char', () => {
    const re = globToRegex('scripts/dispatch-?.sh')
    assert.ok(re.test('scripts/dispatch-1.sh'))
    assert.ok(!re.test('scripts/dispatch-12.sh'))
  })

  it('escapes regex special characters', () => {
    const re = globToRegex('docs/file.txt')
    assert.ok(re.test('docs/file.txt'))
    assert.ok(!re.test('docs/filextxt'))
  })

  it('rejects non-matching path', () => {
    const re = globToRegex('scripts/upstream/*.sh')
    assert.ok(!re.test('tools/ci/check.mjs'))
  })
})

describe('parseBlocklist', () => {
  it('parses lines and ignores comments and blanks', () => {
    const content = `
# Ephemeral scripts
scripts/upstream/*.sh
**/submit-*.sh

# Ad-hoc utilities
scripts/ad-hoc/**
`
    const result = parseBlocklist(content)
    assert.deepEqual(result, [
      'scripts/upstream/*.sh',
      '**/submit-*.sh',
      'scripts/ad-hoc/**',
    ])
  })

  it('returns empty array for empty content', () => {
    assert.deepEqual(parseBlocklist(''), [])
    assert.deepEqual(parseBlocklist('# only comments\n# another'), [])
  })
})

describe('findViolations', () => {
  it('detects matching tracked files', () => {
    const patterns = ['scripts/upstream/*.sh', '**/dispatch-*.py']
    const files = [
      'README.md',
      'scripts/upstream/submit-wsl.sh',
      'tools/ci/check.mjs',
      'ad-hoc/dispatch-issues.py',
    ]
    const violations = findViolations(patterns, files)
    assert.equal(violations.length, 2)
    assert.equal(violations[0].file, 'scripts/upstream/submit-wsl.sh')
    assert.equal(violations[0].pattern, 'scripts/upstream/*.sh')
    assert.equal(violations[1].file, 'ad-hoc/dispatch-issues.py')
    assert.equal(violations[1].pattern, '**/dispatch-*.py')
  })

  it('returns empty array when no matches', () => {
    const patterns = ['scripts/upstream/*.sh']
    const files = ['README.md', 'src/main.rs', 'tools/ci/check.mjs']
    const violations = findViolations(patterns, files)
    assert.equal(violations.length, 0)
  })

  it('returns empty array when no patterns', () => {
    assert.deepEqual(findViolations([], ['README.md']), [])
  })

  it('reports one violation per file even with multiple matching patterns', () => {
    const patterns = ['scripts/upstream/*.sh', '**/submit-*.sh']
    const files = ['scripts/upstream/submit-wsl.sh']
    const violations = findViolations(patterns, files)
    assert.equal(violations.length, 1) // first match wins
  })
})

describe('main', () => {
  it('passes when blocklist file does not exist', () => {
    setup()
    const msgs = []
    const errs = []
    const code = main(
      ['--blocklist', path.join(TMP_DIR, 'nonexistent-blocklist')],
      { print: m => msgs.push(m), error: e => errs.push(e) },
    )
    assert.equal(code, 0)
    assert.ok(msgs[0]?.includes('PASS'))
    teardown()
  })

  it('passes when blocklist is empty (comments only)', () => {
    setup()
    const blPath = path.join(TMP_DIR, 'empty-blocklist')
    writeFileSync(blPath, '# only comments\n# nothing here\n')
    const msgs = []
    const code = main(
      ['--blocklist', blPath],
      { print: m => msgs.push(m), error: () => {} },
    )
    assert.equal(code, 0)
    assert.ok(msgs[0]?.includes('PASS'))
    teardown()
  })

  it('passes against actual repo with no violations (default blocklist)', () => {
    const msgs = []
    // Use the real .ci-ephemeral-blocklist in the repo (submit script already removed)
    const code = main([], { print: m => msgs.push(m), error: () => {} })
    assert.equal(code, 0)
    assert.ok(msgs[0]?.includes('PASS'))
  })

  it('detects violations with custom blocklist matching real files', () => {
    setup()
    // Create a blocklist that matches the actual README.md
    const blPath = path.join(TMP_DIR, 'catch-readme-blocklist')
    writeFileSync(blPath, 'README.md\n')
    const msgs = []
    const errs = []
    const code = main(
      ['--blocklist', blPath],
      { print: m => msgs.push(m), error: e => errs.push(e) },
    )
    assert.equal(code, 1)
    assert.ok(errs.some(e => e.includes('FAIL')))
    assert.ok(errs.some(e => e.includes('README.md')))
    teardown()
  })
})
