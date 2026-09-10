#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only

set -euo pipefail

# Function to verify prerequisites
check_prerequisites() {
    if ! command -v modinfo >/dev/null 2>&1; then
        echo "Error: modinfo is required but not installed." >&2
        exit 1
    fi
}

main() {
    check_prerequisites

    if [[ $# -lt 1 ]]; then
        echo "Error: Missing module path argument." >&2
        echo "Usage: $0 <path_to_module.ko>" >&2
        exit 1
    fi

    local module_path="$1"

    if [[ ! -f "${module_path}" ]]; then
        echo "Error: Built module not found at ${module_path}." >&2
        exit 1
    fi

    local signer
    signer="$(modinfo -F signer "${module_path}" 2>/dev/null || true)"

    if [[ -z "${signer}" ]]; then
        echo "Error: Module ${module_path} is not signed (no signer field found)." >&2
        exit 1
    fi

    echo "Success: Module ${module_path} is signed by '${signer}'."
}

main "$@"
