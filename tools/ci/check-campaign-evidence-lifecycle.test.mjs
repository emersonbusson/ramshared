import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import test from 'node:test'

import {
  buildEvidenceCatalog,
  renderEvidenceCatalog,
  runCli,
  validateCampaignManifest,
  validateProspectiveEvidence,
  validateRepository,
} from './check-campaign-evidence-lifecycle.mjs'

function sha256(value) {
  return createHash('sha256').update(value).digest('hex')
}

function fixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), 'ramshared-campaign-evidence-'))
  mkdirSync(join(root, 'docs/specs/no-milestone/wsl2-freeze/evidence'), {
    recursive: true,
  })
  return root
}

function write(root, relative, content) {
  const destination = join(root, relative)
  mkdirSync(dirname(destination), { recursive: true })
  writeFileSync(destination, content)
  return destination
}

function git(root, args) {
  return execFileSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
}

function policy() {
  return {
    schema_version: 1,
    limits: {
      max_artifacts_per_run: 8,
      max_artifact_bytes: 4096,
      max_total_artifact_bytes: 16384,
      max_catalog_entries: 128,
    },
    legacy: {
      owner_role: 'reliability-evidence',
      classification: 'legacy-unqualified',
      retention_class: 'historical-immutable',
      immutable: true,
      reason: 'predates campaign lifecycle v1',
    },
    roots: [
      {
        prefix: 'docs/specs/no-milestone/wsl2-freeze/evidence/',
        owner_role: 'wsl2-reliability',
        surface: 'wsl2-freeze',
      },
    ],
  }
}

function repositoryPolicy() {
  return { ...policy(), schema_version: 'ramshared-campaign-evidence-policy/v1' }
}

function completeManifest({ runRelative, artifactRelative, artifact }) {
  return {
    schema_version: 'ramshared-campaign-evidence/v1',
    run_id: 'freeze-20260811-001',
    owner_role: 'wsl2-reliability',
    surface: 'wsl2-freeze',
    lifecycle: 'complete',
    claim_state: 'PASS',
    started_at: '2026-08-11T12:00:00Z',
    finished_at: '2026-08-11T12:01:00Z',
    published_at: '2026-08-11T12:02:00Z',
    source: { commit: 'a'.repeat(40), dirty: false },
    environment: { tier: 'isolated', sanitization: 'public' },
    before: 'swap inactive and daemon absent',
    action: 'isolated native harness completed',
    after: 'terminal cleanup has zero residue',
    legitimate: { name: 'bounded native lifecycle', verdict: 'PASS' },
    refusals: [{ name: 'missing binary identity', verdict: 'PASS' }],
    cleanup: { complete: true, residue: 0 },
    artifacts: [
      {
        path: artifactRelative,
        bytes: Buffer.byteLength(artifact),
        sha256: sha256(artifact),
        sanitized: true,
      },
    ],
    retention: { class: 'repository-evidence', immutable: true },
    rollback_trigger: 'one altered artifact or nonzero cleanup residue',
    _run_relative: runRelative,
  }
}

