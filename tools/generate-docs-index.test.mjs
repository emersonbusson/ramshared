import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import {
  buildRows,
  deriveStatus,
  loadClaimsRegistry,
  renderIndex,
} from './generate-docs-index.mjs'

function rootFixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'ramshared-index-'))
  mkdirSync(path.join(root, 'docs', 'specs', 'no-milestone'), { recursive: true })
  return root
}

function write(root, relative, text) {
  const target = path.join(root, relative)
  mkdirSync(path.dirname(target), { recursive: true })
  writeFileSync(target, text)
}

test('build_rows_discovers_grouped_and_legacy_specs', () => {
  const root = rootFixture()
  write(root, 'docs/specs/no-milestone/grouped/PRD.md', '# PRD — Grouped\n')
  write(root, 'docs/legacy/PRD.md', '# PRD — Legacy\n')
  write(root, 'docs/methodology/ignored/PRD.md', '# PRD — Ignored\n')
  assert.deepEqual(buildRows(root).map((row) => row.slug), ['grouped', 'legacy'])
})

test('build_rows_uses_frontmatter_and_heading_fallback', () => {
  const root = rootFixture()
  write(root, 'docs/specs/no-milestone/a/PRD.md', '---\nslug: alpha\ntitle: "Alpha | Tier"\nmilestone: M1\nissues: [7, invalid, 9]\n---\n# ignored\n')
  write(root, 'docs/specs/no-milestone/b/PRD.md', '# PRD — Heading fallback\n')
  const rows = buildRows(root)
  assert.equal(rows[0].title, 'Alpha | Tier')
  assert.deepEqual(rows[0].issues, [7, 9])
  assert.equal(rows[1].title, 'PRD — Heading fallback')
})

test('build_rows_parses_block_issue_lists_with_ordered_local_and_external_refs', () => {
  const root = rootFixture()
  write(root, 'docs/specs/no-milestone/mixed/PRD.md', [
    '---',
    'slug: mixed',
    'title: Mixed issue metadata',
    'milestone: M1',
    'issues:',
    '  - 194',
    '  - "owner/repo#195"',
    '  - malformed',
    '  - 196.5',
    '  - 197',
    '---',
    '# ignored',
    '',
  ].join('\n'))
  const rows = buildRows(root)
  assert.deepEqual(rows[0].issues, [194, 'owner/repo#195', 197])
  const rendered = renderIndex(rows, path.join(root, 'docs', 'INDEX.md'))
  assert.match(rendered, /#194, owner\/repo#195, #197/)
  assert.doesNotMatch(rendered, /malformed|196\.5|NaN/)
})

test('repository_block_issue_metadata_renders_all_current_issue_refs', () => {
  const rendered = renderIndex(buildRows(process.cwd()))
  for (const reference of ['#194', '#195', '#196', '#197', 'microsoft/WSL#41054']) {
    assert.match(rendered, new RegExp(reference.replace(/[#/]/g, '\\$&')))
  }
})

test('build_rows_fails_closed_to_unqualified_without_done_claim', () => {
  const root = rootFixture()
  const dir = path.join(root, 'docs/specs/no-milestone/implemented')
  write(root, 'docs/specs/no-milestone/implemented/PRD.md', '# Implemented\n')
  write(root, 'docs/specs/no-milestone/implemented/IMPL.md', '# IMPL\n')
  assert.equal(buildRows(root)[0].status, 'UNQUALIFIED')
  write(root, 'docs/governance/claims.json', JSON.stringify({ schema_version: 1, claims: [{ slug: 'implemented', state: 'DONE' }] }))
  assert.equal(buildRows(root)[0].status, 'DONE')
  assert.equal(deriveStatus(dir, new Map([['implemented', { state: 'PARTIAL' }]])), 'PARTIAL')
})

test('derive_status_distinguishes_prd_and_spec', () => {
  const root = rootFixture()
  const dir = path.join(root, 'docs/specs/no-milestone/state')
  mkdirSync(dir, { recursive: true })
  assert.equal(deriveStatus(dir), 'PRD')
  writeFileSync(path.join(dir, 'SPEC.md'), '# SPEC\n')
  assert.equal(deriveStatus(dir), 'SPEC')
})

test('load_claims_rejects_missing_invalid_and_wrong_schema', () => {
  const root = rootFixture()
  assert.equal(loadClaimsRegistry(root).size, 0)
  write(root, 'docs/governance/claims.json', '{')
  assert.equal(loadClaimsRegistry(root).size, 0)
  write(root, 'docs/governance/claims.json', JSON.stringify({ schema_version: 2, claims: [] }))
  assert.equal(loadClaimsRegistry(root).size, 0)
})

test('render_index_handles_empty_rows_escaping_and_issues', () => {
  assert.match(renderIndex([], '/repo/docs/INDEX.md'), /No specs yet/)
  const rendered = renderIndex([{ slug: 'alpha', title: 'A | B', milestone: '—', issues: [7, 9], status: 'DONE', dir: '/repo/docs/specs/no-milestone/alpha' }], '/repo/docs/INDEX.md')
  assert.match(rendered, /A \\| B/)
  assert.match(rendered, /#7, #9/)
  assert.match(rendered, /\(specs\/no-milestone\/alpha\/\)/)
})

test('index_output_is_deterministic', () => {
  const rows = [
    { slug: 'b', title: 'B', milestone: '—', issues: [], status: 'SPEC', dir: '/repo/docs/b' },
    { slug: 'a', title: 'A', milestone: '—', issues: [], status: 'PRD', dir: '/repo/docs/a' },
  ]
  assert.equal(renderIndex(rows, '/repo/docs/INDEX.md'), renderIndex(structuredClone(rows), '/repo/docs/INDEX.md'))
})
