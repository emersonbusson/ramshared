# Fuzzing Guide

## Overview

This guide details the continuous fuzzing infrastructure, `cargo-fuzz` corpus management, coverage-guided workflows, and CI integration for `ramshared`.

## 1. Corpus Management

Fuzzing corpora are managed locally and synchronized with the CI environment.

*   **Initialization:** Corpora are automatically initialized using standard payloads.
*   **Minimization:** Run `cargo fuzz cmin <fuzz_target>` regularly to minimize the corpus.
*   **Storage:** The master corpus is tracked securely; do not commit large binary corpora directly to the main tree without minimization and approval.

## 2. Coverage-Guided Fuzzing

*   **Tooling:** We rely on `cargo-fuzz` (libFuzzer).
*   **Execution:** Run a fuzzer via `cargo +nightly fuzz run <fuzz_target>`.
*   **Coverage Reports:** Generate coverage using `cargo +nightly fuzz coverage <fuzz_target>`.

## 3. CI Integration (Regression Tests)

Fuzzing regression tests are integrated into our CI pipeline.

*   **Execution:** CI runs `cargo fuzz run <fuzz_target> -- -max_total_time=300` to prevent regressions.
*   **Failures:** Any crash found by the fuzzer fails the CI run immediately.
*   **Artifacts:** Crash artifacts are uploaded as part of the CI run for debugging.
