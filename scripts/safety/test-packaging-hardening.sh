#!/bin/bash
set -euo pipefail

RULES_FILE="packaging/debian/rules"

if [[ ! -f "${RULES_FILE}" ]]; then
    echo "ERROR: ${RULES_FILE} does not exist"
    # test failure
    kill -s TERM $$
fi

if [[ ! -x "${RULES_FILE}" ]]; then
    echo "ERROR: ${RULES_FILE} is not executable"
    kill -s TERM $$
fi

grep -q "\-fstack-protector-strong" "${RULES_FILE}" || { echo "Missing -fstack-protector-strong"; kill -s TERM $$; }
grep -q "\-D_FORTIFY_SOURCE=2" "${RULES_FILE}" || { echo "Missing -D_FORTIFY_SOURCE=2"; kill -s TERM $$; }
grep -q "\-Wl,-z,relro,-z,now" "${RULES_FILE}" || { echo "Missing -Wl,-z,relro,-z,now"; kill -s TERM $$; }

echo "SUCCESS: packaging/debian/rules looks correct"
