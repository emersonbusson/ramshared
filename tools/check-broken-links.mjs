#!/usr/bin/env node
/**
 * Broken-link checker for markdown docs — RamShared.
 *
 * Default: scan only under docs/ (specs, ADRs, methodology).
 * Full repo: node tools/check-broken-links.mjs --all
 *
 * Skips:
 *   - http(s)/mailto/absolute paths
 *   - glob-like targets containing * or ?
 *   - file:// and other schemes
 *
 * Pure Node ESM, no deps.
 */
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const SCAN_ALL = process.argv.includes("--all");
const SCAN_ROOT = SCAN_ALL ? REPO_ROOT : join(REPO_ROOT, "docs");

const SKIP_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "out",
  "coverage",
  "build",
  "artifacts",
  ".session",
  ".claude",
  ".agents",
  ".codex",
  ".grok",
]);

function* walk(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    if (e.isDirectory()) {
      if (SKIP_DIRS.has(e.name)) continue;
      yield* walk(join(dir, e.name));
    } else if (e.isFile()) {
      yield join(dir, e.name);
    }
  }
}

/** Collect all .md under repo so targets outside docs/ still resolve when scanning docs. */
const allMd = new Set();
for (const f of walk(REPO_ROOT)) {
  if (f.endsWith(".md")) allMd.add(f);
}

const filesToCheck = [];
for (const f of walk(SCAN_ROOT)) {
  if (f.endsWith(".md")) filesToCheck.push(f);
}

const linkRegex = /\[([^\]]*)\]\(([^)\s]+)\)/g;
const broken = [];

for (const src of filesToCheck) {
  let content;
  try {
    content = readFileSync(src, "utf8");
  } catch {
    continue;
  }
  const srcDir = dirname(src);
  let m;
  const re = new RegExp(linkRegex.source, "g");
  while ((m = re.exec(content)) !== null) {
    let href = m[2].trim();
    // strip optional title: path "title"
    const spaceQ = href.match(/^([^\s]+)\s+"/);
    if (spaceQ) href = spaceQ[1];

    if (
      href.startsWith("http://") ||
      href.startsWith("https://") ||
      href.startsWith("mailto:") ||
      href.startsWith("#") ||
      href.startsWith("/") ||
      href.includes("://")
    ) {
      continue;
    }
    // Only check markdown targets
    const pathPart = href.split("#")[0];
    if (!pathPart.endsWith(".md")) continue;
    if (pathPart.includes("*") || pathPart.includes("?")) continue;

    let decoded;
    try {
      decoded = decodeURIComponent(pathPart);
    } catch {
      broken.push({ src: relative(REPO_ROOT, src), link: pathPart });
      continue;
    }
    const target = resolve(srcDir, decoded);
    if (allMd.has(target) || existsSync(target)) continue;
    broken.push({ src: relative(REPO_ROOT, src), link: pathPart });
  }
}

if (broken.length === 0) {
  process.stdout.write(
    `✓ no broken markdown links (scan=${SCAN_ALL ? "repo" : "docs/"})\n`,
  );
  process.exit(0);
}

process.stdout.write(
  `✗ ${broken.length} broken markdown link(s) (scan=${SCAN_ALL ? "repo" : "docs/"}):\n`,
);
for (const b of broken.slice(0, 50)) {
  process.stdout.write(`  ${b.src} -> ${b.link}\n`);
}
if (broken.length > 50) {
  process.stdout.write(`  ... ${broken.length - 50} more\n`);
}
process.exit(1);
