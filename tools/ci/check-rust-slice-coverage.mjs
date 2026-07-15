#!/usr/bin/env node
/**
 * SSDV3 Step 3 — RamShared slice coverage gate (per production file).
 *
 * Runs `cargo llvm-cov` on the named workspace packages, then asserts
 * **line** coverage ≥ `--min` (default 80) for each path in `--files`.
 * Workspace / package average does **not** pass the gate.
 *
 * Usage:
 *   node tools/ci/check-rust-slice-coverage.mjs \
 *     --packages ramshared-broker,ramshared-cli \
 *     --files crates/ramshared-broker/src/arbiter.rs,crates/ramshared-cli/src/cascade/mod.rs \
 *     --min 80
 *
 *   node tools/ci/check-rust-slice-coverage.mjs \
 *     -p ramshared-broker \
 *     --files-from /tmp/slice-files.txt \
 *     --min 80
 *
 * Options:
 *   --packages / -p   Comma-separated cargo package names (workspace members). Required unless --report-only.
 *   --files           Comma-separated production .rs paths (repo-root relative).
 *   --files-from      Text file: one path per line (# comments / blank skipped).
 *   --min             Minimum line coverage percent (default 80).
 *   --report-json     Write raw llvm-cov JSON export to this path.
 *   --report-only PATH
 *                     Skip tests; gate against an existing llvm-cov JSON export
 *                     (must include per-file summaries, e.g. from a prior --report-json).
 *   --allow-missing   If a --files path is absent from the profile, treat as note (still FAIL
 *                     unless the path also does not exist on disk → always FAIL).
 *   --metric lines|regions|functions   Default: lines.
 *
 * Exit: 0 pass · 1 gate fail · 2 usage / tool error.
 *
 * See docs/SSDV3-PROMPTS.md § Cover vs E2E · .claude/rules/ssdv3.md.
 */

import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..", "..");

function usage(exit = 2) {
  console.error(`Usage:
  node tools/ci/check-rust-slice-coverage.mjs -p <pkg[,pkg...]> --files <path[,path...]> [--min 80]
  node tools/ci/check-rust-slice-coverage.mjs -p <pkg> --files-from <list.txt> [--min 80]
  node tools/ci/check-rust-slice-coverage.mjs --report-only <export.json> --files <...> [--min 80]`);
  process.exit(exit);
}

function parseArgs(argv) {
  const out = {
    packages: [],
    files: [],
    filesFrom: "",
    min: 80,
    reportJson: "",
    reportOnly: "",
    allowMissing: false,
    metric: "lines",
    help: false,
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      const v = argv[++i];
      if (v === undefined) {
        console.error(`missing value after ${a}`);
        usage(2);
      }
      return v;
    };
    if (a === "--help" || a === "-h") out.help = true;
    else if (a === "--packages" || a === "-p") {
      out.packages = next()
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
    } else if (a === "--files") {
      out.files.push(
        ...next()
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      );
    } else if (a === "--files-from") out.filesFrom = next();
    else if (a === "--min") out.min = Number(next());
    else if (a === "--report-json") out.reportJson = next();
    else if (a === "--report-only") out.reportOnly = next();
    else if (a === "--allow-missing") out.allowMissing = true;
    else if (a === "--metric") out.metric = next();
    else {
      console.error(`unknown arg: ${a}`);
      usage(2);
    }
  }
  return out;
}

