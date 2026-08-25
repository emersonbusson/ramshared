#!/usr/bin/env node
/**
 * Specs index generator — RamShared.
 *
 * Scans SSDV3 artifacts and writes docs/INDEX.md.
 *
 * Layouts:
 *   1. docs/specs/<slug>/
 *   2. docs/specs/<milestone|no-milestone>/<slug>/
 *   3. Legacy flat: docs/<slug>/ with PRD.md or SPEC.md
 *      (skips methodology/, decisions/, postmortems/, reliability/,
 *       runbooks/, benchmarks/, specs/, libraries/)
 *
 * Status:
 *   IMPL.md present                         -> DONE
 *   SPEC.md present                         -> SPEC
 *   only PRD.md                             -> PRD
 *
 * Usage:
 *   node tools/generate-docs-index.mjs
 *   node tools/generate-docs-index.mjs --check
 */

import { readFileSync, readdirSync, existsSync, writeFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  evaluateClaimClosures,
  loadClaimClosures,
} from "./ci/documentation-claim-closure.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DOCS_DIR = join(REPO_ROOT, "docs");
const INDEX_PATH = join(DOCS_DIR, "INDEX.md");

const SPEC_MARKER_FILES = ["PRD.md", "SPEC.md"];
const SKIP_TOP_LEVEL = new Set([
  "methodology",
  "decisions",
  "postmortems",
  "reliability",
  "runbooks",
  "benchmarks",
  "specs",
  "libraries",
  "reference",
]);

function readFrontmatter(filePath) {
  if (!existsSync(filePath)) return null;
  const raw = readFileSync(filePath, "utf8");
  if (!raw.startsWith("---")) return null;
  const end = raw.indexOf("\n---", 3);
  if (end === -1) return null;
  const block = raw.slice(3, end).trim();
  const fm = {};
  const lines = block.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const m = line.match(/^([a-zA-Z_]+):\s*(.*)$/);
    if (!m) continue;
    const key = m[1];
    const value = m[2].trim();
    if (key === "slug" || key === "title" || key === "milestone") {
      fm[key] = stripQuotes(value);
    } else if (key === "issues") {
      if (value) {
        fm.issues = parseIssueList(value);
        continue;
      }
      const issues = [];
      let next = index + 1;
      for (; next < lines.length; next += 1) {
        const item = lines[next].match(/^\s*-\s*(.*?)\s*$/);
        if (!item) break;
        const parsed = parseIssueEntry(item[1]);
        if (parsed !== null) issues.push(parsed);
      }
      fm.issues = issues;
      index = next - 1;
    }
  }
  return fm;
}

