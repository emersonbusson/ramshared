#!/usr/bin/env bash
# Sealed NBD-only cascade deactivation. The Rust lifecycle owns swapoff-first
# and leaves the daemon/device alive if swapoff fails.
set -euo pipefail

PRODUCT_ROOT=/opt/ramshared
SELECTOR="$PRODUCT_ROOT/current"

refuse() {
  printf 'NBD_LIFECYCLE_STATE=REFUSED\n'
  printf 'NBD_LIFECYCLE_REASON=%s\n' "$1"
  exit 1
}

[[ -L $SELECTOR ]] || refuse RELEASE_SELECTOR_MISSING
RELEASE=$(readlink -f -- "$SELECTOR" 2>/dev/null || true)
[[ $RELEASE == "$PRODUCT_ROOT"/releases/* && -d $RELEASE ]] || refuse RELEASE_SELECTOR_INVALID
RELEASE_VERSION=${RELEASE##*/}
CLI="$RELEASE/bin/ramshared"
[[ -x $CLI ]] || refuse RELEASE_LAYOUT_INVALID

case $# in
  0)
    printf 'NBD_LIFECYCLE_STATE=PLAN\n'
    printf 'NBD_LIFECYCLE_ACTION=deactivate\n'
    printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
    printf 'NBD_LIFECYCLE_TRANSPORT=nbd\n'
    exit 0
    ;;
  1)
    [[ $1 == --execute ]] || refuse UNSUPPORTED_ARGUMENT
    ;;
  *) refuse UNSUPPORTED_ARGUMENT ;;
esac

[[ ${RAMSHARED_NBD_LIFECYCLE_APPROVAL:-} == "deactivate:$RELEASE_VERSION" ]] || refuse APPROVAL_MISSING
printf 'NBD_LIFECYCLE_STATE=EXECUTING\n'
printf 'NBD_LIFECYCLE_ACTION=deactivate\n'
printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
exec "$CLI" down
