#!/bin/bash
set -euo pipefail

# Validate prerequisites
command -v mkdir >/dev/null 2>&1 || { echo "mkdir is required but not installed. Aborting."; exit 1; }
command -v echo >/dev/null 2>&1 || { echo "echo is required but not installed. Aborting."; exit 1; }
command -v chmod >/dev/null 2>&1 || { echo "chmod is required but not installed. Aborting."; exit 1; }

CONF_DIR="/etc/modules-load.d"
CONF_FILE="${CONF_DIR}/ramshared.conf"

if ! test -d "${CONF_DIR}"; then
    mkdir -p "${CONF_DIR}"
fi

echo "ramshared" > "${CONF_FILE}"
chmod 0644 "${CONF_FILE}"
