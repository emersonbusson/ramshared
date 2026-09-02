#!/usr/bin/env bash
# Sealed NBD-only cascade deactivation. The Rust lifecycle owns swapoff-first
# and leaves the daemon/device alive if swapoff fails.
set -euo pipefail

PRODUCT_ROOT=/opt/ramshared
SELECTOR="$PRODUCT_ROOT/current"

refuse() {
  local reason=$1
  local code=${2:-69}
  printf 'NBD_LIFECYCLE_STATE=REFUSED\n'
  printf 'NBD_LIFECYCLE_REASON=%s\n' "$reason"
  exit "$code"
}

# Guard clauses: Validate missing dependencies (EX_UNAVAILABLE = 69)
command -v readlink >/dev/null || refuse DEPENDENCY_MISSING_READLINK 69
command -v awk >/dev/null || refuse DEPENDENCY_MISSING_AWK 69

[[ -L $SELECTOR ]] || refuse RELEASE_SELECTOR_MISSING 78
RELEASE=$(readlink -f -- "$SELECTOR" 2>/dev/null || true)
[[ $RELEASE == "$PRODUCT_ROOT"/releases/* && -d $RELEASE ]] || refuse RELEASE_SELECTOR_INVALID 78
RELEASE_VERSION=${RELEASE##*/}
CLI="$RELEASE/bin/ramshared"
[[ -x $CLI ]] || refuse RELEASE_LAYOUT_INVALID 78

case $# in
  0)
    printf 'NBD_LIFECYCLE_STATE=PLAN\n'
    printf 'NBD_LIFECYCLE_ACTION=deactivate\n'
    printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
    printf 'NBD_LIFECYCLE_TRANSPORT=nbd\n'
    exit 0
    ;;
  1)
    [[ $1 == --execute ]] || refuse UNSUPPORTED_ARGUMENT 64
    ;;
  *) refuse UNSUPPORTED_ARGUMENT 64 ;;
esac

[[ ${RAMSHARED_NBD_LIFECYCLE_APPROVAL:-} == "deactivate:$RELEASE_VERSION" ]] || refuse APPROVAL_MISSING 78

# Guard: Verify no processes are pinning swap pages before swapoff
if test -f /proc/swaps; then
  if ! awk 'NR>1 && $1 ~ /^\/dev\/nbd/ && $4 > 0 { exit 1 }' /proc/swaps; then
    refuse "SWAP_PAGES_PINNED" 69
  fi
fi

printf 'NBD_LIFECYCLE_STATE=EXECUTING\n'
printf 'NBD_LIFECYCLE_ACTION=deactivate\n'
printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
exec "$CLI" down
