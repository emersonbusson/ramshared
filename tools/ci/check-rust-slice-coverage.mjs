#!/usr/bin/env node
/**
 * SSDV3 Step 3 — RamShared slice coverage gate (per production file).
 *
 * Runs `cargo llvm-cov` on the named workspace packages, then asserts
 * **line** coverage ≥ `--min` (default 80) for each path in `--files`.
 * Workspace / package average does **not** pass the gate.
 *
 * Every Cargo invocation owns a temporary target/profile/report directory and
 * takes a bounded fail-closed lock. This prevents two legitimate local gates
 * from sharing cargo-llvm-cov's default `target/llvm-cov-target` state.
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
import { createHash, randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..", "..");
const LOCK_SCHEMA_VERSION = 1;
const RUN_SCHEMA_VERSION = 1;
const COVERAGE_LOCK_WAIT_MS = 60_000;
const COVERAGE_LOCK_POLL_MS = 100;
const COVERAGE_LOCK_LEASE_MS = 45 * 60_000;
const COVERAGE_CHILD_TIMEOUT_MS = 15 * 60_000;
const COVERAGE_CHILD_KILL_GRACE_MS = 5_000;
const COVERAGE_SUPERVISOR_TIMEOUT_MS = COVERAGE_CHILD_TIMEOUT_MS + 10_000;

class CoverageGateError extends Error {
  constructor(code, message, exitCode = 2) {
    super(message);
    this.code = code;
    this.exitCode = exitCode;
  }
}

function usageText() {
  return `Usage:
  node tools/ci/check-rust-slice-coverage.mjs -p <pkg[,pkg...]> --files <path[,path...]> [--min 80]
  node tools/ci/check-rust-slice-coverage.mjs -p <pkg> --files-from <list.txt> [--min 80]
  node tools/ci/check-rust-slice-coverage.mjs --report-only <export.json> --files <...> [--min 80]`;
}

function usageError(message) {
  return new CoverageGateError("COVERAGE_USAGE_ERROR", message, 2);
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
    const argument = argv[i];
    const next = () => {
      const value = argv[++i];
      if (value === undefined) throw usageError(`missing value after ${argument}`);
      return value;
    };
    if (argument === "--help" || argument === "-h") out.help = true;
    else if (argument === "--packages" || argument === "-p") {
      out.packages = next()
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean);
    } else if (argument === "--files") {
      out.files.push(
        ...next()
          .split(",")
          .map((value) => value.trim())
          .filter(Boolean),
      );
    } else if (argument === "--files-from") out.filesFrom = next();
    else if (argument === "--min") out.min = Number(next());
    else if (argument === "--report-json") out.reportJson = next();
    else if (argument === "--report-only") out.reportOnly = next();
    else if (argument === "--allow-missing") out.allowMissing = true;
    else if (argument === "--metric") out.metric = next();
    else throw usageError(`unknown arg: ${argument}`);
  }
  return out;
}

function loadFilesFrom(path, repoRoot = REPO_ROOT) {
  const absolutePath = resolve(repoRoot, path);
  if (!existsSync(absolutePath)) {
    throw usageError(`--files-from not found: ${path}`);
  }
  return readFileSync(absolutePath, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => line.replace(/^\.\//, ""));
}

function normRepoPath(path, repoRoot = REPO_ROOT) {
  let normalized = path.replaceAll("\\", "/").replace(/^\.\//, "");
  const normalizedRoot = repoRoot.replaceAll("\\", "/");
  if (normalized.startsWith(`${normalizedRoot}/`)) {
    normalized = normalized.slice(normalizedRoot.length + 1);
  }
  const absolutePath = resolve(normalized);
  if (absolutePath.startsWith(`${repoRoot}/`) || absolutePath.startsWith(`${repoRoot}\\`)) {
    normalized = relative(repoRoot, absolutePath).replaceAll("\\", "/");
  }
  return normalized;
}

/**
 * @returns {Map<string, { percent: number, covered: number, count: number }>}
 */
