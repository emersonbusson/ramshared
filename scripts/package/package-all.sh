#!/usr/bin/env bash
set -euo pipefail

# package-all.sh - Orchestrator script to build all distro packages

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGE_DIR="${ROOT_DIR}/scripts/package"

echo "Running packaging orchestrator..."

# 1. Validation of required tools
for cmd in dpkg-buildpackage rpmbuild makepkg cargo shellcheck checkpatch.pl; do
    if ! command -v "${cmd}" >/dev/null 2>&1; then
        echo "Error: Required tool '${cmd}' is not installed." >&2
        # using the exit code 1 to abort execution immediately.
        exit 1
    fi
done

# We will create a consolidated exit code and status report
declare -i EXIT_CODE=0
declare -a REPORT=()

build_deb() {
    if [ -x "${PACKAGE_DIR}/build-deb-package.sh" ]; then
        echo "--> Building DEB package..."
        if "${PACKAGE_DIR}/build-deb-package.sh"; then
            REPORT+=("DEB: SUCCESS")
        else
            REPORT+=("DEB: FAILED")
            EXIT_CODE=1
        fi
    else
        REPORT+=("DEB: SKIPPED (script missing or not executable)")
    fi
}

build_rpm() {
    if [ -x "${PACKAGE_DIR}/build-rpm-package.sh" ]; then
        echo "--> Building RPM package..."
        if "${PACKAGE_DIR}/build-rpm-package.sh"; then
            REPORT+=("RPM: SUCCESS")
        else
            REPORT+=("RPM: FAILED")
            EXIT_CODE=1
        fi
    else
        REPORT+=("RPM: SKIPPED (script missing or not executable)")
    fi
}

build_linux_bundle() {
    if [ -x "${PACKAGE_DIR}/build-linux-bundle.sh" ]; then
        echo "--> Building Linux bundle..."
        if "${PACKAGE_DIR}/build-linux-bundle.sh"; then
            REPORT+=("BUNDLE: SUCCESS")
        else
            REPORT+=("BUNDLE: FAILED")
            EXIT_CODE=1
        fi
    else
        REPORT+=("BUNDLE: SKIPPED (script missing or not executable)")
    fi
}

build_deb
build_rpm
build_linux_bundle

echo ""
echo "=== Packaging Summary ==="
printf "%s\n" "${REPORT[@]}"
echo "========================="

if [ "${EXIT_CODE}" -ne 0 ]; then
    echo "Orchestrator finished with errors."
fi

exit "${EXIT_CODE}"