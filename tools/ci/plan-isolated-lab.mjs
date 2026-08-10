#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const LAB_KINDS = new Set(['windows', 'wsl2'])
const REQUIRED_MODE = 'plan'
const REQUIRED_TARGET = 'isolated-lab'
const REQUIRED_ENVIRONMENT = 'protected-isolated-lab'

function safeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !path.isAbsolute(value) &&
    !/^[A-Za-z]:[\\/]/.test(value) && !value.split(/[\\/]/).includes('..')
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

function add(errors, rule) {
  if (!errors.includes(rule)) errors.push(rule)
}

export function validateLabPlanInput(input) {
  const errors = []
  if (!input || !LAB_KINDS.has(input.kind)) add(errors, 'lab-kind-invalid')
  if (!input || input.mode !== REQUIRED_MODE) add(errors, 'lab-mode-invalid')
  if (!input || input.target !== REQUIRED_TARGET) add(errors, 'lab-target-invalid')
  if (!input || typeof input.revision !== 'string' || !/^[0-9a-f]{40}$/.test(input.revision)) add(errors, 'lab-revision-invalid')
  if (!input || input.environment !== REQUIRED_ENVIRONMENT) add(errors, 'lab-environment-invalid')
  return { ok: errors.length === 0, errors: errors.sort() }
}

export function buildLabPlan(input) {
  const checked = validateLabPlanInput(input)
  if (!checked.ok) throw new Error('lab-plan-input-invalid')
  return {
    schema_version: 1,
    kind: input.kind,
    mode: REQUIRED_MODE,
    target: REQUIRED_TARGET,
    revision: input.revision,
    environment: REQUIRED_ENVIRONMENT,
    host_action: 'none',
    terminal_status: 'PASS',
  }
}

export function writeLabPlan(input, { outPath, manifestPath } = {}) {
  if (typeof outPath !== 'string' || typeof manifestPath !== 'string' ||
      path.dirname(path.resolve(outPath)) !== path.dirname(path.resolve(manifestPath))) {
    throw new Error('lab-plan-output-invalid')
  }
  const plan = buildLabPlan(input)
  const planBytes = Buffer.from(`${JSON.stringify(plan, null, 2)}\n`)
  writeFileSync(outPath, planBytes)
  const manifest = {
    schema_version: 1,
    source_sha: plan.revision,
    terminal_status: 'PASS',
    artifacts: [{
      path: path.basename(outPath),
      class: 'lab-plan',
      attachment: 'workflow',
      retention_days: 14,
      bytes: planBytes.length,
      sha256: sha256(planBytes),
      sanitized: true,
      summary: `${plan.kind} plan-only isolated-lab result`,
    }],
  }
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  return { plan, manifest }
}

function parseArguments(argv) {
  const values = new Map()
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]
    const value = argv[index + 1]
    if (!['--kind', '--out', '--manifest'].includes(key) || typeof value !== 'string' || values.has(key)) return null
    values.set(key, value)
  }
  if (argv.length !== 6 || values.size !== 3) return null
  return { kind: values.get('--kind'), out: values.get('--out'), manifest: values.get('--manifest') }
}

function resolveOutput(cwd, value) {
  if (!safeRelative(value)) return null
  const root = path.resolve(cwd)
  const output = path.resolve(root, value)
  return output.startsWith(`${root}${path.sep}`) ? output : null
}

export function main(argv = process.argv.slice(2), {
  env = process.env,
  cwd = process.cwd(),
  print = console.log,
  error = console.error,
} = {}) {
  const args = parseArguments(argv)
  if (!args) {
    error('usage: plan-isolated-lab.mjs --kind <windows|wsl2> --out <path> --manifest <path>')
    return 2
  }
  const outPath = resolveOutput(cwd, args.out)
  const manifestPath = resolveOutput(cwd, args.manifest)
  if (!outPath || !manifestPath || path.dirname(outPath) !== path.dirname(manifestPath)) {
    error('LAB_PLAN_ERROR=lab-output-path-invalid')
    return 1
  }
  const input = {
    kind: args.kind,
    mode: env.LAB_MODE,
    target: env.LAB_TARGET,
    revision: env.LAB_REVISION,
    environment: env.LAB_ENVIRONMENT,
  }
  const checked = validateLabPlanInput(input)
  if (!checked.ok) {
    for (const rule of checked.errors) error(`LAB_PLAN_ERROR=${rule}`)
    print('LAB_PLAN_STATUS=FAIL')
    return 1
  }
  try {
    writeLabPlan(input, { outPath, manifestPath })
  } catch {
    error('LAB_PLAN_ERROR=lab-write-failed')
    print('LAB_PLAN_STATUS=FAIL')
    return 1
  }
  print('LAB_PLAN_STATUS=PASS')
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === new URL(import.meta.url).pathname) process.exitCode = main()