function parseLlvmCovJson(content, metric, repoRoot = REPO_ROOT) {
  let data;
  try {
    data = JSON.parse(content);
  } catch (error) {
    throw new CoverageGateError("COVERAGE_REPORT_INVALID", "failed to parse llvm-cov JSON", 2);
  }
  const files = data?.data?.[0]?.files;
  if (!Array.isArray(files)) {
    throw new CoverageGateError(
      "COVERAGE_REPORT_INVALID",
      "llvm-cov JSON missing data[0].files (use export with per-file summary)",
      2,
    );
  }
  const map = new Map();
  for (const file of files) {
    const rawName = file.filename || file.name || "";
    if (!rawName) continue;
    const key = normRepoPath(rawName, repoRoot);
    if (key.endsWith("_test.rs") || key.includes("/tests/")) continue;
    const summary = file.summary?.[metric];
    if (!summary || typeof summary.count !== "number") continue;
    const count = summary.count;
    const covered = summary.covered ?? 0;
    const percent =
      typeof summary.percent === "number"
        ? summary.percent
        : count === 0
          ? 100
          : (100 * covered) / count;
    const previous = map.get(key);
    if (previous) {
      const mergedCount = previous.count + count;
      const mergedCovered = previous.covered + covered;
      map.set(key, {
        count: mergedCount,
        covered: mergedCovered,
        percent: mergedCount === 0 ? 100 : (100 * mergedCovered) / mergedCount,
      });
    } else {
      map.set(key, { count, covered, percent });
    }
  }
  return map;
}

function createLockOwner({
  runId = randomUUID(),
  pid = process.pid,
  now = Date.now(),
  leaseMs = COVERAGE_LOCK_LEASE_MS,
} = {}) {
  if (!Number.isSafeInteger(pid) || pid <= 0) throw usageError("coverage lock PID must be positive");
  if (!Number.isSafeInteger(now) || now < 0) throw usageError("coverage lock time must be non-negative");
  if (!Number.isSafeInteger(leaseMs) || leaseMs <= 0) {
    throw usageError("coverage lock lease must be positive");
  }
  if (typeof runId !== "string" || runId.length < 1 || runId.length > 128) {
    throw usageError("coverage lock run ID is invalid");
  }
  return {
    schema_version: LOCK_SCHEMA_VERSION,
    run_id: runId,
    pid,
    started_at_ms: now,
    lease_expires_at_ms: now + leaseMs,
  };
}

function validLockOwner(owner) {
  return (
    owner &&
    typeof owner === "object" &&
    owner.schema_version === LOCK_SCHEMA_VERSION &&
    typeof owner.run_id === "string" &&
    owner.run_id.length >= 1 &&
    owner.run_id.length <= 128 &&
    Number.isSafeInteger(owner.pid) &&
    owner.pid > 0 &&
    Number.isSafeInteger(owner.started_at_ms) &&
    owner.started_at_ms >= 0 &&
    Number.isSafeInteger(owner.lease_expires_at_ms) &&
    owner.lease_expires_at_ms >= owner.started_at_ms
  );
}

function sameLockOwner(left, right) {
  return (
    left?.schema_version === right?.schema_version &&
    left?.run_id === right?.run_id &&
    left?.pid === right?.pid &&
    left?.started_at_ms === right?.started_at_ms &&
    left?.lease_expires_at_ms === right?.lease_expires_at_ms
  );
}

function readCoverageLockOwner(lockDir) {
  let content;
  try {
    content = readFileSync(join(lockDir, "owner.json"), "utf8");
  } catch (error) {
    if (error?.code === "ENOENT" && !existsSync(lockDir)) return null;
    throw new CoverageGateError(
      "COVERAGE_LOCK_OWNER_CORRUPT",
      "coverage lock owner record is missing or unreadable",
      2,
    );
  }
  let owner;
  try {
    owner = JSON.parse(content);
  } catch {
    throw new CoverageGateError("COVERAGE_LOCK_OWNER_CORRUPT", "coverage lock owner record is invalid", 2);
  }
  if (!validLockOwner(owner)) {
    throw new CoverageGateError("COVERAGE_LOCK_OWNER_CORRUPT", "coverage lock owner record is invalid", 2);
  }
  return owner;
}

function defaultIsPidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code !== "ESRCH";
  }
}

const SLEEP_CELL = new Int32Array(new SharedArrayBuffer(4));