function loadFilesFrom(path) {
  const abs = resolve(REPO_ROOT, path);
  if (!existsSync(abs)) {
    console.error(`--files-from not found: ${path}`);
    process.exit(2);
  }
  return readFileSync(abs, "utf8")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.replace(/^\.\//, ""));
}

function normRepoPath(p) {
  let s = p.replaceAll("\\", "/").replace(/^\.\//, "");
  if (s.startsWith(REPO_ROOT.replaceAll("\\", "/") + "/")) {
    s = s.slice(REPO_ROOT.replaceAll("\\", "/").length + 1);
  }
  // llvm-cov sometimes embeds absolute paths
  const abs = resolve(s);
  if (abs.startsWith(REPO_ROOT + "/") || abs.startsWith(REPO_ROOT + "\\")) {
    s = relative(REPO_ROOT, abs).replaceAll("\\", "/");
  }
  return s;
}

/**
 * @returns {Map<string, { percent: number, covered: number, count: number }>}
 */
function parseLlvmCovJson(content, metric) {
  let data;
  try {
    data = JSON.parse(content);
  } catch (e) {
    console.error("failed to parse llvm-cov JSON:", e.message);
    process.exit(2);
  }
  const files = data?.data?.[0]?.files;
  if (!Array.isArray(files)) {
    console.error("llvm-cov JSON missing data[0].files (use export with per-file summary)");
    process.exit(2);
  }
  const map = new Map();
  for (const f of files) {
    const rawName = f.filename || f.name || "";
    if (!rawName) continue;
    const key = normRepoPath(rawName);
    // skip tests and non-src noise
    if (key.endsWith("_test.rs") || key.includes("/tests/")) continue;
    const summary = f.summary?.[metric];
    if (!summary || typeof summary.count !== "number") continue;
    const count = summary.count;
    const covered = summary.covered ?? 0;
    const percent =
      typeof summary.percent === "number"
        ? summary.percent
        : count === 0
          ? 100
          : (100 * covered) / count;
    // if same path appears twice, merge by weighted lines
    const prev = map.get(key);
    if (prev) {
      const c = prev.count + count;
      const cov = prev.covered + covered;
      map.set(key, {
        count: c,
        covered: cov,
        percent: c === 0 ? 100 : (100 * cov) / c,
      });
    } else {
      map.set(key, { count, covered, percent });
    }
  }
  return map;
}

function runLlvmCov(packages, jsonOutPath) {
  if (!existsSync(join(REPO_ROOT, "Cargo.toml"))) {
    console.error(`Cargo.toml not found at repo root: ${REPO_ROOT}`);
    process.exit(2);
  }
  const args = ["llvm-cov"];
  for (const p of packages) {
    args.push("-p", p);
  }
  args.push("--json", "--summary-only", "--output-path", jsonOutPath);

  console.error(`$ cargo ${args.join(" ")}`);
  const res = spawnSync("cargo", args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (res.stdout) process.stderr.write(res.stdout);
  if (res.stderr) process.stderr.write(res.stderr);
  if (res.status !== 0) {
    console.error(`cargo llvm-cov failed (exit ${res.status ?? 1})`);
    process.exit(res.status ?? 1);
  }
  if (!existsSync(jsonOutPath)) {
    console.error(`llvm-cov did not write ${jsonOutPath}`);
    process.exit(2);
  }
}

function main() {
  const opts = parseArgs(process.argv);
  if (opts.help) usage(0);

  if (!Number.isFinite(opts.min) || opts.min <= 0 || opts.min > 100) {
    console.error("invalid --min (expected (0, 100])");
    process.exit(2);
  }
  if (!["lines", "regions", "functions"].includes(opts.metric)) {
    console.error("--metric must be lines|regions|functions");
    process.exit(2);
  }

  let files = opts.files.map(normRepoPath);
  if (opts.filesFrom) {
    files.push(...loadFilesFrom(opts.filesFrom).map(normRepoPath));
  }
  // unique preserve order
  files = [...new Set(files)];
  if (files.length === 0) {
    console.error("provide --files and/or --files-from (production paths to gate)");
    usage(2);
  }

  let jsonPath;
  let tmpDir = null;
  try {
    if (opts.reportOnly) {
      jsonPath = resolve(REPO_ROOT, opts.reportOnly);
      if (!existsSync(jsonPath)) {
        console.error(`--report-only not found: ${opts.reportOnly}`);
        process.exit(2);
      }
    } else {
      if (opts.packages.length === 0) {
        console.error("--packages / -p required unless --report-only");
        usage(2);
      }
      tmpDir = mkdtempSync(join(tmpdir(), "rs-slice-cov-"));
      jsonPath = join(tmpDir, "llvm-cov.json");
      runLlvmCov(opts.packages, jsonPath);
      if (opts.reportJson) {
        const dest = resolve(REPO_ROOT, opts.reportJson);
        mkdirSync(dirname(dest), { recursive: true });
        writeFileSync(dest, readFileSync(jsonPath));
        console.error(`wrote report JSON: ${opts.reportJson}`);
      }
    }

    const stats = parseLlvmCovJson(readFileSync(jsonPath, "utf8"), opts.metric);
    const rows = [];
    const violations = [];

    for (const file of files) {
      const hit = stats.get(file);
      const onDisk = existsSync(join(REPO_ROOT, file));
      if (!onDisk) {
        violations.push({
          file,
          percent: 0,
          reason: "file not found in repo",
        });
        rows.push({
          file,
          percent: 0,
          covered: 0,
          count: 0,
          mark: "FAIL",
          note: "missing on disk",
        });
        continue;
      }
      if (!hit) {
        const msg = "absent from coverage profile (not instrumented or wrong -p)";
        if (opts.allowMissing) {
          rows.push({
            file,
            percent: 0,
            covered: 0,
            count: 0,
            mark: "skip",
            note: msg,
          });
        } else {
          violations.push({ file, percent: 0, reason: msg });
          rows.push({
            file,
            percent: 0,
            covered: 0,
            count: 0,
            mark: "FAIL",
            note: msg,
          });
        }
        continue;
      }
      if (hit.count === 0) {
        // no executable lines — treat as N/A pass
        rows.push({
          file,
          percent: 100,
          covered: 0,
          count: 0,
          mark: "ok  ",
          note: "0 instrumented lines",
        });
        continue;
      }
      const fail = hit.percent + 1e-9 < opts.min;
      rows.push({
        file,
        percent: hit.percent,
        covered: hit.covered,
        count: hit.count,
        mark: fail ? "FAIL" : "ok  ",
        note: "",
      });
      if (fail) {
        violations.push({
          file,
          percent: hit.percent,
          reason: `below ${opts.min}% (${hit.covered}/${hit.count} ${opts.metric})`,
        });
      }
    }

    console.log(
      `Rust slice coverage gate (metric=${opts.metric}, min ${opts.min}%)` +
        (opts.packages.length ? ` — packages: ${opts.packages.join(",")}` : " — report-only"),
    );
    for (const r of rows.sort((a, b) => a.file.localeCompare(b.file))) {
      console.log(
        `  [${r.mark}] ${r.percent.toFixed(1).padStart(6)}%  ${String(r.covered).padStart(4)}/${String(r.count).padStart(4)}  ${r.file}${r.note ? "  (" + r.note + ")" : ""}`,
      );
    }

    if (violations.length > 0) {
      console.error("\nCoverage gate FAILED:");
      for (const v of violations) {
        const pct =
          typeof v.percent === "number" ? `${v.percent.toFixed(1)}% ` : "";
        console.error(`  - ${v.file}: ${pct}${v.reason}`);
      }
      console.error(
        "\nSSDV3 Step 3: business-logic files in the SPEC matrix must be ≥ min% (workspace average does not count).",
      );
      process.exit(1);
    }
    console.log("\nCoverage gate PASSED.");
  } finally {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
  }
}

main();
