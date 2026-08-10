#!/usr/bin/env bash
# Docs hygiene for RamShared (Node, zero deps).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v node >/dev/null 2>&1; then
  echo "docs-check: node not found" >&2
  exit 1
fi

node tools/ci/check-documentation-governance.mjs --all
node tools/ci/check-documentation-localization.mjs --all
node tools/generate-docs-index.mjs --check
node tools/check-broken-links.mjs --check
node tools/ci/check-gap-register.mjs
node tools/ci/check-public-hygiene.mjs --candidate
node --test --test-reporter=dot tools/ci/check-public-hygiene.test.mjs
node --test --test-reporter=dot tools/ci/check-documentation-governance.test.mjs
node --test --test-reporter=dot tools/ci/check-documentation-localization.test.mjs
node --test --test-reporter=dot tools/generate-docs-index.test.mjs
node --test --test-reporter=dot tools/ci/check-validation-schema.test.mjs
node --test --test-reporter=dot tools/ci/check-benchmark-evidence.test.mjs
node --test --test-reporter=dot tools/ci/check-spec-evidence.test.mjs
node tools/ci/check-benchmark-evidence.mjs --check
node tools/ci/check-spec-evidence.mjs --check
echo "✓ docs-check OK"
