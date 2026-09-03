#!/usr/bin/env bash
set -euo pipefail
# Docs hygiene for RamShared (Node, zero deps).
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 74

declare -a DOCS_CHECK_FAILURES=()

run_gate() {
  local label="$1"
  shift
  echo "docs-check: RUN ${label}"

  # Guard clause: Verify tools exist before execution
  for arg in "$@"; do
    if [[ "$arg" == *.mjs ]] && [ ! -f "$arg" ]; then
      echo "docs-check: EX_UNAVAILABLE (69): Tool dependency $arg not found" >&2
      exit 69
    fi
  done

  if "$@"; then
    echo "docs-check: PASS ${label}"
  else
    local exit_code=$?
    DOCS_CHECK_FAILURES+=("${label}:${exit_code}")
    echo "docs-check: FAIL ${label} (exit=${exit_code})" >&2
  fi
}

if ! command -v node >/dev/null 2>&1; then
  echo "docs-check: EX_UNAVAILABLE (69): node not found" >&2
  exit 69
fi

run_gate documentation-governance node tools/ci/check-documentation-governance.mjs --all
run_gate agent-orchestration node tools/ci/check-agent-orchestration.mjs --check
run_gate comment-language node tools/ci/check-comment-language.mjs --diff origin/main
run_gate documentation-localization node tools/ci/check-documentation-localization.mjs --all
run_gate document-lifecycle node tools/ci/check-document-lifecycle.mjs --all
run_gate documentation-inventory node tools/ci/generate-documentation-inventory.mjs --check
run_gate capability-observations node tools/ci/generate-capability-observations.mjs --check
run_gate task-log node tools/ci/check-task-log.mjs --all
run_gate space-cleanup-receipts node tools/ci/check-space-cleanup-receipts.mjs --check
run_gate campaign-evidence-lifecycle node tools/ci/check-campaign-evidence-lifecycle.mjs --check
run_gate adr-index node tools/ci/check-adr-index.mjs --check
run_gate docs-index node tools/generate-docs-index.mjs --check
run_gate broken-links node tools/check-broken-links.mjs --all
run_gate gap-register node tools/ci/check-gap-register.mjs
run_gate public-hygiene node tools/ci/check-public-hygiene.mjs --candidate
run_gate public-hygiene-tests node --test --test-reporter=dot tools/ci/check-public-hygiene.test.mjs
run_gate legacy-preallocation-removal node tools/ci/check-legacy-preallocation-removal.mjs --candidate
run_gate legacy-preallocation-removal-tests node --experimental-test-coverage \
  --test-coverage-include=tools/ci/check-legacy-preallocation-removal.mjs \
  --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 \
  --test-reporter=dot tools/ci/check-legacy-preallocation-removal.test.mjs
run_gate agent-orchestration-tests node --experimental-test-coverage \
  --test-coverage-include=tools/ci/check-agent-orchestration.mjs \
  --test-coverage-lines=80 --test-coverage-branches=80 --test-coverage-functions=80 \
  --test-reporter=dot tools/ci/check-agent-orchestration.test.mjs
run_gate claim-closure-tests node --test --test-reporter=dot tools/ci/documentation-claim-closure.test.mjs
run_gate documentation-governance-tests node --test --test-reporter=dot tools/ci/check-documentation-governance.test.mjs
run_gate documentation-localization-tests node --test --test-reporter=dot tools/ci/check-documentation-localization.test.mjs
run_gate docs-index-tests node --test --test-reporter=dot tools/generate-docs-index.test.mjs
run_gate validation-schema-tests node --test --test-reporter=dot tools/ci/check-validation-schema.test.mjs
run_gate document-lifecycle-tests node --test --test-reporter=dot tools/ci/check-document-lifecycle.test.mjs
run_gate documentation-inventory-tests node --test --test-reporter=dot tools/ci/generate-documentation-inventory.test.mjs
run_gate capability-observations-tests node --test --test-reporter=dot tools/ci/generate-capability-observations.test.mjs
run_gate task-log-tests node --test --test-reporter=dot tools/ci/check-task-log.test.mjs
run_gate space-cleanup-receipts-tests node --test --test-reporter=dot tools/ci/check-space-cleanup-receipts.test.mjs
run_gate campaign-evidence-lifecycle-tests node --test --test-reporter=dot tools/ci/check-campaign-evidence-lifecycle.test.mjs
run_gate adr-index-tests node --test --test-reporter=dot tools/ci/check-adr-index.test.mjs
run_gate benchmark-evidence-tests node --test --test-reporter=dot tools/ci/check-benchmark-evidence.test.mjs
run_gate spec-evidence-tests node --test --test-reporter=dot tools/ci/check-spec-evidence.test.mjs
run_gate docs-check-aggregation-tests node --test --test-reporter=dot tools/ci/check-docs-check.test.mjs
run_gate benchmark-evidence node tools/ci/check-benchmark-evidence.mjs --check
run_gate spec-evidence node tools/ci/check-spec-evidence.mjs --check

if (( ${#DOCS_CHECK_FAILURES[@]} > 0 )); then
  echo "docs-check: NO-GO (${#DOCS_CHECK_FAILURES[@]} independent failure(s))" >&2
  for failure in "${DOCS_CHECK_FAILURES[@]}"; do echo "docs-check: ${failure}" >&2; done
  exit 1
fi

echo "✓ docs-check OK"
