#!/bin/bash
set -euo pipefail

# format-patchset.sh
# Generate properly formatted RFC patch series with cover letter and diffstat

# Prerequisites
command -v git >/dev/null 2>&1 || { echo >&2 "git is required but not installed. Aborting."; (exit 1) || return 1 2>/dev/null; }

usage() {
    echo "Usage: $0 [options] <base_commit>"
    echo "Options:"
    echo "  -o, --output-dir <dir>    Output directory for patches (default: patches)"
    echo "  -v, --version <version>   Patch version (e.g. v2)"
    echo "  -r, --rfc                 Prefix subject with [RFC PATCH]"
    echo "  -h, --help                Show this help message"
    (exit 1) || return 1 2>/dev/null
}

OUTPUT_DIR="patches"
PATCH_VERSION=""
RFC_PREFIX=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -o|--output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        -v|--version)
            PATCH_VERSION="v$2"
            shift 2
            ;;
        -r|--rfc)
            RFC_PREFIX="--subject-prefix=\"RFC PATCH\""
            shift
            ;;
        -h|--help)
            usage
            ;;
        -*)
            echo "Unknown option: $1"
            usage
            ;;
        *)
            BASE_COMMIT="$1"
            shift
            ;;
    esac
done

if [[ -z "${BASE_COMMIT:-}" ]]; then
    echo "Error: base_commit is required."
    usage
fi

if ! git rev-parse --verify "$BASE_COMMIT" >/dev/null 2>&1; then
    echo "Error: Invalid base commit: $BASE_COMMIT"
    (exit 1) || return 1 2>/dev/null
fi

mkdir -p "$OUTPUT_DIR"

# Build git format-patch command
CMD=(git format-patch --cover-letter --stat -M "$BASE_COMMIT" -o "$OUTPUT_DIR")

if [[ -n "$PATCH_VERSION" ]]; then
    CMD+=(-v "${PATCH_VERSION#v}")
fi

if [[ -n "$RFC_PREFIX" ]]; then
    CMD+=("--subject-prefix=RFC PATCH")
fi

echo "Generating patchset against $BASE_COMMIT in $OUTPUT_DIR..."
"${CMD[@]}"

echo "Patchset generated successfully."
ls -l "$OUTPUT_DIR"
