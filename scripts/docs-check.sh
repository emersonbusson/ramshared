#!/usr/bin/env bash
# Docs hygiene for RamShared (Node, zero deps).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v node >/dev/null 2>&1; then
  echo "docs-check: node not found" >&2
  exit 1
fi

node tools/generate-docs-index.mjs --check
node tools/check-broken-links.mjs --check
node tools/ci/check-gap-register.mjs
node tools/ci/check-public-hygiene.mjs
echo "✓ docs-check OK"
