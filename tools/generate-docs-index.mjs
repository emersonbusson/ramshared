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
  for (const line of block.split(/\r?\n/)) {
    const m = line.match(/^([a-zA-Z_]+):\s*(.+)$/);
    if (!m) continue;
    const key = m[1];
    const value = m[2].trim();
    if (key === "slug" || key === "title" || key === "milestone") {
      fm[key] = stripQuotes(value);
    } else if (key === "issues") {
      fm.issues = parseIssueList(value);
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
    .map((s) => Number(s))
    .filter((n) => Number.isFinite(n));
}

function deriveStatus(slugDir) {
  const implPath = join(slugDir, "IMPL.md");
  const hasImpl = existsSync(implPath);
  const hasSpec = existsSync(join(slugDir, "SPEC.md"));
  if (hasImpl) {
    // SSDV3: IMPL with Status partial must not be index-quality DONE.
    try {
      const head = readFileSync(implPath, "utf8").slice(0, 4000);
      if (/^##\s*Status\s*$/im.test(head) && /\bpartial\b/i.test(head)) {
        const statusBlock = head.split(/##\s*Status/i)[1] || "";
        const firstLines = statusBlock.split("\n").slice(0, 8).join("\n");
        if (/\bpartial\b/i.test(firstLines) && !/\bimplemented\b/i.test(firstLines)) {
          return "PARTIAL";
        }
      }
    } catch {
      /* fall through to DONE */
    }
    return "DONE";
  }
  if (hasSpec) return "SPEC";
  return "PRD";
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

function listSpecEntries() {
  const entries = [];
  const seen = new Set();

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

  const specsDir = join(DOCS_DIR, "specs");
  if (existsSync(specsDir)) {
    // docs/specs/<slug>/ or docs/specs/<group>/<slug>/
    visit(specsDir, 1, 2);
  }

  // Legacy flat: docs/<slug>/ with PRD/SPEC
  if (existsSync(DOCS_DIR)) {
    let names;
    try {
      names = readdirSync(DOCS_DIR);
    } catch {
      names = [];
    }
    for (const name of names.sort()) {
      if (SKIP_TOP_LEVEL.has(name)) continue;
      const sub = join(DOCS_DIR, name);
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

function buildRows() {
  const rows = [];
  for (const { slug, dir } of listSpecEntries()) {
    const prdPath = join(dir, "PRD.md");
    const fm = readFrontmatter(prdPath) ?? {};
    rows.push({
      slug: fm.slug ?? slug,
      title: fm.title ?? deriveTitleFromPrd(prdPath),
      milestone: fm.milestone ?? "—",
      issues: fm.issues ?? [],
      status: deriveStatus(dir),
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

function renderIndex(rows) {
  const header = [
    "# Specs Index",
    "",
    "> Generated by `node tools/generate-docs-index.mjs`. Do not edit by hand.",
    "> Check: `node tools/generate-docs-index.mjs --check`",
    "",
    "Each row is an SSDV3 feature folder. Status from file presence: `PRD` → only PRD; `SPEC` → SPEC.md present; `DONE` → IMPL.md (implemented); `PARTIAL` → IMPL.md with Status partial (env-bound gaps).",
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
    "| Slug | Title | Milestone | Issues | Status |",
    "| --- | --- | --- | --- | --- |",
  ];
  const indexDir = dirname(INDEX_PATH);
  for (const r of rows) {
    const issues = r.issues.length > 0 ? r.issues.map((n) => `#${n}`).join(", ") : "—";
    const rel = relative(indexDir, r.dir).split("\\").join("/");
    const link = `[\`${r.slug}\`](${rel}/)`;
    table.push(
      `| ${link} | ${escapeCell(r.title)} | ${escapeCell(String(r.milestone))} | ${issues} | ${r.status} |`,
    );
  }
  return [...header, ...table, ""].join("\n");
}

function escapeCell(s) {
  return String(s).replace(/\|/g, "\\|");
}

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

process.exit(main());