test('valid_complete_run_passes', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifactRelative = 'before-after.json'
    const artifact = '{"status":"sanitized"}\n'
    write(root, `${runRelative}/${artifactRelative}`, artifact)
    const manifest = completeManifest({ runRelative, artifactRelative, artifact })
    delete manifest._run_relative

    assert.deepEqual(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }),
      [],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('writing_and_failed_runs_remain_nonpromoting', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    manifest.lifecycle = 'writing'
    manifest.claim_state = 'PASS'
    delete manifest.finished_at
    delete manifest.published_at
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /writing.*pass|writing.*published/i,
    )

    manifest.lifecycle = 'failed'
    manifest.claim_state = 'FAIL'
    manifest.finished_at = '2026-08-11T12:01:00Z'
    assert.deepEqual(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }),
      [],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('unsafe_or_sensitive_artifact_is_refused', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = 'password=not-public\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: '../outside.json',
      artifact,
    })
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-path|artifact-sensitive/i,
    )

    manifest.artifacts[0].path = 'before-after.json'
    manifest.artifacts[0].sha256 = sha256(artifact)
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-sensitive/i,
    )

    const outside = write(root, 'outside.json', '{}\n')
    rmSync(join(root, runRelative, 'before-after.json'))
    symlinkSync(outside, join(root, runRelative, 'before-after.json'))
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-symlink/i,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('inventory_tamper_and_orphan_are_refused', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    write(root, `${runRelative}/orphan.tmp`, 'temporary\n')
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-orphan/i,
    )
    rmSync(join(root, runRelative, 'orphan.tmp'))
    write(root, `${runRelative}/before-after.json`, '{"status":"tampered"}\n')
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-hash|artifact-bytes/i,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('historical_catalog_is_observed_not_qualified', () => {
  const root = fixtureRoot()
  try {
    write(
      root,
      'docs/specs/no-milestone/wsl2-freeze/evidence/historical/summary.txt',
      'historical observation\n',
    )
    const catalog = buildEvidenceCatalog({ root, policy: policy() })
    assert.equal(catalog.entries.length, 1)
    assert.equal(catalog.entries[0].classification, 'legacy-unqualified')
    assert.equal(catalog.entries[0].promotion_eligible, false)
    assert.match(renderEvidenceCatalog(catalog), /legacy-unqualified/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('catalog_ignores_untracked_local_evidence', () => {
  const root = fixtureRoot()
  try {
    const tracked =
      'docs/specs/no-milestone/wsl2-freeze/evidence/historical/summary.txt'
    const ignored =
      'docs/specs/no-milestone/wsl2-freeze/evidence/historical/local-forensics.log'
    write(root, tracked, 'tracked historical observation\n')
    write(root, ignored, 'local only\n')

    const catalog = buildEvidenceCatalog({
      root,
      policy: policy(),
      trackedPaths: new Set([tracked]),
    })

    assert.deepEqual(catalog.entries.map((entry) => entry.path), [tracked])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('cli_catalog_generation_uses_tracked_git_paths', () => {
  const root = fixtureRoot()
  try {
    const tracked =
      'docs/specs/no-milestone/wsl2-freeze/evidence/historical/summary.txt'
    const ignored =
      'docs/specs/no-milestone/wsl2-freeze/evidence/historical/local-forensics.log'
    write(root, tracked, 'tracked historical observation\n')
    write(root, 'docs/governance/campaign-evidence-lifecycle.json', `${JSON.stringify(repositoryPolicy())}\n`)
    git(root, ['init', '--quiet'])
    git(root, ['add', 'docs/governance/campaign-evidence-lifecycle.json', tracked])

    assert.equal(runCli(['--generate'], root), 0)
    git(root, ['add', 'docs/governance/campaign-evidence-catalog.generated.json'])
    git(root, [
      '-c', 'user.name=RamShared fixture',
      '-c', 'user.email=fixture@localhost',
      'commit', '--quiet', '-m', 'fixture evidence',
    ])
    write(root, ignored, 'local only\n')

    assert.equal(runCli(['--check'], root), 0)
    assert.equal(runCli(['--check', '--base', 'HEAD'], root), 0)
    assert.equal(runCli(['--check', '--base', 'missing-ref'], root), 1)
    assert.equal(runCli(['--unexpected'], root), 64)
    const catalog = JSON.parse(readFileSync(join(root, 'docs/governance/campaign-evidence-catalog.generated.json'), 'utf8'))
    assert.deepEqual(catalog.entries.map((entry) => entry.path), [tracked])
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('cli_refuses_missing_policy_or_tracked_source', () => {
  const withoutPolicy = fixtureRoot()
  const withoutGit = fixtureRoot()
  try {
    assert.equal(runCli(['--generate'], withoutPolicy), 1)

    write(
      withoutGit,
      'docs/governance/campaign-evidence-lifecycle.json',
      `${JSON.stringify(repositoryPolicy())}\n`,
    )
    assert.equal(runCli(['--generate'], withoutGit), 1)
  } finally {
    rmSync(withoutPolicy, { recursive: true, force: true })
    rmSync(withoutGit, { recursive: true, force: true })
  }
})

test('diff_ratchet_refuses_new_unmanifested_evidence', () => {
  const root = fixtureRoot()
  try {
    const relative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/new-campaign/summary.json'
    write(root, relative, '{}\n')
    assert.deepEqual(
      validateProspectiveEvidence({
        changedPaths: [relative],
        root,
        policy: policy(),
      }),
      ['new-evidence-manifest-missing:docs/specs/no-milestone/wsl2-freeze/evidence/new-campaign'],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('prospective_ratchet_ignores_untracked_local_evidence', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    const artifactPath = `${runRelative}/before-after.json`
    const manifestPath = `${runRelative}/campaign-manifest.json`
    write(root, artifactPath, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    delete manifest._run_relative
    write(root, manifestPath, `${JSON.stringify(manifest)}\n`)
    write(root, `${runRelative}/local-forensics.log`, 'local only\n')

    assert.deepEqual(
      validateProspectiveEvidence({
        changedPaths: [manifestPath, artifactPath],
        root,
        policy: policy(),
        trackedPaths: new Set([manifestPath, artifactPath]),
      }),
      [],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('repository_check_requires_a_current_catalog_and_closed_manifest', () => {
  const root = fixtureRoot()
  try {
    const activePolicy = repositoryPolicy()
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    delete manifest._run_relative
    write(root, `${runRelative}/campaign-manifest.json`, `${JSON.stringify(manifest)}\n`)
    const trackedPaths = new Set([
      `${runRelative}/before-after.json`,
      `${runRelative}/campaign-manifest.json`,
    ])
    write(
      root,
      'docs/governance/campaign-evidence-lifecycle.json',
      `${JSON.stringify(activePolicy)}\n`,
    )
    const catalog = buildEvidenceCatalog({ root, policy: activePolicy, trackedPaths })
    write(root, 'docs/governance/campaign-evidence-catalog.generated.json', renderEvidenceCatalog(catalog))

    assert.deepEqual(validateRepository({ root, trackedPaths }).findings, [])
    assert.match(
      validateRepository({ root, trackedPaths, base: 'missing-ref' }).findings.join('\n'),
      /base-diff/,
    )
    write(root, 'docs/governance/campaign-evidence-catalog.generated.json', '{}\n')
    assert.match(validateRepository({ root, trackedPaths }).findings.join('\n'), /catalog-stale/)
    const manifestPath = join(root, runRelative, 'campaign-manifest.json')
    const outside = write(root, 'outside-manifest.json', '{}\n')
    rmSync(manifestPath)
    symlinkSync(outside, manifestPath)
    assert.match(validateRepository({ root, trackedPaths }).findings.join('\n'), /manifest:.*manifest-parse/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('manifest_refuses_untracked_and_malformed_inputs', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    const artifactPath = `${runRelative}/before-after.json`
    write(root, artifactPath, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })

    manifest.started_at = 'not-a-timestamp'
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /started-timestamp/,
    )

    manifest.started_at = '2026-99-99T99:99:99Z'
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /started-timestamp/,
    )

    manifest.started_at = '2026-08-11T12:00:00Z'
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: {},
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /policy-schema/,
    )
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
        trackedPaths: new Set([`${runRelative}/campaign-manifest.json`]),
      }).join('\n'),
      /artifact-untracked/,
    )

    manifest.artifacts[0].bytes = -1
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-metadata/,
    )

    manifest.artifacts = []
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifacts/,
    )

    manifest.artifacts = [{
      path: 'missing.json',
      bytes: Buffer.byteLength(artifact),
      sha256: sha256(artifact),
      sanitized: true,
    }]
    assert.match(
      validateCampaignManifest(manifest, {
        root,
        runRelative,
        policy: policy(),
        now: '2026-08-11T12:03:00Z',
      }).join('\n'),
      /artifact-missing/,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('repository_check_refuses_missing_tracked_file_source', () => {
  const root = fixtureRoot()
  try {
    write(
      root,
      'docs/governance/campaign-evidence-lifecycle.json',
      `${JSON.stringify(repositoryPolicy())}\n`,
    )

    assert.match(validateRepository({ root }).findings.join('\n'), /tracked-files/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('manifest_refuses_invalid_temporal_and_custody_metadata', () => {
  const root = fixtureRoot()
  try {
    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"status":"sanitized"}\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    manifest.finished_at = '2026-08-11T11:59:00Z'
    manifest.published_at = '2036-08-11T12:02:00Z'
    manifest.source.commit = 'BAD'
    manifest.environment = { tier: 'unknown', sanitization: 'private' }
    manifest.cleanup = { complete: false, residue: 1 }
    manifest.artifacts.push({ ...manifest.artifacts[0] })
    const findings = validateCampaignManifest(manifest, {
      root,
      runRelative,
      policy: policy(),
      now: '2026-08-11T12:03:00Z',
    }).join('\n')
    assert.match(findings, /finished-before-started/)
    assert.match(findings, /published-future/)
    assert.match(findings, /source/)
    assert.match(findings, /environment/)
    assert.match(findings, /complete-cleanup/)
    assert.match(findings, /artifact-path/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('catalog_bounds_legacy_artifacts_and_valid_manifest_ratchet', () => {
  const root = fixtureRoot()
  try {
    const activePolicy = policy()
    activePolicy.limits.max_artifact_bytes = 8
    activePolicy.limits.max_total_artifact_bytes = 16
    write(root, 'docs/specs/no-milestone/wsl2-freeze/evidence/historical/large.txt', 'too-large\n')
    const catalog = buildEvidenceCatalog({ root, policy: activePolicy })
    assert.equal(catalog.entries[0].bounded, false)
    assert.match(catalog.findings.join('\n'), /legacy-artifact-size-limit/)

    const runRelative =
      'docs/specs/no-milestone/wsl2-freeze/evidence/freeze-20260811-001'
    const artifact = '{"ok":1}\n'
    write(root, `${runRelative}/before-after.json`, artifact)
    const manifest = completeManifest({
      runRelative,
      artifactRelative: 'before-after.json',
      artifact,
    })
    delete manifest._run_relative
    write(root, `${runRelative}/campaign-manifest.json`, `${JSON.stringify(manifest)}\n`)
    assert.deepEqual(
      validateProspectiveEvidence({
        changedPaths: [
          `${runRelative}/campaign-manifest.json`,
          `${runRelative}/before-after.json`,
        ],
        root,
        policy: policy(),
      }),
      [],
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('catalog_discovery_is_bounded_and_refuses_historical_symlinks', () => {
  const root = fixtureRoot()
  try {
    const activePolicy = policy()
    activePolicy.limits.max_catalog_entries = 1
    const evidenceRoot = 'docs/specs/no-milestone/wsl2-freeze/evidence/historical'
    const outside = write(root, 'outside.txt', 'outside\n')
    mkdirSync(join(root, evidenceRoot), { recursive: true })
    symlinkSync(outside, join(root, evidenceRoot, 'aaa-link.txt'))
    write(root, `${evidenceRoot}/bbb.txt`, 'bounded\n')
    write(root, `${evidenceRoot}/ccc.txt`, 'not scanned\n')
    const catalog = buildEvidenceCatalog({ root, policy: activePolicy })
    assert.match(catalog.findings.join('\n'), /artifact-symlink/)
    assert.match(catalog.findings.join('\n'), /catalog-entry-limit/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