function defaultSleep(milliseconds) {
  Atomics.wait(SLEEP_CELL, 0, 0, milliseconds);
}

function assertLockTiming(waitMs, pollMs) {
  if (!Number.isSafeInteger(waitMs) || waitMs < 0 || waitMs > COVERAGE_LOCK_WAIT_MS) {
    throw usageError(`coverage lock wait must be between 0 and ${COVERAGE_LOCK_WAIT_MS} ms`);
  }
  if (!Number.isSafeInteger(pollMs) || pollMs <= 0 || pollMs > COVERAGE_LOCK_WAIT_MS) {
    throw usageError("coverage lock poll must be positive and bounded");
  }
}

function acquireCoverageLock({
  lockDir,
  owner,
  waitMs = COVERAGE_LOCK_WAIT_MS,
  pollMs = COVERAGE_LOCK_POLL_MS,
  now = Date.now,
  isPidAlive = defaultIsPidAlive,
  sleep = defaultSleep,
} = {}) {
  if (typeof lockDir !== "string" || lockDir.length === 0 || !validLockOwner(owner)) {
    throw usageError("coverage lock input is invalid");
  }
  assertLockTiming(waitMs, pollMs);
  const deadline = now() + waitMs;
  mkdirSync(dirname(lockDir), { recursive: true, mode: 0o700 });

  while (true) {
    try {
      mkdirSync(lockDir, { mode: 0o700 });
      try {
        writeFileSync(join(lockDir, "owner.json"), `${JSON.stringify(owner)}\n`, {
          encoding: "utf8",
          flag: "wx",
          mode: 0o600,
        });
      } catch (error) {
        rmSync(lockDir, { recursive: true, force: true });
        throw new CoverageGateError(
          "COVERAGE_LOCK_CREATE_FAILED",
          "coverage lock owner record could not be created",
          2,
        );
      }
      return { lockDir, owner };
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
    }

    const existingOwner = readCoverageLockOwner(lockDir);
    if (existingOwner === null) continue;
    const currentTime = now();
    if (existingOwner.lease_expires_at_ms < currentTime && !isPidAlive(existingOwner.pid)) {
      throw new CoverageGateError(
        "COVERAGE_LOCK_STALE_OWNER",
        "coverage lock has an expired owner; refusing automatic deletion",
        2,
      );
    }
    if (currentTime >= deadline) {
      throw new CoverageGateError(
        "COVERAGE_LOCK_TIMEOUT",
        "coverage lock remained owned until the bounded wait elapsed",
        2,
      );
    }
    sleep(Math.min(pollMs, Math.max(1, deadline - currentTime)));
  }
}

function releaseCoverageLock(lock) {
  if (!lock?.lockDir || !validLockOwner(lock.owner) || !existsSync(lock.lockDir)) return false;
  let existingOwner;
  try {
    existingOwner = readCoverageLockOwner(lock.lockDir);
  } catch {
    return false;
  }
  if (!sameLockOwner(existingOwner, lock.owner)) return false;
  rmSync(lock.lockDir, { recursive: true, force: true });
  return true;
}

function createCoverageRun({
  temporaryRoot = tmpdir(),
  runId = randomUUID(),
  pid = process.pid,
  now = Date.now(),
} = {}) {
  if (typeof temporaryRoot !== "string" || temporaryRoot.length === 0) {
    throw usageError("coverage temporary root is invalid");
  }
  if (!Number.isSafeInteger(pid) || pid <= 0 || !Number.isSafeInteger(now) || now < 0) {
    throw usageError("coverage run owner is invalid");
  }
  if (typeof runId !== "string" || runId.length < 1 || runId.length > 128) {
    throw usageError("coverage run ID is invalid");
  }
  const normalizedTemporaryRoot = resolve(temporaryRoot);
  const root = mkdtempSync(join(normalizedTemporaryRoot, "rs-slice-cov-"));
  const owner = {
    schema_version: RUN_SCHEMA_VERSION,
    run_id: runId,
    pid,
    created_at_ms: now,
  };
  writeFileSync(join(root, "owner.json"), `${JSON.stringify(owner)}\n`, {
    encoding: "utf8",
    flag: "wx",
    mode: 0o600,
  });
  const cargoTargetDir = join(root, "cargo-target");
  mkdirSync(cargoTargetDir, { recursive: true, mode: 0o700 });
  return {
    temporaryRoot: normalizedTemporaryRoot,
    root,
    runId,
    owner,
    cargoTargetDir,
    jsonPath: join(root, "llvm-cov.json"),
  };
}

