#!/usr/bin/env node
/**
 * Broken-link checker for markdown docs — RamShared.
 */
import { readdirSync, readFileSync, existsSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export async function checkExternalUrl(url, maxRetries = 3, timeoutMs = 5000, delayFactor = 1000) {
  for (let i = 0; i <= maxRetries; i++) {
    try {
      const response = await fetch(url, {
        method: "HEAD",
        signal: AbortSignal.timeout(timeoutMs),
      });

      if (response.ok) return { ok: true };

      if (response.status === 429 || response.status >= 500) {
        if (i === maxRetries) return { ok: false, status: response.status };
        await new Promise((r) => setTimeout(r, delayFactor * (2 ** i)));
        continue;
      }

      if (response.status === 405 || response.status === 403) {
        const getRes = await fetch(url, {
          method: "GET",
          signal: AbortSignal.timeout(timeoutMs),
        });
        if (getRes.ok) return { ok: true };
        if (getRes.status === 429 || getRes.status >= 500) {
          if (i === maxRetries) return { ok: false, status: getRes.status };
          await new Promise((r) => setTimeout(r, delayFactor * (2 ** i)));
          continue;
        }
        return { ok: false, status: getRes.status };
      }

      return { ok: false, status: response.status };
    } catch (err) {
      if (i === maxRetries) return { ok: false, error: err.name || err.message };
      await new Promise((r) => setTimeout(r, delayFactor * (2 ** i)));
    }
  }
}

export function validateArgs(argv) {
  if (!Array.isArray(argv)) {
    return { error: 'args-not-array' };
  }
  let scanAll = false;
  let checkExternal = false;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === undefined || (arg.startsWith('--') && arg !== '--all' && arg !== '--external')) {
      return { error: `invalid-arg=${arg}` };
    }
    if (arg === '--all') scanAll = true;
    if (arg === '--external') checkExternal = true;
  }
  return { scanAll, checkExternal };
}

export async function main() {
  const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
  const REPO_ROOT = resolve(SCRIPT_DIR, "..");

  const argsParse = validateArgs(process.argv.slice(2));
  if (argsParse.error) {
    process.stderr.write(`{"error": "${argsParse.error}"}\n`);
    process.exit(1);
  }

  const { scanAll, checkExternal } = argsParse;
  const SCAN_ROOT = scanAll ? REPO_ROOT : join(REPO_ROOT, "docs");

  if (!existsSync(SCAN_ROOT) || !statSync(SCAN_ROOT).isDirectory()) {
    process.stderr.write(`{"error": "invalid-root=${SCAN_ROOT}"}\n`);
    process.exit(1);
  }

  const SKIP_DIRS = new Set([
    "node_modules", ".git", "target", "dist", "out", "coverage",
    "build", "artifacts", ".session", ".claude", ".agents",
    ".codex", ".grok", "local"
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
  const externalCache = new Map();

  for (const src of filesToCheck) {
    let content;
    try {
      content = readFileSync(src, "utf8");
    } catch (err) {
      if (err.code === 'ENOENT' || err.code === 'EACCES') continue;
      throw err;
    }
    const srcDir = dirname(src);
    let m;
    const re = new RegExp(linkRegex.source, "g");
    while ((m = re.exec(content)) !== null) {
      let href = m[2].trim();
      const spaceQ = href.match(/^([^\s]+)\s+"/);
      if (spaceQ) href = spaceQ[1];

      if (href.startsWith("mailto:") || href.startsWith("#") || href.startsWith("/")) {
        continue;
      }

      const isExternal = href.startsWith("http://") || href.startsWith("https://");

      if (isExternal) {
        if (!checkExternal) continue;

        let result = externalCache.get(href);
        if (!result) {
          result = await checkExternalUrl(href, 3, 5000);
          externalCache.set(href, result);
        }

        if (!result.ok) {
          broken.push({
            src: relative(REPO_ROOT, src),
            link: href,
            reason: result.error ? `Error: ${result.error}` : `Status: ${result.status}`
          });
        }
        continue;
      }

      if (href.includes("://")) continue;

      const pathPart = href.split("#")[0];
      if (!pathPart.endsWith(".md")) continue;
      if (pathPart.includes("*") || pathPart.includes("?")) continue;

      let decoded;
      try {
        decoded = decodeURIComponent(pathPart);
      } catch {
        broken.push({ src: relative(REPO_ROOT, src), link: pathPart, reason: "Invalid URI" });
        continue;
      }
      const target = resolve(srcDir, decoded);
      if (allMd.has(target) || existsSync(target)) continue;
      broken.push({ src: relative(REPO_ROOT, src), link: pathPart, reason: "File not found" });
    }
  }

  const report = {
    scanAll,
    checkExternal,
    totalBroken: broken.length,
    brokenLinks: broken
  };

  if (broken.length === 0) {
    process.stdout.write(JSON.stringify(report, null, 2) + "\n");
    process.exit(0);
  }

  process.stderr.write(JSON.stringify(report, null, 2) + "\n");
  process.exit(1);
}

const isTestEnv = typeof process.env.NODE_TEST_CONTEXT !== 'undefined' || process.argv.includes('--test');
if (!isTestEnv && process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch(err => {
    process.stderr.write(`{"error": "${err.message}"}\n`);
    process.exit(1);
  });
}
