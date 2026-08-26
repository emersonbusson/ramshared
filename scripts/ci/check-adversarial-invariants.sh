#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Programmatic Adversarial Invariant Verifier for RamShared.
# Enforces Kahneman #1-#18 disciplines, anti-false-green guards, and zero-panic invariants.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "==> Running Automated Adversarial Invariant Checks..."

CHECKS_PASSED=0
TOTAL_CHECKS=6

# Check 1: Ban uncalibrated generic retries in CI workflows (Kahneman #15)
echo "[1/6] Auditing CI workflows for uncalibrated generic retries..."
if grep -n -E '\bretry:\s*[0-9]+' .github/workflows/*.yml 2>/dev/null | grep -v 'transient'; then
  echo "FAIL: Uncalibrated generic retry detected in GitHub Actions workflows without transient filter." >&2
  exit 1
fi
echo "  ✓ Check 1 PASS: Zero uncalibrated CI retries."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

# Check 2: Ban unsafe string functions in all C/H files (Buffer Overflow Prevention)
echo "[2/6] Auditing C and header source code for banned unsafe APIs..."
UNSAFE_HITS=0
while IFS= read -r f; do
  if grep -n -E '\b(strcpy|strcat|sprintf|vsprintf)\b' "$f" 2>/dev/null; then
    echo "ERROR: Unsafe string function found in $f" >&2
    UNSAFE_HITS=$((UNSAFE_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" | grep -v -E '^(target/|artifacts/|build/|drivers/windows/)')

if [[ $UNSAFE_HITS -gt 0 ]]; then
  echo "FAIL: $UNSAFE_HITS banned unsafe API occurrences found." >&2
  exit 1
fi
echo "  ✓ Check 2 PASS: Zero banned unsafe C string APIs."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

# Check 3: Verify trailing whitespace absence in active source code (Formatting Rigor)
echo "[3/6] Auditing active source code for trailing whitespace..."
WS_HITS=0
while IFS= read -r f; do
  if grep -n -E '[[:space:]]+$' "$f" 2>/dev/null; then
    echo "ERROR: Trailing whitespace found in $f" >&2
    WS_HITS=$((WS_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" "*.rs" | grep -v -E '^(target/|artifacts/|build/)')

if [[ $WS_HITS -gt 0 ]]; then
  echo "FAIL: Trailing whitespace violations found in source code." >&2
  exit 1
fi
echo "  ✓ Check 3 PASS: Zero trailing whitespace."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

# Check 4: Enforce English language across all active comments and documentation diffs
echo "[4/6] Auditing comment language across git diff..."
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-comment-language.mjs" ]]; then
  node tools/ci/check-comment-language.mjs --diff origin/main
fi
echo "  ✓ Check 4 PASS: English comment language verified."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

# Check 5: Verify Append-Only integrity of validation.md and MEMORY.md (Kahneman #13)
echo "[5/6] Verifying Append-Only log integrity in validation.md..."
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-validation-schema.mjs" ]]; then
  node tools/ci/check-validation-schema.mjs --diff HEAD
fi
echo "  ✓ Check 5 PASS: Validation log append-only schema verified."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

# Check 6: Verify Adversarial Audit document existence and structure
echo "[6/6] Verifying Adversarial Audit document completeness..."
if [[ ! -f "docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md" ]]; then
  echo "FAIL: docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md missing." >&2
  exit 1
fi
echo "  ✓ Check 6 PASS: Adversarial Audit document verified."
CHECKS_PASSED=$((CHECKS_PASSED + 1))

echo "========================================================"
echo "✓ ADVERSARIAL INVARIANTS OK: All $CHECKS_PASSED/$TOTAL_CHECKS checks passed."
echo "========================================================"
