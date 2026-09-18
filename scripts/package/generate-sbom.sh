#!/bin/bash
set -euo pipefail

# Physical bounds / Setup
if [ -z "${CARGO_HOME:-}" ]; then
    export CARGO_HOME="$HOME/.cargo"
fi

if ! command -v cargo-cyclonedx > /dev/null 2>&1; then
    echo "cargo-cyclonedx is required but not installed. Aborting." >&2
    exit 1
fi

if ! command -v cargo-sbom > /dev/null 2>&1; then
    echo "cargo-sbom is required but not installed. Aborting." >&2
    exit 1
fi

if ! command -v node > /dev/null 2>&1; then
    echo "node is required but not installed. Aborting." >&2
    exit 1
fi

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT_DIR="${WORKSPACE_ROOT}/dist/sbom"

# Ensure output directory exists and is sandboxed (0755)
mkdir -p "${OUTPUT_DIR}"
chmod 0755 "${OUTPUT_DIR}"

# Secure temp directory with cleanup
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

cd "${WORKSPACE_ROOT}"

echo "Generating CycloneDX SBOM for workspace components..."
# We need to generate SBOMs for ramshared-cli and ramshared-wsl2d, the EXPECTED_ROOTS in the merge script
# cargo cyclonedx --all puts them in the crates directory
cargo cyclonedx --all --format json --spec-version 1.5

echo "Generating SPDX SBOM for workspace..."
cargo sbom > "${TMP_DIR}/workspace.spdx.json"

# Grab required values from git
TAG_NAME="v0.9.0-beta.1" # from regex /^v0\.9\.0-beta\.1$/
GIT_REV="$(git rev-parse HEAD)"
# For deterministic builds: source-date-epoch
GIT_DATE_EPOCH="$(git log -1 --pretty=%ct)"

echo "Merging SBOMs..."
node tools/ci/merge-release-sboms.mjs \
    --input crates/ramshared-cli/ramshared-cli.cdx.json \
    --input crates/ramshared-wsl2d/ramshared-wsl2d.cdx.json \
    --tag "${TAG_NAME}" \
    --revision "${GIT_REV}" \
    --source-date-epoch "${GIT_DATE_EPOCH}" \
    --out "${OUTPUT_DIR}/merged.cdx.json"

echo "Copying SPDX..."
cp "${TMP_DIR}/workspace.spdx.json" "${OUTPUT_DIR}/merged.spdx.json"

# Clean up the generated cdx.json files from crates to leave a clean git tree
rm -f crates/*/*.cdx.json

# Set permissions
chmod 0644 "${OUTPUT_DIR}/merged.cdx.json"
chmod 0644 "${OUTPUT_DIR}/merged.spdx.json"

echo "SBOM generation complete."
