#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Generate SBOM (CycloneDX and SPDX) for RamShared workspace

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${RAMSHARED_PACKAGE_OUT:-$ROOT/artifacts/sbom}"

usage() {
  cat <<'HELP'
Usage: scripts/package/generate-sbom.sh [options]

Generates CycloneDX and SPDX SBOMs for the cargo workspace.

Options:
  --help, -h    Show this help message
HELP
}

for arg in "$@"; do
  case $arg in
    --help|-h) usage; trap - EXIT; kill -s TERM $$ ;;
    *) printf 'unsupported argument: %s\n' "$arg" >&2; usage >&2; trap - EXIT; kill -s TERM $$ ;;
  esac
done

if ! command -v cargo-cyclonedx >/dev/null 2>&1; then
  echo "ERROR: missing cargo-cyclonedx tool (version 0.5.9)" >&2
  trap - EXIT; kill -s TERM $$
fi

if ! command -v cargo-sbom >/dev/null 2>&1; then
  echo "ERROR: missing cargo-sbom tool" >&2
  trap - EXIT; kill -s TERM $$
fi

echo "==> Generating CycloneDX SBOM..."
mkdir -p "$OUT_DIR"

cd "$ROOT"
# The CI tools expect exactly version 0.5.9 and output format per spec
cargo cyclonedx --manifest-path Cargo.toml --format json --spec-version 1.5 --override-filename ramshared-sbom.cdx

# Copy generated CycloneDX files to output directory
find "$ROOT" -name "ramshared-sbom.cdx.json" -type f ! -path "*/artifacts/*" -exec cp {} "$OUT_DIR/ramshared-cyclonedx.json" \;

echo "==> Generating SPDX SBOM..."
cargo sbom --output-format spdx_json_2_3 > "$OUT_DIR/ramshared-spdx.json"

echo "==> SBOM generation complete in $OUT_DIR"
