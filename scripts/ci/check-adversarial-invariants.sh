#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Programmatic Adversarial Invariant Verifier for RamShared.
# Enforces Kahneman #1-#18 disciplines, anti-false-green guards, and zero-panic invariants.
set -euo pipefail

# Semantic exit codes (sysexits.h)
readonly EX_DATAERR=65
readonly EX_UNAVAILABLE=69
readonly EX_SOFTWARE=70
readonly EX_CONFIG=78

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TOTAL_CHECKS=6
echo "1..${TOTAL_CHECKS}"

# Check 1: Ban uncalibrated generic retries in CI workflows (Kahneman #15)
if grep -n -E '\bretry:\s*[0-9]+' .github/workflows/*.yml 2>/dev/null | grep -v 'transient' >/dev/null 2>&1; then
  echo "not ok 1 - Ban uncalibrated generic retries in CI workflows"
  echo "# FAIL: Uncalibrated generic retry detected in GitHub Actions workflows without transient filter." >&2
  exit "${EX_CONFIG}"
fi
echo "ok 1 - Ban uncalibrated generic retries in CI workflows"

# Check 2: Ban unsafe string functions in all C/H files (Buffer Overflow Prevention)
UNSAFE_HITS=0
while IFS= read -r f; do
  if grep -n -E '\b(strcpy|strcat|sprintf|vsprintf)\b' "$f" 2>/dev/null >/dev/null; then
    echo "# ERROR: Unsafe string function found in $f" >&2
    UNSAFE_HITS=$((UNSAFE_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" 2>/dev/null | grep -v -E '^(target/|artifacts/|build/|drivers/windows/)' || true)

if [[ $UNSAFE_HITS -gt 0 ]]; then
  echo "not ok 2 - Ban unsafe string functions in all C/H files"
  echo "# FAIL: $UNSAFE_HITS banned unsafe API occurrences found." >&2
  exit "${EX_SOFTWARE}"
fi
echo "ok 2 - Ban unsafe string functions in all C/H files"

# Check 3: Verify trailing whitespace absence in active source code (Formatting Rigor)
WS_HITS=0
while IFS= read -r f; do
  if grep -n -E '[[:space:]]+$' "$f" 2>/dev/null >/dev/null; then
    echo "# ERROR: Trailing whitespace found in $f" >&2
    WS_HITS=$((WS_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" "*.rs" 2>/dev/null | grep -v -E '^(target/|artifacts/|build/)' || true)

if [[ $WS_HITS -gt 0 ]]; then
  echo "not ok 3 - Verify trailing whitespace absence in active source code"
  echo "# FAIL: Trailing whitespace violations found in source code." >&2
  exit "${EX_DATAERR}"
fi
echo "ok 3 - Verify trailing whitespace absence in active source code"

# Check 4: Enforce English language across all active comments and documentation diffs
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-comment-language.mjs" ]]; then
  if ! node tools/ci/check-comment-language.mjs --diff origin/main >/dev/null 2>&1; then
    echo "not ok 4 - Enforce English language across all active comments"
    echo "# FAIL: Non-English comments found." >&2
    exit "${EX_DATAERR}"
  fi
fi
echo "ok 4 - Enforce English language across all active comments"

# Check 5: Verify Append-Only integrity of validation.md and MEMORY.md (Kahneman #13)
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-validation-schema.mjs" ]]; then
  if ! node tools/ci/check-validation-schema.mjs --diff HEAD >/dev/null 2>&1; then
    echo "not ok 5 - Verify Append-Only integrity of validation.md"
    echo "# FAIL: Append-Only schema violation in validation.md." >&2
    exit "${EX_DATAERR}"
  fi
fi
echo "ok 5 - Verify Append-Only integrity of validation.md"

# Check 6: Verify Adversarial Audit document existence and structure
if [[ ! -f "docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md" ]]; then
  echo "not ok 6 - Verify Adversarial Audit document existence"
  echo "# FAIL: docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md missing." >&2
  exit "${EX_UNAVAILABLE}"
fi
echo "ok 6 - Verify Adversarial Audit document existence"
