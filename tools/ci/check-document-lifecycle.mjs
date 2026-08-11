#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const POLICY_PATH = 'docs/governance/document-lifecycle-policy.json'
const LIFECYCLES = new Set(['immutable', 'historical', 'reviewable'])

function finding(code, detail, pathname = POLICY_PATH) {
  return { path: pathname, code, detail }
}

function sorted(findings) {
  return findings.sort((left, right) => left.path.localeCompare(right.path) || left.code.localeCompare(right.code) || left.detail.localeCompare(right.detail))
}

function isSafeRelative(value) {
  return typeof value === 'string' && value.length > 0 && !value.startsWith('/') && !value.includes('\\') && !value.split('/').includes('..') && !/^[A-Za-z]:/.test(value)
}

function safeGlob(value) {
  return isSafeRelative(value) && !value.includes('//') && !value.includes('{') && !value.includes('}')
}

function dateIsSafe(value, now) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(value)) return 'invalid-timestamp'
  const parsed = new Date(value)
  if (Number.isNaN(parsed.valueOf())) return 'invalid-timestamp'
  return parsed > now ? 'future-metadata' : null
}

function globToRegExp(glob) {
  let expression = ''
  for (let index = 0; index < glob.length; index++) {
    const char = glob[index]
    if (char === '*') {
      if (glob[index + 1] === '*') {
        index++
        if (glob[index + 1] === '/') { index++; expression += '(?:.*/)?' } else expression += '.*'
      } else expression += '[^/]*'
    } else if (char === '?') expression += '[^/]'
    else expression += char.replace(/[|\\{}()[\]^$+?.]/g, '\\$&')
  }
  return new RegExp(`^${expression}$`)
}

function matching(items, pathname) {
  return items.filter((item) => globToRegExp(item.pattern).test(pathname))
}

export function classifyDocument(pathname, policy) {
  const exclusions = matching(policy?.exclusions ?? [], pathname)
  if (exclusions.length === 1) return { disposition: 'excluded', ruleId: exclusions[0].id, owner: exclusions[0].owner, reason: exclusions[0].reason, verificationState: 'not-applicable' }
  if (exclusions.length > 1) return { disposition: 'ambiguous', ruleId: exclusions.map((item) => item.id).join(',') }
  const routes = matching(policy?.routes ?? [], pathname)
  if (routes.length === 0) return { disposition: 'unclassified' }
  if (routes.length > 1) return { disposition: 'ambiguous', ruleId: routes.map((item) => item.id).join(',') }
  const route = routes[0]
  return {
    disposition: 'classified', ruleId: route.id, owner: route.owner,
    canonicalSource: route.canonicalSource, lifecycle: route.lifecycle,
    freshnessDays: route.freshnessDays, verificationState: route.verification.state,
  }
}

function validateRule(item, kind, now, findings) {
  const prefix = `${kind}:${item?.id ?? 'missing'}`
  if (!item?.id || !/^[a-z0-9][a-z0-9-]*$/.test(item.id)) findings.push(finding('invalid-id', prefix))
  if (!safeGlob(item?.pattern)) findings.push(finding('unsafe-pattern', prefix))
  if (!item?.owner || !/^[a-z0-9][a-z0-9-]*$/.test(item.owner)) findings.push(finding('invalid-owner', prefix))
  const timestamp = dateIsSafe(item?.registeredAt, now)
  if (timestamp) findings.push(finding(timestamp, prefix))
  if (kind === 'exclusion') {
    if (!item?.reason || item.reason.trim().length < 8) findings.push(finding('invalid-exclusion-reason', prefix))
    return
  }
  if (!isSafeRelative(item?.canonicalSource)) findings.push(finding('unsafe-canonical-source', prefix))
  if (!LIFECYCLES.has(item?.lifecycle)) findings.push(finding('invalid-lifecycle', prefix))
  if (item?.lifecycle === 'reviewable') {
    if (!Number.isInteger(item?.freshnessDays) || item.freshnessDays < 1 || item.freshnessDays > 365) findings.push(finding('invalid-freshness', prefix))
  } else if (item?.freshnessDays !== null) findings.push(finding('invalid-freshness', prefix))
  if (item?.verification?.state !== 'unverified' || Object.keys(item?.verification ?? {}).length !== 1) findings.push(finding('verification-must-be-unverified', prefix))
}

export function validatePolicy(policy, now = new Date()) {
  const findings = []
  if (policy?.schemaVersion !== 'ramshared.document-lifecycle-policy.v1') findings.push(finding('invalid-schema', 'policy'))
  if (!policy?.owner || !/^[a-z0-9][a-z0-9-]*$/.test(policy.owner)) findings.push(finding('invalid-owner', 'policy'))
  const timestamp = dateIsSafe(policy?.registeredAt, now)
  if (timestamp) findings.push(finding(timestamp, 'policy'))
  if (!Array.isArray(policy?.routes) || policy.routes.length === 0) findings.push(finding('missing-routes', 'policy'))
  if (!Array.isArray(policy?.exclusions)) findings.push(finding('missing-exclusions', 'policy'))
  const ids = new Set()
  for (const route of policy?.routes ?? []) {
    validateRule(route, 'route', now, findings)
    if (ids.has(route?.id)) findings.push(finding('duplicate-rule-id', route?.id ?? 'missing'))
    ids.add(route?.id)
  }
  for (const exclusion of policy?.exclusions ?? []) {
    validateRule(exclusion, 'exclusion', now, findings)
    if (ids.has(exclusion?.id)) findings.push(finding('duplicate-rule-id', exclusion?.id ?? 'missing'))
    ids.add(exclusion?.id)
  }
  return sorted(findings)
}

