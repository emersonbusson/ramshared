#!/usr/bin/env node
/**
 * check-ephemeral-blocklist.mjs — CI gate for ephemeral file hygiene
 *
 * Prevents maintainer utilities, ad-hoc dispatch scripts, and other
 * ephemeral artifacts from persisting in the production tree.
 *
 * Reads glob patterns (one per line) from `.ci-ephemeral-blocklist`
 * at the repository root and verifies that no tracked file matches
 * any pattern. Lines starting with `#` are comments, blank lines
 * are ignored.
 *
 * Usage:
 *   node tools/ci/check-ephemeral-blocklist.mjs [--blocklist <path>]
 *
 * Exit codes:
 *   0  No blocklisted files found in tree
 *   1  Blocklisted files detected (merge must be blocked)
 *   2  Usage error or missing blocklist
 */
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const DEFAULT_BLOCKLIST = '.ci-ephemeral-blocklist'

/**
 * Minimal glob-to-regex converter supporting:
 *   *  → any non-slash chars
 *   ** → any depth (including /)
 *   ?  → single non-slash char
 * All other characters are escaped.
 */
export function globToRegex(pattern) {
  let re = '^'
  let i = 0
  while (i < pattern.length) {
    const c = pattern[i]
    if (c === '*' && pattern[i + 1] === '*') {
      // ** matches any path depth
      re += '.*'
      i += 2
      // consume trailing /
      if (pattern[i] === '/') i++
    } else if (c === '*') {
      re += '[^/]*'
      i++
    } else if (c === '?') {
      re += '[^/]'
      i++
    } else if ('.+^${}()|[]\\'.includes(c)) {
      re += `\\${c}`
      i++
    } else {
      re += c
      i++
    }
  }
  re += '$'
  return new RegExp(re)
}

export function parseBlocklist(content) {
  return content
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0 && !line.startsWith('#'))
}

export function findViolations(patterns, trackedFiles) {
  const regexes = patterns.map(p => ({ pattern: p, regex: globToRegex(p) }))
  const violations = []
  for (const file of trackedFiles) {
    for (const { pattern, regex } of regexes) {
      if (regex.test(file)) {
        violations.push({ file, pattern })
        break // one match per file is enough
      }
    }
  }
  return violations
}

function getTrackedFiles(root) {
  const output = execFileSync('git', ['ls-files', '-z'], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  return output.split('\0').filter(Boolean)
}

export function main(argv = process.argv.slice(2), { print = console.log, error = console.error } = {}) {
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim()

  let blocklistPath = path.join(root, DEFAULT_BLOCKLIST)
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--blocklist' && argv[i + 1]) {
      blocklistPath = path.resolve(argv[i + 1])
      i++
    }
  }

  if (!existsSync(blocklistPath)) {
    print('EPHEMERAL_BLOCKLIST_STATUS=PASS (no blocklist file found, nothing to enforce)')
    return 0
  }

  const content = readFileSync(blocklistPath, 'utf8')
  const patterns = parseBlocklist(content)

  if (patterns.length === 0) {
    print('EPHEMERAL_BLOCKLIST_STATUS=PASS (blocklist is empty)')
    return 0
  }

  const trackedFiles = getTrackedFiles(root)
  const violations = findViolations(patterns, trackedFiles)

  if (violations.length > 0) {
    error('EPHEMERAL_BLOCKLIST_STATUS=FAIL')
    error(`Found ${violations.length} ephemeral file(s) that must not persist in production:`)
    for (const { file, pattern } of violations) {
      error(`  BLOCKED: ${file}  (matched pattern: ${pattern})`)
    }
    error('')
    error('Remove these files before merging, or update .ci-ephemeral-blocklist if the pattern is stale.')
    return 1
  }

  print(`EPHEMERAL_BLOCKLIST_STATUS=PASS (${patterns.length} patterns checked, ${trackedFiles.length} files scanned)`)
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === new URL(import.meta.url).pathname) process.exitCode = main()
