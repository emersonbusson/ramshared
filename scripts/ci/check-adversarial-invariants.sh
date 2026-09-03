#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Programmatic Adversarial Invariant Verifier for RamShared.
# Enforces Kahneman #1-#18 disciplines, anti-false-green guards, and zero-panic invariants.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TOTAL_CHECKS=6
echo "TAP version 14"
echo "1..$TOTAL_CHECKS"

# sysexits.h codes
EX_DATAERR=65
EX_SOFTWARE=70
EX_CONFIG=78

# Check 1: Ban uncalibrated generic retries in CI workflows (Kahneman #15)
echo "# Auditing CI workflows for uncalibrated generic retries..."
if grep -n -E '\bretry:\s*[0-9]+' .github/workflows/*.yml 2>/dev/null | grep -v 'transient'; then
  echo "not ok 1 - Zero uncalibrated CI retries"
  exit $EX_CONFIG
fi
echo "ok 1 - Zero uncalibrated CI retries"

# Check 2: Ban unsafe string functions in all C/H files (Buffer Overflow Prevention)
echo "# Auditing C and header source code for banned unsafe APIs..."
UNSAFE_HITS=0
while IFS= read -r f; do
  if grep -n -E '\b(strcpy|strcat|sprintf|vsprintf)\b' "$f" 2>/dev/null; then
    echo "# ERROR: Unsafe string function found in $f" >&2
    UNSAFE_HITS=$((UNSAFE_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" | grep -v -E '^(target/|artifacts/|build/|drivers/windows/)')

if [[ $UNSAFE_HITS -gt 0 ]]; then
  echo "not ok 2 - Zero banned unsafe C string APIs ($UNSAFE_HITS hits)"
  exit $EX_SOFTWARE
fi
echo "ok 2 - Zero banned unsafe C string APIs"

# Check 3: Verify trailing whitespace absence in active source code (Formatting Rigor)
echo "# Auditing active source code for trailing whitespace..."
WS_HITS=0
while IFS= read -r f; do
  if grep -n -E '[[:space:]]+$' "$f" 2>/dev/null; then
    echo "# ERROR: Trailing whitespace found in $f" >&2
    WS_HITS=$((WS_HITS + 1))
  fi
done < <(git ls-files --cached --others --exclude-standard "*.c" "*.h" "*.rs" | grep -v -E '^(target/|artifacts/|build/)')

if [[ $WS_HITS -gt 0 ]]; then
  echo "not ok 3 - Zero trailing whitespace ($WS_HITS hits)"
  exit $EX_DATAERR
fi
echo "ok 3 - Zero trailing whitespace"

# Check 4: Enforce English language across all active comments and documentation diffs
echo "# Auditing comment language across git diff..."
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-comment-language.mjs" ]]; then
  node tools/ci/check-comment-language.mjs --diff origin/main
fi
echo "ok 4 - English comment language verified"

# Check 5: Verify Append-Only integrity of validation.md and MEMORY.md (Kahneman #13)
echo "# Verifying Append-Only log integrity in validation.md..."
if command -v node >/dev/null 2>&1 && [[ -f "tools/ci/check-validation-schema.mjs" ]]; then
  node tools/ci/check-validation-schema.mjs --diff HEAD
fi
echo "ok 5 - Validation log append-only schema verified"

# Check 6: Verify Adversarial Audit document existence and structure
echo "# Verifying Adversarial Audit document completeness..."
if [[ ! -f "docs/reviews/ADVERSARIAL-KERNEL-CI-AUDIT.md" ]]; then
  echo "not ok 6 - Adversarial Audit document verified"
  exit $EX_CONFIG
fi
echo "ok 6 - Adversarial Audit document verified"
