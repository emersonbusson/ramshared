#!/bin/bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <output-tarball> <source-dir>" >&2
  exit 1
fi

OUTPUT_TARBALL="$1"
SOURCE_DIR="$2"

# Validate prerequisites
if ! command -v tar >/dev/null 2>&1; then
  echo "Error: tar command not found." >&2
  exit 1
fi

if ! command -v find >/dev/null 2>&1; then
  echo "Error: find command not found." >&2
  exit 1
fi

if [ ! -d "$SOURCE_DIR" ]; then
  echo "Error: Source directory '$SOURCE_DIR' does not exist or is not a directory." >&2
  exit 1
fi

# Secure temporary directory
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

# Stage files
STAGE_DIR="$TEMP_DIR/ramshared"
mkdir -p "$STAGE_DIR"
cp -a "$SOURCE_DIR/." "$STAGE_DIR/"

# Normalize permissions
find "$STAGE_DIR" -type f -exec chmod 0644 {} +
find "$STAGE_DIR" -type d -exec chmod 0755 {} +
# Ensure executables have execute permission
find "$STAGE_DIR" -type f \( -name "*.sh" -o -name "*.pl" -o -name "*.py" -o -name "ramshared*" \) -exec chmod 0755 {} +

# Create portable tarball with uid 0, gid 0
tar --owner=0 --group=0 -czf "$OUTPUT_TARBALL" -C "$TEMP_DIR" "ramshared"

echo "Tarball created successfully: $OUTPUT_TARBALL"
