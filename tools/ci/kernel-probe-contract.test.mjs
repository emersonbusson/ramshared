import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const readDriver = (name) => readFileSync(
  path.join(root, 'drivers', 'block', 'ramshared', name),
  'utf8',
)

test('PCI probe rejects a capacity that cannot be backed by BAR0', () => {
  const probe = readDriver('main.c')
  const dma = readDriver('dma.c')

  assert.match(
    probe,
    /bar_len\s*=\s*pci_resource_len\(pdev,\s*0\);[\s\S]*?\(u64\)bar_len\s*<\s*capacity_bytes[\s\S]*?return\s+-ERANGE;/,
  )
  assert.match(
    dma,
    /\(u64\)bar_len\s*<\s*rs_dev->capacity_bytes[\s\S]*?return\s+-ERANGE;/,
  )
  assert.match(
    dma,
    /rs_dev->dma\.size\s*=\s*\(size_t\)rs_dev->capacity_bytes;/,
  )
})

test('PCI probe preserves the PCI enable error for its caller', () => {
  const probe = readDriver('main.c')
  const enableFailure = probe.match(
    /ret\s*=\s*pci_enable_device_mem\(pdev\);\s*if\s*\(ret\)\s*\{([\s\S]*?)\n\t\}/,
  )

  assert.ok(enableFailure, 'the PCI enable failure branch must exist')
  assert.match(enableFailure[1], /return\s+ret;/)
})
