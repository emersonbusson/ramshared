import test from 'node:test'
import assert from 'node:assert'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import fs from 'node:fs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const SCRIPT = path.join(ROOT, 'tools/ci/check-gap-register.mjs')

test('test_check_gap_register_invalid_argument_rejects', () => {
  const result = spawnSync('node', [SCRIPT, '--invalid'])
  assert.strictEqual(result.status, 1)
  assert.match(result.stderr.toString(), /Usage:/)
})

test('test_check_gap_register_valid_argument_accepts', () => {
  const result = spawnSync('node', [SCRIPT, '--check'])
  assert.strictEqual(result.status, 0)
  assert.match(result.stdout.toString(), /gap register OK/)
})

test('test_check_gap_register_missing_argument_accepts', () => {
  const result = spawnSync('node', [SCRIPT])
  assert.strictEqual(result.status, 0)
  assert.match(result.stdout.toString(), /gap register OK/)
})
