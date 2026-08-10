import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import * as coverageChecker from "./check-rust-slice-coverage.mjs";

const TOOL_PATH = resolve(fileURLToPath(new URL("./check-rust-slice-coverage.mjs", import.meta.url)));
const REPO_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const COVERED_FILE = "crates/ramshared-cli/src/cascade/cascade_io.rs";

function runChecker(env) {
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn(
      process.execPath,
      [TOOL_PATH, "-p", "ramshared-cli", "--files", COVERED_FILE, "--min", "80"],
      { cwd: REPO_ROOT, env, stdio: ["ignore", "pipe", "pipe"] },
    );
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", rejectRun);
    child.on("close", (code, signal) => resolveRun({ code, signal, stdout, stderr }));
  });
}

async function waitFor(path) {
  const deadline = Date.now() + 2_000;
  while (!existsSync(path)) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${path}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 10));
  }
}

function writeFakeCargo(binDir) {
  const fakeCargo = join(binDir, "cargo");
  writeFileSync(
    fakeCargo,
    `#!${process.execPath}
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const outputIndex = args.indexOf("--output-path");
const outputPath = args[outputIndex + 1];
const targetDir = process.env.CARGO_TARGET_DIR || join(process.env.RS_FAKE_COV_STATE, "shared-target");
const active = join(targetDir, "active");
mkdirSync(targetDir, { recursive: true });
try {
  writeFileSync(join(process.env.RS_FAKE_COV_STATE, "first-target"), targetDir, { flag: "wx" });
} catch (error) {
  if (error.code !== "EEXIST") throw error;
}
if (existsSync(active)) {
  process.stderr.write("simulated shared llvm-cov target collision\\n");
  process.exit(73);
}
writeFileSync(active, String(process.pid));
setTimeout(() => {
  writeFileSync(outputPath, JSON.stringify({ data: [{ files: [{ filename: ${JSON.stringify(join(REPO_ROOT, COVERED_FILE))}, summary: { lines: { count: 1, covered: 1, percent: 100 } } }] }] }));
  rmSync(active, { force: true });
}, 250);
`,
  );
  chmodSync(fakeCargo, 0o755);
}

function checkerApi(name) {
  assert.equal(typeof coverageChecker[name], "function", `${name} must be exported for checker tests`);
  return coverageChecker[name];
}

function writeLockOwner(lockDir, owner) {
  mkdirSync(lockDir, { recursive: true });
  writeFileSync(join(lockDir, "owner.json"), `${JSON.stringify(owner)}\n`);
}

