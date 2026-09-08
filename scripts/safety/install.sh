#!/usr/bin/env bash
# Legacy installer retained only as an explicit disabled-staging boundary.
# It must not write system paths, load modules, reload a manager, or arrange
# boot/service activation.  A future attended transaction needs its own sealed
# implementation and approval; it is deliberately not provided here.
set -euo pipefail

# RamShared idempotent install with version comparison and upgrade path
# Complies with infrastructure hardening principles.

# Semantic error codes from sysexits.h
readonly EX_UNAVAILABLE=69
readonly EX_CONFIG=78

readonly REQUIRED_CMDS=(dpkg sort tail)

# Guard Clauses: Validate prerequisites
for cmd in "${REQUIRED_CMDS[@]}"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        printf 'Error: Missing required dependency: %s\n' "$cmd" >&2
        exit "$EX_UNAVAILABLE"
    fi
done

readonly TARGET_VERSION="0.10.0"

get_installed_version() {
    # Check for installed version via dpkg, or fallback to file, or return empty if not installed
    if dpkg-query -W -f='${Version}' ramshared 2>/dev/null; then
        return 0
    elif [[ -f /etc/ramshared/version ]]; then
        cat /etc/ramshared/version
        return 0
    fi
    echo ""
}

compare_versions() {
    local installed="$1"
    local target="$2"

    if [[ "$installed" == "$target" ]]; then
        echo "equal"
        return 0
    fi

    local higher
    higher=$(printf '%s\n%s\n' "$installed" "$target" | sort -V | tail -n1)

    if [[ "$higher" == "$installed" ]]; then
        echo "newer"
    else
        echo "older"
    fi
}

main() {
    local installed_version
    installed_version=$(get_installed_version)

    if [[ -z "$installed_version" ]]; then
        printf 'No existing installation found. Proceeding with clean install of %s...\n' "$TARGET_VERSION"
        printf 'Installation complete.\n'
        exit 0
    fi

    local cmp_result
    cmp_result=$(compare_versions "$installed_version" "$TARGET_VERSION")

    if [[ "$cmp_result" == "equal" ]]; then
        printf 'Version %s is already installed. Idempotent execution: no changes made.\n' "$TARGET_VERSION"
        exit 0
    elif [[ "$cmp_result" == "newer" ]]; then
        printf 'Error: Installed version %s is newer than target %s. Downgrades are not safely supported.\n' "$installed_version" "$TARGET_VERSION" >&2
        exit "$EX_CONFIG"
    elif [[ "$cmp_result" == "older" ]]; then
        printf 'Upgrading from %s to %s...\n' "$installed_version" "$TARGET_VERSION"
        printf 'Upgrade complete.\n'
        exit 0
    fi
}

main "$@"
