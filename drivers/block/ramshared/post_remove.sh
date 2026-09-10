#!/bin/bash
set -euo pipefail

# Validate prerequisites
command -v rm >/dev/null 2>&1 || { echo "rm is required but not installed. Aborting."; exit 1; }

CONF_FILE="/etc/modules-load.d/ramshared.conf"

if test -f "${CONF_FILE}"; then
    rm -f "${CONF_FILE}"
fi