test("overlapping_checker_invocations_isolate_llvm_cov_target_state", async () => {
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-overlap-"));
  try {
    const binDir = join(root, "bin");
    const stateDir = join(root, "state");
    mkdirSync(binDir);
    mkdirSync(stateDir);
    writeFakeCargo(binDir);
    const env = {
      ...process.env,
      PATH: `${binDir}${delimiter}${process.env.PATH ?? ""}`,
      RS_FAKE_COV_STATE: stateDir,
      TMPDIR: root,
      TEMP: root,
      TMP: root,
    };

    const first = runChecker(env);
    const firstTargetPath = join(stateDir, "first-target");
    await waitFor(firstTargetPath);
    const firstTarget = readFileSync(firstTargetPath, "utf8");
    await waitFor(join(firstTarget, "active"));
    const second = runChecker(env);
    const [firstResult, secondResult] = await Promise.all([first, second]);

    assert.equal(firstResult.code, 0, firstResult.stderr);
    assert.equal(secondResult.code, 0, secondResult.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_lock_wait_is_bounded_and_live_owner_is_preserved", () => {
  const acquireCoverageLock = checkerApi("acquireCoverageLock");
  const releaseCoverageLock = checkerApi("releaseCoverageLock");
  const createLockOwner = checkerApi("createLockOwner");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-lock-live-"));
  try {
    const lockDir = join(root, "coverage.lock");
    let now = 1_000;
    writeLockOwner(lockDir, createLockOwner({ runId: "live-owner", pid: 42, now, leaseMs: 1_000 }));
    let waits = 0;
    const lease = acquireCoverageLock({
      lockDir,
      owner: createLockOwner({ runId: "waiting-owner", pid: 43, now, leaseMs: 1_000 }),
      waitMs: 30,
      pollMs: 10,
      now: () => now,
      isPidAlive: () => true,
      sleep: (milliseconds) => {
        waits += 1;
        now += milliseconds;
        if (waits === 1) rmSync(lockDir, { recursive: true, force: true });
      },
    });
    assert.equal(waits, 1);
    assert.equal(lease.owner.run_id, "waiting-owner");
    assert.equal(releaseCoverageLock(lease), true);

    now = 2_000;
    const retained = createLockOwner({ runId: "still-live", pid: 44, now, leaseMs: 1_000 });
    writeLockOwner(lockDir, retained);
    assert.throws(
      () =>
        acquireCoverageLock({
          lockDir,
          owner: createLockOwner({ runId: "timed-out-owner", pid: 45, now, leaseMs: 1_000 }),
          waitMs: 20,
          pollMs: 10,
          now: () => now,
          isPidAlive: () => true,
          sleep: (milliseconds) => {
            now += milliseconds;
          },
        }),
      (error) => error?.code === "COVERAGE_LOCK_TIMEOUT",
    );
    assert.deepEqual(JSON.parse(readFileSync(join(lockDir, "owner.json"), "utf8")), retained);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_lock_detects_stale_and_corrupt_owner_fail_closed", () => {
  const acquireCoverageLock = checkerApi("acquireCoverageLock");
  const createLockOwner = checkerApi("createLockOwner");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-lock-invalid-"));
  try {
    const lockDir = join(root, "coverage.lock");
    const staleOwner = createLockOwner({ runId: "stale-owner", pid: 77, now: 1_000, leaseMs: 1 });
    writeLockOwner(lockDir, staleOwner);
    assert.throws(
      () =>
        acquireCoverageLock({
          lockDir,
          owner: createLockOwner({ runId: "new-owner", pid: 78, now: 2_000, leaseMs: 1_000 }),
          now: () => 2_000,
          isPidAlive: () => false,
        }),
      (error) => error?.code === "COVERAGE_LOCK_STALE_OWNER",
    );
    assert.deepEqual(JSON.parse(readFileSync(join(lockDir, "owner.json"), "utf8")), staleOwner);

    rmSync(lockDir, { recursive: true, force: true });
    mkdirSync(lockDir);
    writeFileSync(join(lockDir, "owner.json"), "not-json\n");
    assert.throws(
      () =>
        acquireCoverageLock({
          lockDir,
          owner: createLockOwner({ runId: "new-owner", pid: 78, now: 2_000, leaseMs: 1_000 }),
          now: () => 2_000,
          isPidAlive: () => false,
        }),
      (error) => error?.code === "COVERAGE_LOCK_OWNER_CORRUPT",
    );
    assert.equal(readFileSync(join(lockDir, "owner.json"), "utf8"), "not-json\n");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_run_cleans_owned_state_after_child_failure", () => {
  const runWithCoverageIsolation = checkerApi("runWithCoverageIsolation");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-child-failure-"));
  let runRoot = "";
  const lockDir = join(root, "coverage.lock");
  try {
    assert.throws(
      () =>
        runWithCoverageIsolation({
          temporaryRoot: root,
          lockDir,
          runId: "child-failure-owner",
          now: () => 1_000,
          installSignalHandlers: false,
          execute: (run) => {
            runRoot = run.root;
            throw Object.assign(new Error("simulated cargo failure"), { code: "COVERAGE_CHILD_FAILED" });
          },
        }),
      (error) => error?.code === "COVERAGE_CHILD_FAILED",
    );
    assert.equal(existsSync(runRoot), false);
    assert.equal(existsSync(lockDir), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_run_signal_cleanup_never_deletes_foreign_owner", () => {
  const acquireCoverageLock = checkerApi("acquireCoverageLock");
  const createCoverageRun = checkerApi("createCoverageRun");
  const createLockOwner = checkerApi("createLockOwner");
  const installCoverageSignalCleanup = checkerApi("installCoverageSignalCleanup");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-signal-"));
  const lockDir = join(root, "coverage.lock");
  try {
    const run = createCoverageRun({ temporaryRoot: root, runId: "signal-owner", now: 1_000 });
    const lock = acquireCoverageLock({
      lockDir,
      owner: createLockOwner({ runId: run.runId, pid: process.pid, now: 1_000, leaseMs: 1_000 }),
      now: () => 1_000,
    });
    const foreignOwner = createLockOwner({ runId: "foreign-live-owner", pid: process.pid, now: 1_000, leaseMs: 1_000 });
    writeFileSync(join(lockDir, "owner.json"), `${JSON.stringify(foreignOwner)}\n`);
    const handlers = new Map();
    const terminalSignals = [];
    const removeHandlers = installCoverageSignalCleanup({
      scope: { run, lock },
      on: (signal, handler) => handlers.set(signal, handler),
      off: (signal) => handlers.delete(signal),
      terminate: (signal) => terminalSignals.push(signal),
    });

    handlers.get("SIGTERM")();

    assert.equal(existsSync(run.root), false);
    assert.equal(existsSync(lockDir), true);
    assert.deepEqual(JSON.parse(readFileSync(join(lockDir, "owner.json"), "utf8")), foreignOwner);
    assert.deepEqual(terminalSignals, ["SIGTERM"]);
    removeHandlers();
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_run_signal_cleanup_covers_lock_wait", () => {
  const createLockOwner = checkerApi("createLockOwner");
  const runWithCoverageIsolation = checkerApi("runWithCoverageIsolation");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-signal-wait-"));
  try {
    const lockDir = join(root, "coverage.lock");
    let now = 1_000;
    const liveOwner = createLockOwner({ runId: "live-owner", pid: process.pid, now, leaseMs: 1_000 });
    writeLockOwner(lockDir, liveOwner);
    const handlers = new Map();
    const terminalSignals = [];
    assert.throws(
      () =>
        runWithCoverageIsolation({
          temporaryRoot: root,
          lockDir,
          runId: "waiting-owner",
          now: () => now,
          waitMs: 10,
          pollMs: 10,
          isPidAlive: () => true,
          sleep: (milliseconds) => {
            assert.equal(typeof handlers.get("SIGINT"), "function");
            handlers.get("SIGINT")();
            now += milliseconds;
          },
          signalOptions: {
            on: (signal, handler) => handlers.set(signal, handler),
            off: (signal) => handlers.delete(signal),
            terminate: (signal) => terminalSignals.push(signal),
          },
          execute: () => assert.fail("a waiting checker must not execute cargo"),
        }),
      (error) => error?.code === "COVERAGE_LOCK_TIMEOUT",
    );
    assert.deepEqual(JSON.parse(readFileSync(join(lockDir, "owner.json"), "utf8")), liveOwner);
    assert.deepEqual(terminalSignals, ["SIGINT"]);
    assert.deepEqual(readdirSync(root).filter((entry) => entry.startsWith("rs-slice-cov-")), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_cli_report_only_preserves_per_file_threshold_and_allow_missing_contract", () => {
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-report-only-"));
  try {
    const reportPath = join(root, "coverage.json");
    const report = {
      data: [
        {
          files: [
            {
              filename: join(REPO_ROOT, COVERED_FILE),
              summary: { lines: { count: 10, covered: 8, percent: 80 } },
            },
          ],
        },
      ],
    };
    writeFileSync(reportPath, `${JSON.stringify(report)}\n`);
    const output = [];
    const errors = [];
    assert.equal(
      coverageChecker.main(
        ["node", "checker", "--report-only", reportPath, "--files", COVERED_FILE, "--min", "80"],
        { print: (line) => output.push(line), error: (line) => errors.push(line) },
      ),
      0,
    );
    assert.equal(output.some((line) => line.includes("Coverage gate PASSED")), true);
    assert.deepEqual(errors, []);

    assert.equal(
      coverageChecker.main(
        ["node", "checker", "--report-only", reportPath, "--files", "crates/ramshared-cli/src/main.rs", "--allow-missing"],
        { print: () => {}, error: () => {} },
      ),
      0,
    );
    assert.equal(
      coverageChecker.main(
        ["node", "checker", "--report-only", reportPath, "--files", COVERED_FILE, "--min", "81"],
        { print: () => {}, error: () => {} },
      ),
      1,
    );
    assert.equal(
      coverageChecker.main(
        ["node", "checker", "--report-only", reportPath, "--files", "crates/no-such-production-file.rs"],
        { print: () => {}, error: () => {} },
      ),
      1,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_cli_refuses_invalid_arguments_and_malformed_report", () => {
  const errors = [];
  assert.equal(
    coverageChecker.main(["node", "checker", "--unknown"], { print: () => {}, error: (line) => errors.push(line) }),
    2,
  );
  assert.equal(errors[0].startsWith("COVERAGE_USAGE_ERROR:"), true);
  assert.equal(
    coverageChecker.main(["node", "checker", "--files"], { print: () => {}, error: () => {} }),
    2,
  );
  assert.equal(
    coverageChecker.main(["node", "checker", "-p", "ramshared-cli", "--files", COVERED_FILE, "--min", "101"], {
      print: () => {},
      error: () => {},
    }),
    2,
  );
  assert.equal(
    coverageChecker.main(["node", "checker", "-p", "ramshared-cli", "--files", COVERED_FILE, "--metric", "bytes"], {
      print: () => {},
      error: () => {},
    }),
    2,
  );
  assert.equal(
    coverageChecker.main(["node", "checker", "--help"], { print: () => {}, error: () => {} }), 0);
  assert.equal(
    coverageChecker.main(["node", "checker", "--report-only", "/missing/report.json", "--files", COVERED_FILE], {
      print: () => {},
      error: () => {},
    }),
    2,
  );
});

test("coverage_parsers_normalize_paths_merge_summaries_and_refuse_invalid_inputs", () => {
  const parseLlvmCovJson = checkerApi("parseLlvmCovJson");
  const loadFilesFrom = checkerApi("loadFilesFrom");
  const normRepoPath = checkerApi("normRepoPath");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-parser-"));
  try {
    const listPath = join(root, "paths.txt");
    writeFileSync(listPath, `# only production paths\n./${COVERED_FILE}\n\n${COVERED_FILE}\n`);
    assert.deepEqual(loadFilesFrom(listPath), [COVERED_FILE, COVERED_FILE]);
    assert.equal(normRepoPath(join(REPO_ROOT, COVERED_FILE)), COVERED_FILE);
    assert.equal(normRepoPath(`./${COVERED_FILE}`), COVERED_FILE);
    assert.throws(() => loadFilesFrom(join(root, "missing.txt")), (error) => error?.code === "COVERAGE_USAGE_ERROR");

    const map = parseLlvmCovJson(
      JSON.stringify({
        data: [
          {
            files: [
              { filename: join(REPO_ROOT, COVERED_FILE), summary: { lines: { count: 2, covered: 1 } } },
              { name: join(REPO_ROOT, COVERED_FILE), summary: { lines: { count: 3, covered: 3 } } },
              { filename: "crates/ramshared-cli/tests/ignored.rs", summary: { lines: { count: 1, covered: 1 } } },
              { filename: "crates/ramshared-cli/src/ignored_test.rs", summary: { lines: { count: 1, covered: 1 } } },
              { filename: "crates/ramshared-cli/src/no-summary.rs", summary: {} },
              { summary: { lines: { count: 1, covered: 1 } } },
            ],
          },
        ],
      }),
      "lines",
    );
    assert.deepEqual(map.get(COVERED_FILE), { count: 5, covered: 4, percent: 80 });
    const zero = parseLlvmCovJson(
      JSON.stringify({ data: [{ files: [{ filename: join(REPO_ROOT, COVERED_FILE), summary: { lines: { count: 0, covered: 0 } } }] }] }),
      "lines",
    );
    assert.equal(zero.get(COVERED_FILE).percent, 100);
    assert.throws(() => parseLlvmCovJson("not-json", "lines"), (error) => error?.code === "COVERAGE_REPORT_INVALID");
    assert.throws(() => parseLlvmCovJson(JSON.stringify({ data: [] }), "lines"), (error) => error?.code === "COVERAGE_REPORT_INVALID");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_lock_and_run_inputs_refuse_invalid_ownership_without_deleting_foreign_state", () => {
  const acquireCoverageLock = checkerApi("acquireCoverageLock");
  const cleanupCoverageRun = checkerApi("cleanupCoverageRun");
  const createCoverageRun = checkerApi("createCoverageRun");
  const createLockOwner = checkerApi("createLockOwner");
  const releaseCoverageLock = checkerApi("releaseCoverageLock");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-invalid-input-"));
  try {
    assert.throws(() => createLockOwner({ pid: 0 }), (error) => error?.code === "COVERAGE_USAGE_ERROR");
    assert.throws(() => createLockOwner({ now: -1 }), (error) => error?.code === "COVERAGE_USAGE_ERROR");
    assert.throws(() => createLockOwner({ leaseMs: 0 }), (error) => error?.code === "COVERAGE_USAGE_ERROR");
    assert.throws(() => createCoverageRun({ temporaryRoot: "", runId: "x" }), (error) => error?.code === "COVERAGE_USAGE_ERROR");
    assert.throws(
      () => acquireCoverageLock({ lockDir: join(root, "bad.lock"), owner: {}, waitMs: 1, pollMs: 1 }),
      (error) => error?.code === "COVERAGE_USAGE_ERROR",
    );

    const run = createCoverageRun({ temporaryRoot: root, runId: "owner" });
    writeFileSync(join(run.root, "owner.json"), "foreign-owner\n");
    assert.equal(cleanupCoverageRun(run), false);
    assert.equal(existsSync(run.root), true);

    const lockDir = join(root, "lock");
    writeLockOwner(lockDir, createLockOwner({ runId: "valid", pid: process.pid, now: 1_000, leaseMs: 1_000 }));
    assert.equal(releaseCoverageLock({ lockDir, owner: createLockOwner({ runId: "other", pid: process.pid, now: 1_000, leaseMs: 1_000 }) }), false);
    writeFileSync(join(lockDir, "owner.json"), "broken\n");
    assert.equal(releaseCoverageLock({ lockDir, owner: createLockOwner({ runId: "valid", pid: process.pid, now: 1_000, leaseMs: 1_000 }) }), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_child_runner_uses_private_target_without_shell_and_propagates_failures", () => {
  const runLlvmCov = checkerApi("runLlvmCov");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-child-runner-"));
  try {
    const cargoRoot = join(root, "cargo-root");
    mkdirSync(cargoRoot);
    writeFileSync(join(cargoRoot, "Cargo.toml"), "[workspace]\n");
    const reportPath = join(root, "result.json");
    const targetPath = join(root, "private-target");
    const commands = [];
    runLlvmCov(["ramshared-cli"], reportPath, targetPath, {
      repoRoot: cargoRoot,
      env: { SAFE: "yes" },
      error: (line) => commands.push(line),
      spawnCommand: (command, args, options) => {
        assert.equal(command, "cargo");
        assert.equal(args.includes("--output-path"), true);
        assert.equal(options.env.CARGO_TARGET_DIR, targetPath);
        assert.equal(options.shell, false);
        writeFileSync(reportPath, "{}\n");
        return { status: 0, stdout: "", stderr: "" };
      },
    });
    assert.equal(commands[0].startsWith("$ cargo llvm-cov"), true);
    assert.equal(commands[0].includes(reportPath), false);
    assert.equal(commands[0].includes("<private-run>/llvm-cov.json"), true);
    assert.throws(
      () =>
        runLlvmCov(["ramshared-cli"], reportPath, targetPath, {
          repoRoot: cargoRoot,
          spawnCommand: () => ({ status: 7, stdout: "", stderr: "" }),
        }),
      (error) => error?.code === "COVERAGE_CHILD_FAILED" && error.exitCode === 7,
    );
    assert.throws(
      () =>
        runLlvmCov(["ramshared-cli"], reportPath, targetPath, {
          repoRoot: cargoRoot,
          spawnCommand: () => ({ status: null, error: new Error("no cargo"), stdout: "", stderr: "" }),
        }),
      (error) => error?.code === "COVERAGE_CHILD_START_FAILED",
    );
    assert.throws(
      () => runLlvmCov(["ramshared-cli"], reportPath, targetPath, { repoRoot: join(root, "missing") }),
      (error) => error?.code === "COVERAGE_TOOL_ROOT_INVALID",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("coverage_child_deadline_is_terminal_and_fail_closed", () => {
  const runLlvmCov = checkerApi("runLlvmCov");
  const root = mkdtempSync(join(tmpdir(), "ramshared-cov-child-timeout-"));
  try {
    writeFileSync(join(root, "Cargo.toml"), "[workspace]\n");
    const reportPath = join(root, "result.json");
    let observedOptions;
    assert.throws(
      () =>
        runLlvmCov(["ramshared-cli"], reportPath, join(root, "private-target"), {
          repoRoot: root,
          error: () => {},
          spawnCommand: (_command, _args, options) => {
            observedOptions = options;
            const error = new Error("manufactured child timeout");
            error.code = "ETIMEDOUT";
            return { status: null, signal: "SIGTERM", error, stdout: "", stderr: "" };
          },
        }),
      (error) => error?.code === "COVERAGE_CHILD_TIMEOUT" && error.exitCode === 2,
    );
    assert.equal(observedOptions.timeout, 15 * 60_000);
    assert.equal(observedOptions.killSignal, "SIGTERM");
    assert.equal(existsSync(reportPath), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