function isOwnedRunPath(run) {
  if (!run?.temporaryRoot || !run?.root) return false;
  const pathFromTemporaryRoot = relative(resolve(run.temporaryRoot), resolve(run.root));
  return pathFromTemporaryRoot !== "" && !pathFromTemporaryRoot.startsWith("..") && !isAbsolute(pathFromTemporaryRoot);
}

function cleanupCoverageRun(run) {
  if (!isOwnedRunPath(run) || !existsSync(run.root)) return false;
  let owner;
  try {
    owner = JSON.parse(readFileSync(join(run.root, "owner.json"), "utf8"));
  } catch {
    return false;
  }
  if (
    owner?.schema_version !== RUN_SCHEMA_VERSION ||
    owner?.run_id !== run.runId ||
    owner?.pid !== run.owner?.pid ||
    owner?.created_at_ms !== run.owner?.created_at_ms
  ) {
    return false;
  }
  rmSync(run.root, { recursive: true, force: true });
  return true;
}

function coverageLockDir({ repoRoot = REPO_ROOT, temporaryRoot = tmpdir() } = {}) {
  const repoFingerprint = createHash("sha256").update(resolve(repoRoot)).digest("hex").slice(0, 32);
  return join(resolve(temporaryRoot), "ramshared-rust-slice-coverage-lock-v1", repoFingerprint);
}

function cleanupCoverageIsolation(scope) {
  return {
    lockReleased: scope?.lock ? releaseCoverageLock(scope.lock) : false,
    runRemoved: scope?.run ? cleanupCoverageRun(scope.run) : false,
  };
}

function defaultTerminateForSignal(signal) {
  process.exit(signal === "SIGINT" ? 130 : 143);
}

function installCoverageSignalCleanup({
  scope,
  on = (signal, handler) => process.once(signal, handler),
  off = (signal, handler) => process.off(signal, handler),
  terminate = defaultTerminateForSignal,
} = {}) {
  let handled = false;
  const handler = (signal) => {
    if (handled) return;
    handled = true;
    if (scope) scope.interrupted = true;
    cleanupCoverageIsolation(scope);
    terminate(signal);
  };
  const onSigint = () => handler("SIGINT");
  const onSigterm = () => handler("SIGTERM");
  on("SIGINT", onSigint);
  on("SIGTERM", onSigterm);
  return () => {
    off("SIGINT", onSigint);
    off("SIGTERM", onSigterm);
  };
}

function runWithCoverageIsolation({
  repoRoot = REPO_ROOT,
  temporaryRoot = tmpdir(),
  lockDir = coverageLockDir({ repoRoot, temporaryRoot }),
  runId = randomUUID(),
  now = Date.now,
  pid = process.pid,
  waitMs = COVERAGE_LOCK_WAIT_MS,
  pollMs = COVERAGE_LOCK_POLL_MS,
  isPidAlive = defaultIsPidAlive,
  sleep = defaultSleep,
  installSignalHandlers = true,
  signalOptions = {},
  execute,
} = {}) {
  if (typeof execute !== "function") throw usageError("coverage isolation execute callback is required");
  const startTime = now();
  const run = createCoverageRun({ temporaryRoot, runId, pid, now: startTime });
  const scope = { run, lock: undefined, interrupted: false };
  let lock;
  let removeSignalHandlers = () => {};
  try {
    if (installSignalHandlers) {
      removeSignalHandlers = installCoverageSignalCleanup({ scope, ...signalOptions });
    }
    lock = acquireCoverageLock({
      lockDir,
      owner: createLockOwner({ runId: run.runId, pid, now: startTime }),
      waitMs,
      pollMs,
      now,
      isPidAlive,
      sleep,
    });
    scope.lock = lock;
    if (scope.interrupted) {
      throw new CoverageGateError("COVERAGE_SIGNAL_INTERRUPTED", "coverage run interrupted before Cargo started", 2);
    }
    return execute(run);
  } finally {
    removeSignalHandlers();
    cleanupCoverageIsolation({ run, lock });
  }
}