export function listTrackedMarkdown(root = ROOT) {
  return execFileSync('git', ['ls-files', '--', '*.md'], { cwd: root, encoding: 'utf8' }).split(/\r?\n/).filter(Boolean).sort()
}

export function listDocumentPaths(root = ROOT) {
  const tracked = listTrackedMarkdown(root)
  const untracked = execFileSync('git', ['ls-files', '--others', '--exclude-standard', '--', '*.md'], { cwd: root, encoding: 'utf8' }).split(/\r?\n/).filter(Boolean)
  return [...new Set([...tracked, ...untracked])].sort()
}

function readPolicy(root) {
  return JSON.parse(readFileSync(path.join(root, POLICY_PATH), 'utf8'))
}

function changedMarkdown(root, base) {
  return execFileSync('git', ['diff', '--name-only', '--diff-filter=ACMR', `${base}...HEAD`, '--', '*.md'], { cwd: root, encoding: 'utf8' }).split(/\r?\n/).filter(Boolean).sort()
}

export function readBasePolicy(root, base) {
  // Verify the Git object first. A missing policy at a real base is the
  // intentional bootstrap state; an invalid revision remains fail-closed.
  execFileSync('git', ['rev-parse', '--verify', `${base}^{commit}`], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  try {
    execFileSync('git', ['cat-file', '-e', `${base}:${POLICY_PATH}`], { cwd: root, stdio: ['ignore', 'ignore', 'ignore'] })
  } catch {
    return null
  }
  return JSON.parse(execFileSync('git', ['show', `${base}:${POLICY_PATH}`], { cwd: root, encoding: 'utf8' }))
}

function lifecycleStrength(value) {
  return { immutable: 3, reviewable: 2, historical: 1 }[value] ?? 0
}

function comparePolicyForChangedPath(pathname, previous, current, findings) {
  const before = classifyDocument(pathname, previous)
  const after = classifyDocument(pathname, current)
  if (before.disposition === 'classified' && after.disposition !== 'classified') findings.push(finding('policy-regression', 'classified-document-lost-lifecycle', pathname))
  if (before.disposition === 'classified' && after.disposition === 'classified') {
    if (lifecycleStrength(after.lifecycle) < lifecycleStrength(before.lifecycle)) findings.push(finding('policy-regression', 'lifecycle-downgrade', pathname))
    if (before.lifecycle === 'reviewable' && after.lifecycle === 'reviewable' && after.freshnessDays > before.freshnessDays) findings.push(finding('policy-regression', 'freshness-weakened', pathname))
  }
}

export function run({ root = ROOT, policy = null, paths = null, trackedPaths = null, basePolicy = null, changedPaths = null, now = new Date() } = {}) {
  const activePolicy = policy ?? readPolicy(root)
  const documentPaths = paths ?? listDocumentPaths(root)
  const findings = validatePolicy(activePolicy, now)
  const classification = { classified: 0, excluded: 0, unclassified: 0, ambiguous: 0 }
  for (const pathname of documentPaths) {
    const result = classifyDocument(pathname, activePolicy)
    classification[result.disposition]++
    if (result.disposition === 'unclassified') findings.push(finding('unclassified-document', 'no-policy-route', pathname))
    if (result.disposition === 'ambiguous') findings.push(finding('ambiguous-document', result.ruleId, pathname))
    if (result.disposition === 'classified' && !existsSync(path.join(root, result.canonicalSource))) findings.push(finding('missing-canonical-source', result.canonicalSource, pathname))
  }
  if (basePolicy) for (const pathname of changedPaths ?? documentPaths) comparePolicyForChangedPath(pathname, basePolicy, activePolicy, findings)
  const trackedCount = trackedPaths ? trackedPaths.length : paths ? documentPaths.length : listTrackedMarkdown(root).length
  return { ok: findings.length === 0, findings: sorted(findings), classification, counts: { documents: documentPaths.length, tracked: trackedCount } }
}

/* node:coverage disable */
function main(argv = process.argv.slice(2)) {
  let base = null
  if (argv.length === 1 && argv[0] === '--all') {
    // Supported complete local check.
  } else if (argv.length === 3 && argv[0] === '--all' && argv[1] === '--base' && argv[2]) base = argv[2]
  else {
    console.error('usage: check-document-lifecycle.mjs --all [--base <git-revision>]')
    return 2
  }
  let result
  try {
    result = base ? run({ root: ROOT, basePolicy: readBasePolicy(ROOT, base), changedPaths: changedMarkdown(ROOT, base) }) : run({ root: ROOT })
  } catch (error) {
    console.error(`DOCUMENT_LIFECYCLE_STATUS=NO-GO\nERROR=${error instanceof Error ? error.message : 'policy-read-failed'}`)
    return 1
  }
  console.log(`TRACKED_MARKDOWN=${result.counts.tracked}`)
  console.log(`WORKTREE_MARKDOWN=${result.counts.documents}`)
  console.log(`CLASSIFIED=${result.classification.classified}`)
  console.log(`EXCLUDED=${result.classification.excluded}`)
  console.log(`UNCLASSIFIED=${result.classification.unclassified}`)
  for (const item of result.findings) console.error(`${item.path} — ${item.code} — ${item.detail}`)
  console.log(`DOCUMENT_LIFECYCLE_STATUS=${result.ok ? 'PASS' : 'NO-GO'}`)
  return result.ok ? 0 : 1
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main())
/* node:coverage enable */
