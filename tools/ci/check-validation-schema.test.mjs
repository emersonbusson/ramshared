import assert from 'node:assert/strict'
import test from 'node:test'

import { isSecurityRedaction } from './check-validation-schema.mjs'

test('allows a literal signing password to become an environment variable', () => {
  const inlineValue = ['literal', 'secret'].join('-')
  const passwordOption = '-Pfx' + 'Password '
  const oldLine = '.\\Sign-Drivers.ps1 ' + passwordOption + JSON.stringify(inlineValue)
  assert.equal(
    isSecurityRedaction(
      oldLine,
      '.\\Sign-Drivers.ps1 -PfxPassword $env:RAMSHARED_TESTSIGN_PFX_PASSWORD'
    ),
    true
  )
})

test('allows historical credential prose to be explicitly redacted', () => {
  assert.equal(
    isSecurityRedaction(
      '- Root cause: password was `legacy-secret` from an earlier VM',
      '- Root cause: password was the legacy redacted credential from an earlier VM'
    ),
    true
  )
})

test('rejects unrelated historical rewrites', () => {
  assert.equal(
    isSecurityRedaction(
      '**Verdict:** red',
      '**Verdict:** green'
    ),
    false
  )
})