function runLlvmCov(
  packages,
  jsonOutPath,
  cargoTargetDir,
  { repoRoot = REPO_ROOT, env = process.env, spawnCommand = spawnSync, error = console.error } = {},
) {
  if (!existsSync(join(repoRoot, "Cargo.toml"))) {
    throw new CoverageGateError("COVERAGE_TOOL_ROOT_INVALID", "Cargo.toml not found at repository root", 2);
  }
  const cargoArgs = ["llvm-cov"];
  for (const packageName of packages) cargoArgs.push("-p", packageName);
  cargoArgs.push("--json", "--summary-only", "--output-path", jsonOutPath);
  cargoArgs.push("--", "--test-threads=1");

  const renderedArgs = cargoArgs.map((argument) =>
    argument === jsonOutPath ? "<private-run>/llvm-cov.json" : argument,
  );
  error(`$ cargo ${renderedArgs.join(" ")}`);
  const supervisorArgs = [
    "--signal=TERM",
    `--kill-after=${COVERAGE_CHILD_KILL_GRACE_MS / 1_000}s`,
    `${COVERAGE_CHILD_TIMEOUT_MS / 60_000}m`,
    "cargo",
    ...cargoArgs,
  ];
  const result = spawnCommand("timeout", supervisorArgs, {
    cwd: repoRoot,
    env: { ...env, CARGO_TARGET_DIR: cargoTargetDir },
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    shell: false,
    timeout: COVERAGE_SUPERVISOR_TIMEOUT_MS,
    killSignal: "SIGTERM",
  });
  if (result.stdout) process.stderr.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.status === 124 || result.error?.code === "ETIMEDOUT") {
    throw new CoverageGateError(
      "COVERAGE_CHILD_TIMEOUT",
      "cargo llvm-cov exceeded the 15-minute deadline",
      2,
    );
  }
  if (result.error) {
    throw new CoverageGateError("COVERAGE_CHILD_START_FAILED", "cargo llvm-cov could not start", 2);
  }
  if (result.status !== 0) {
    throw new CoverageGateError(
      "COVERAGE_CHILD_FAILED",
      `cargo llvm-cov failed (exit ${result.status ?? 1})`,
      result.status ?? 1,
    );
  }
  if (!existsSync(jsonOutPath)) {
    throw new CoverageGateError("COVERAGE_REPORT_MISSING", "cargo llvm-cov did not write its JSON report", 2);
  }
}

function evaluateCoverage({ files, stats, min, allowMissing, metric, repoRoot = REPO_ROOT }) {
  const rows = [];
  const violations = [];
  for (const file of files) {
    const hit = stats.get(file);
    const onDisk = existsSync(join(repoRoot, file));
    if (!onDisk) {
      violations.push({ file, percent: 0, reason: "file not found in repo" });
      rows.push({ file, percent: 0, covered: 0, count: 0, mark: "FAIL", note: "missing on disk" });
      continue;
    }
    if (!hit) {
      const message = "absent from coverage profile (not instrumented or wrong -p)";
      if (allowMissing) {
        rows.push({ file, percent: 0, covered: 0, count: 0, mark: "skip", note: message });
      } else {
        violations.push({ file, percent: 0, reason: message });
        rows.push({ file, percent: 0, covered: 0, count: 0, mark: "FAIL", note: message });
      }
      continue;
    }
    if (hit.count === 0) {
      rows.push({ file, percent: 100, covered: 0, count: 0, mark: "ok  ", note: "0 instrumented lines" });
      continue;
    }
    const failed = hit.percent + 1e-9 < min;
    rows.push({
      file,
      percent: hit.percent,
      covered: hit.covered,
      count: hit.count,
      mark: failed ? "FAIL" : "ok  ",
      note: "",
    });
    if (failed) {
      violations.push({
        file,
        percent: hit.percent,
        reason: `below ${min}% (${hit.covered}/${hit.count} ${metric})`,
      });
    }
  }
  return { rows, violations };
}