function stripQuotes(s) {
  return s.replace(/^["']|["']$/g, "");
}

function parseIssueList(value) {
  const inner = value.replace(/^\[|\]$/g, "").trim();
  if (!inner) return [];
  return inner
    .split(",")
    .map((s) => s.trim())
    .map(parseIssueEntry)
    .filter((issue) => issue !== null);
}

function parseIssueEntry(value) {
  const normalized = stripQuotes(value.trim());
  if (/^\d+$/.test(normalized)) {
    const issue = Number(normalized);
    return Number.isFinite(issue) ? issue : null;
  }
  if (/^[A-Za-z0-9][A-Za-z0-9_.-]*\/[A-Za-z0-9][A-Za-z0-9_.-]*#\d+$/.test(normalized)) {
    return normalized;
  }
  return null;
}

export function loadClaimsRegistry(root = REPO_ROOT) {
  const claimPath = join(root, "docs", "governance", "claims.json");
  if (!existsSync(claimPath)) return new Map();
  try {
    const parsed = JSON.parse(readFileSync(claimPath, "utf8"));
    if (parsed.schema_version !== 1 || !Array.isArray(parsed.claims)) return new Map();
    return new Map(parsed.claims.map((claim) => [claim.slug, claim]));
  } catch {
    return new Map();
  }
}

function loadClaimsDocument(root = REPO_ROOT) {
  const claimPath = join(root, "docs", "governance", "claims.json");
  if (!existsSync(claimPath)) return { schema_version: 1, claims: [] };
  try {
    const parsed = JSON.parse(readFileSync(claimPath, "utf8"));
    if (parsed.schema_version !== 1 || !Array.isArray(parsed.claims)) return { schema_version: 1, claims: [] };
    return parsed;
  } catch {
    return { schema_version: 1, claims: [] };
  }
}

export function loadValidatedClaims(root = REPO_ROOT) {
  const claims = loadClaimsDocument(root);
  const result = evaluateClaimClosures(claims, loadClaimClosures(root), { root });
  const registryInvalid = result.findings.some((item) => item.startsWith("claim-closures:"));
  if (registryInvalid) {
    for (const [slug, evaluation] of result.evaluations) {
      if (evaluation.status === "DONE") result.evaluations.set(slug, { ...evaluation, qualified: false, status: "STALE", revision: null });
    }
  }
  return result.evaluations;
}

export function deriveStatus(slugDir, claims = new Map()) {
  const implPath = join(slugDir, "IMPL.md");
  const hasImpl = existsSync(implPath);
  const hasSpec = existsSync(join(slugDir, "SPEC.md"));
  if (hasImpl) {
    const slug = slugDir.split(/[\\/]/).filter(Boolean).at(-1);
    const claim = claims.get(slug);
    return claim?.status ?? "UNQUALIFIED";
  }
  if (hasSpec) return "SPEC";
  return "PRD";
}

function deriveRevision(slugDir, claims = new Map()) {
  if (!existsSync(join(slugDir, "IMPL.md"))) return "—";
  const slug = slugDir.split(/[\\/]/).filter(Boolean).at(-1);
  const revision = claims.get(slug)?.revision;
  return typeof revision === "string" ? revision.slice(0, 12) : "—";
}

function deriveTitleFromPrd(filePath) {
  if (!existsSync(filePath)) return "(no title)";
  const raw = readFileSync(filePath, "utf8");
  const stripped = raw.replace(/^---[\s\S]*?\n---\s*\n?/, "");
  const m = stripped.match(/^#\s+(.+)$/m);
  return m ? m[1].trim() : "(no title)";
}

function isSpecFolder(dir) {
  return SPEC_MARKER_FILES.some((f) => existsSync(join(dir, f)));
}

function listSpecEntries(root = REPO_ROOT) {
  const entries = [];
  const seen = new Set();
  const docsDir = join(root, "docs");

  function push(name, dir) {
    const key = resolve(dir);
    if (seen.has(key)) return;
    seen.add(key);
    entries.push({ slug: name, dir });
  }

  function visit(dir, depth, maxDepth) {
    if (depth > maxDepth) return;
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return;
    }
    for (const name of names.sort()) {
      const sub = join(dir, name);
      let isDir = false;
      try {
        isDir = statSync(sub).isDirectory();
      } catch {
        isDir = false;
      }
      if (!isDir) continue;
      if (isSpecFolder(sub)) {
        push(name, sub);
      } else if (depth < maxDepth) {
        visit(sub, depth + 1, maxDepth);
      }
    }
  }

  const specsDir = join(docsDir, "specs");
  if (existsSync(specsDir)) {
    // docs/specs/<slug>/ or docs/specs/<group>/<slug>/
    visit(specsDir, 1, 2);
  }

  // Legacy flat: docs/<slug>/ with PRD/SPEC
  if (existsSync(docsDir)) {
    let names;
    try {
      names = readdirSync(docsDir);
    } catch {
      names = [];
    }
    for (const name of names.sort()) {
      if (SKIP_TOP_LEVEL.has(name)) continue;
      const sub = join(docsDir, name);
      try {
        if (!statSync(sub).isDirectory()) continue;
      } catch {
        continue;
      }
      if (isSpecFolder(sub)) push(name, sub);
    }
  }

  return entries;
}

export function buildRows(root = REPO_ROOT) {
  const rows = [];
  const claims = loadValidatedClaims(root);
  for (const { slug, dir } of listSpecEntries(root)) {
    const prdPath = join(dir, "PRD.md");
    const fm = readFrontmatter(prdPath) ?? {};
    rows.push({
      slug: fm.slug ?? slug,
      title: fm.title ?? deriveTitleFromPrd(prdPath),
      milestone: fm.milestone ?? "—",
      issues: fm.issues ?? [],
      status: deriveStatus(dir, claims),
      revision: deriveRevision(dir, claims),
      dir,
    });
  }
  // Deterministic: slug then path
  rows.sort((a, b) => {
    const s = String(a.slug).localeCompare(String(b.slug));
    if (s !== 0) return s;
    return a.dir.localeCompare(b.dir);
  });
  return rows;
}

export function renderIndex(rows, indexPath = INDEX_PATH) {
  const header = [
    "# Specs Index",
    "",
    "> Generated by `node tools/generate-docs-index.mjs`. Do not edit by hand.",
    "> Check: `node tools/generate-docs-index.mjs --check`",
    "",
    "Each row is an SSDV3 feature folder. `PRD`/`SPEC` are document stages. An `IMPL.md` without a validated claim closure is `UNQUALIFIED`; an invalid recorded closure is `STALE`. `PARTIAL` and `BLOCKED` remain non-promotional and show no revision unless their closure is validated.",
    "",
    "Canonical layout: `docs/specs/no-milestone/{slug}/`. Flat `docs/{slug}/` is legacy (README stub only).",
    "Process: [`SSDV3-PROMPTS.md`](SSDV3-PROMPTS.md) · rules: [`.claude/rules/ssdv3.md`](../.claude/rules/ssdv3.md).",
    "",
  ];
  if (rows.length === 0) {
    header.push(
      "No specs yet. Create `docs/specs/no-milestone/<slug>/PRD.md` via SSDV3 Passo 1.",
      "",
    );
    return header.join("\n");
  }
  const table = [
    "| Slug | Title | Milestone | Issues | Status | Qualified revision |",
    "| --- | --- | --- | --- | --- | --- |",
  ];
  const indexDir = dirname(indexPath);
  for (const r of rows) {
    const issues = r.issues.length > 0
      ? r.issues.map((issue) => typeof issue === "number" ? `#${issue}` : issue).join(", ")
      : "—";
    const rel = relative(indexDir, r.dir).split("\\").join("/");
    const link = `[\`${r.slug}\`](${rel}/)`;
    table.push(
      `| ${link} | ${escapeCell(r.title)} | ${escapeCell(String(r.milestone))} | ${issues} | ${r.status} | ${r.revision ?? "—"} |`,
    );
  }
  return [...header, ...table, ""].join("\n");
}

function escapeCell(s) {
  return String(s).replace(/\|/g, "\\|");
}

/* node:coverage disable */
function main() {
  const checkOnly = process.argv.includes("--check");
  const next = renderIndex(buildRows());

  if (checkOnly) {
    const current = existsSync(INDEX_PATH) ? readFileSync(INDEX_PATH, "utf8") : "";
    if (current.trim() === next.trim()) {
      process.stdout.write("✓ docs/INDEX.md is in sync.\n");
      return 0;
    }
    process.stdout.write(
      "✗ docs/INDEX.md is out of sync. Run: node tools/generate-docs-index.mjs\n",
    );
    return 1;
  }

  writeFileSync(INDEX_PATH, next, "utf8");
  process.stdout.write(`✓ wrote ${INDEX_PATH} (${buildRows().length} specs)\n`);
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main());
}
/* node:coverage enable */
