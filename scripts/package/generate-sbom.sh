#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# Generate SPDX and CycloneDX SBOM for RamShared
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/artifacts/sbom"

usage() {
  cat <<'EOF_USAGE'
Usage: scripts/package/generate-sbom.sh [--out <dir>]

Generates SPDX and CycloneDX SBOMs for the RamShared workspace.
EOF_USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out)
      if [[ -z "${2:-}" ]]; then
        echo "Error: --out requires a directory argument" >&2
        exit 1
      fi
      OUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo is not installed or not in PATH" >&2
  exit 1
fi

if ! command -v cargo-cyclonedx >/dev/null 2>&1; then
  echo "Error: cargo-cyclonedx not found. Run: cargo install cargo-cyclonedx --locked --version 0.5.9" >&2
  exit 1
fi

if ! command -v cargo-sbom >/dev/null 2>&1; then
  echo "Error: cargo-sbom not found. Run: cargo install cargo-sbom --locked" >&2
  exit 1
fi

echo "Generating CycloneDX SBOM..."
cargo cyclonedx --manifest-path Cargo.toml --format json --spec-version 1.5 --override-filename ramshared-sbom.cdx

# Copy generated CycloneDX files preserving structure and cleanup workspace
find crates -name 'ramshared-sbom.cdx.json' | while read -r cdx_file; do
  crate_dir="$(dirname "$cdx_file")"
  crate_name="$(basename "$crate_dir")"
  mkdir -p "$OUT_DIR/$crate_name"
  cp "$cdx_file" "$OUT_DIR/$crate_name/ramshared-sbom.cdx.json"
  rm "$cdx_file"
done

echo "Generating SPDX SBOM..."
cargo sbom --output-format spdx_json_2_3 > "$OUT_DIR/ramshared-sbom.spdx.json"

echo "SBOMs successfully generated in $OUT_DIR"