function reportError(error) {
  if (error instanceof CoverageGateError) return `${error.code}: ${error.message}`;
  return `COVERAGE_TOOL_ERROR: ${error?.message ?? "unexpected error"}`;
}

function main(argv = process.argv, { print = console.log, error = console.error } = {}) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      print(usageText());
      return 0;
    }
    if (!Number.isFinite(options.min) || options.min <= 0 || options.min > 100) {
      throw usageError("invalid --min (expected (0, 100])");
    }
    if (!["lines", "regions", "functions"].includes(options.metric)) {
      throw usageError("--metric must be lines|regions|functions");
    }

    let files = options.files.map((file) => normRepoPath(file));
    if (options.filesFrom) files.push(...loadFilesFrom(options.filesFrom).map((file) => normRepoPath(file)));
    files = [...new Set(files)];
    if (files.length === 0) throw usageError("provide --files and/or --files-from (production paths to gate)");

    let coverageContent;
    if (options.reportOnly) {
      const reportPath = resolve(REPO_ROOT, options.reportOnly);
      if (!existsSync(reportPath)) throw usageError(`--report-only not found: ${options.reportOnly}`);
      coverageContent = readFileSync(reportPath, "utf8");
    } else {
      if (options.packages.length === 0) throw usageError("--packages / -p required unless --report-only");
      coverageContent = runWithCoverageIsolation({
        execute: (run) => {
          runLlvmCov(options.packages, run.jsonPath, run.cargoTargetDir, { error });
          if (options.reportJson) {
            const destination = resolve(REPO_ROOT, options.reportJson);
            mkdirSync(dirname(destination), { recursive: true });
            writeFileSync(destination, readFileSync(run.jsonPath));
            error(`wrote report JSON: ${options.reportJson}`);
          }
          return readFileSync(run.jsonPath, "utf8");
        },
      });
    }

    const stats = parseLlvmCovJson(coverageContent, options.metric);
    const { rows, violations } = evaluateCoverage({
      files,
      stats,
      min: options.min,
      allowMissing: options.allowMissing,
      metric: options.metric,
    });
    print(
      `Rust slice coverage gate (metric=${options.metric}, min ${options.min}%)` +
        (options.packages.length ? ` — packages: ${options.packages.join(",")}` : " — report-only"),
    );
    for (const row of rows.sort((left, right) => left.file.localeCompare(right.file))) {
      print(
        `  [${row.mark}] ${row.percent.toFixed(1).padStart(6)}%  ${String(row.covered).padStart(4)}/${String(row.count).padStart(4)}  ${row.file}${row.note ? `  (${row.note})` : ""}`,
      );
    }
    if (violations.length > 0) {
      error("\nCoverage gate FAILED:");
      for (const violation of violations) {
        const percent = typeof violation.percent === "number" ? `${violation.percent.toFixed(1)}% ` : "";
        error(`  - ${violation.file}: ${percent}${violation.reason}`);
      }
      error(
        "\nSSDV3 Step 3: business-logic files in the SPEC matrix must be ≥ min% (workspace average does not count).",
      );
      return 1;
    }
    print("\nCoverage gate PASSED.");
    return 0;
  } catch (caught) {
    error(reportError(caught));
    return caught instanceof CoverageGateError ? caught.exitCode : 2;
  }
}

export {
  COVERAGE_CHILD_KILL_GRACE_MS,
  COVERAGE_CHILD_TIMEOUT_MS,
  COVERAGE_SUPERVISOR_TIMEOUT_MS,
  COVERAGE_LOCK_LEASE_MS,
  COVERAGE_LOCK_POLL_MS,
  COVERAGE_LOCK_WAIT_MS,
  CoverageGateError,
  acquireCoverageLock,
  cleanupCoverageIsolation,
  cleanupCoverageRun,
  coverageLockDir,
  createCoverageRun,
  createLockOwner,
  evaluateCoverage,
  installCoverageSignalCleanup,
  loadFilesFrom,
  main,
  normRepoPath,
  parseArgs,
  parseLlvmCovJson,
  releaseCoverageLock,
  runLlvmCov,
  runWithCoverageIsolation,
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
