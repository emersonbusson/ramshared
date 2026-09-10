#!/bin/bash
set -euo pipefail

command -v systemd-analyze >/dev/null 2>&1 || { echo "systemd-analyze not found"; (exit 1) || return 1 2>/dev/null; }
command -v mktemp >/dev/null 2>&1 || { echo "mktemp not found"; (exit 1) || return 1 2>/dev/null; }
command -v find >/dev/null 2>&1 || { echo "find not found"; (exit 1) || return 1 2>/dev/null; }
command -v sed >/dev/null 2>&1 || { echo "sed not found"; (exit 1) || return 1 2>/dev/null; }
command -v awk >/dev/null 2>&1 || { echo "awk not found"; (exit 1) || return 1 2>/dev/null; }

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TEMP_DIR}"' EXIT

mapfile -t UNITS < <(find "${ROOT_DIR}" -type f \( -name "*.service" -o -name "*.timer" \))

if [[ ${#UNITS[@]} -eq 0 ]]; then
    echo "No unit files found."
    exit 0
fi

mkdir -p "${TEMP_DIR}/etc/systemd/system"

for UNIT_FILE in "${UNITS[@]}"; do
    UNIT_NAME="$(basename "${UNIT_FILE}")"
    TMP_UNIT="${TEMP_DIR}/etc/systemd/system/${UNIT_NAME}"
    sed 's|@SCRIPTS_PATH@|/opt/ramshared/scripts|g' "${UNIT_FILE}" > "${TMP_UNIT}"

    while read -r line; do
        if [[ "${line}" =~ ^Exec[a-zA-Z]*= ]]; then
            val="${line#*=}"
            val="${val#-}"
            val="${val#@}"
            val="${val#+}"
            val="${val#!}"
            val="${val#!}"

            exe="$(echo "${val}" | awk '{print $1}')"

            if [[ -n "${exe}" && "${exe}" == /* ]]; then
                rel_exe="${exe#/}"
                dummy_path="${TEMP_DIR}/${rel_exe}"
                if [[ ! -e "${dummy_path}" ]]; then
                    mkdir -p "$(dirname "${dummy_path}")"
                    touch "${dummy_path}"
                    chmod +x "${dummy_path}"
                fi
            fi
        fi
    done < "${TMP_UNIT}"
done

FAILED=0
for UNIT_FILE in "${UNITS[@]}"; do
    UNIT_NAME="$(basename "${UNIT_FILE}")"
    TMP_UNIT="${TEMP_DIR}/etc/systemd/system/${UNIT_NAME}"

    set +e
    OUTPUT=$(systemd-analyze verify --root="${TEMP_DIR}" "${TMP_UNIT}" 2>&1)
    set -e

    CLEAN_OUTPUT=$(printf "%s\n" "${OUTPUT}" | grep -v "Failed to create .*/start: Unit sysinit.target not found" || true)
    CLEAN_OUTPUT=$(printf "%s\n" "${CLEAN_OUTPUT}" | sed '/^[[:space:]]*$/d')

    if [[ -n "${CLEAN_OUTPUT}" ]]; then
        echo "Error: ${UNIT_NAME} failed validation."
        printf "%s\n" "${CLEAN_OUTPUT}"
        FAILED=1
    fi
done

if [[ ${FAILED} -ne 0 ]]; then
    echo "One or more units failed validation."
    (exit 1) || return 1 2>/dev/null
fi

echo "All units validated successfully."
