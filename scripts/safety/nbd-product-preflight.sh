#!/usr/bin/env bash
# Read-only WSL2 NBD product readiness gate.
# SPEC: docs/specs/no-milestone/wsl2-nbd-product-readiness/SPEC.md
set -euo pipefail

PRODUCT_ROOT=${RAMSHARED_PRODUCT_ROOT:-/opt/ramshared}
RELEASE_ROOT="$PRODUCT_ROOT/releases"
SELECTOR="$PRODUCT_ROOT/current"
PROC_ROOT=${RAMSHARED_NBD_PROC_ROOT:-/proc}
SWAPS_FILE=${RAMSHARED_NBD_SWAPS_FILE:-/proc/swaps}
PID_FILE=${RAMSHARED_NBD_PID_FILE:-/run/ramshared/ramsharedd.pid}
MODULES_FILE=${RAMSHARED_NBD_MODULES_FILE:-/proc/modules}
DEV_ROOT=${RAMSHARED_NBD_DEV_ROOT:-/dev}
SYS_BLOCK_ROOT=${RAMSHARED_NBD_SYS_BLOCK_ROOT:-/sys/block}
SYSTEMCTL=${RAMSHARED_NBD_SYSTEMCTL:-systemctl}
RELAY_HEALTH=${RAMSHARED_NBD_RELAY_HEALTH:-}
DF=${RAMSHARED_NBD_DF:-df}
STAT=${RAMSHARED_NBD_STAT:-stat}
EXPECTED_UID=${RAMSHARED_NBD_EXPECT_UID:-0}
EXPECTED_GID=${RAMSHARED_NBD_EXPECT_GID:-0}

declare -A MANIFEST_HASHES=()

block() {
  printf 'NBD_PRODUCT_STATE=BLOCKED\n'
  printf 'NBD_READINESS_REASON=%s\n' "$1"
  exit 1
}

usage() {
  block UNSUPPORTED_ARGUMENT
}

is_decimal() {
  [[ $1 =~ ^[0-9]+$ ]]
}

owner_mode() {
  "$STAT" -c '%u:%g:%a' -- "$1" 2>/dev/null || true
}

selector_owner_mode() {
  # GNU stat does not dereference a symlink unless given -L, so this is lstat.
  "$STAT" -c '%u:%g:%a' -- "$1" 2>/dev/null || true
}

require_owner_mode() {
  local path=$1 expected_mode=$2 reason=$3
  [[ $(owner_mode "$path") == "$EXPECTED_UID:$EXPECTED_GID:$expected_mode" ]] || block "$reason"
}

require_sealed_file() {
  local path=$1 metadata mode
  metadata=$(owner_mode "$path")
  [[ $metadata =~ ^${EXPECTED_UID}:${EXPECTED_GID}:([0-7]{3,4})$ ]] || block RELEASE_FILE_OWNER_MODE_INVALID
  mode=${BASH_REMATCH[1]}
  (( (8#$mode & 0222) == 0 )) || block RELEASE_FILE_OWNER_MODE_INVALID
}

require_sealed_directory() {
  local path=$1
  [[ $(owner_mode "$path") == "$EXPECTED_UID:$EXPECTED_GID:555" ]] || block RELEASE_DIR_MODE_INVALID
}

safe_manifest_path() {
  local path=$1
  [[ $path =~ ^[A-Za-z0-9][A-Za-z0-9._/-]*$ ]] || return 1
  [[ $path != *'//' && $path != *'/./'* && $path != ../* && $path != */../* && $path != *'/..' ]] || return 1
}

read_sealed_release() {
  local target resolved version
  [[ -d $PRODUCT_ROOT && -d $RELEASE_ROOT && -L $SELECTOR ]] || block RELEASE_SELECTOR_MISSING
  require_owner_mode "$PRODUCT_ROOT" 755 PRODUCT_ROOT_OWNER_MODE_INVALID
  require_owner_mode "$RELEASE_ROOT" 755 RELEASE_ROOT_OWNER_MODE_INVALID
  [[ $(selector_owner_mode "$SELECTOR") == "$EXPECTED_UID:$EXPECTED_GID:777" ]] || block RELEASE_SELECTOR_OWNER_MODE_INVALID

  target=$(readlink -- "$SELECTOR" 2>/dev/null || true)
  [[ $target =~ ^releases/[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || block RELEASE_SELECTOR_INVALID
  version=${target#releases/}
  resolved=$(readlink -f -- "$SELECTOR" 2>/dev/null || true)
  RELEASE="$RELEASE_ROOT/$version"
  [[ $resolved == "$RELEASE" && -d $RELEASE ]] || block RELEASE_SELECTOR_INVALID
  require_sealed_directory "$RELEASE"
  RELEASE_VERSION=$version
}

verify_manifest() {
  local manifest line digest marker relative actual listed
  manifest="$RELEASE/SHA256SUMS"
  [[ -f $manifest && ! -L $manifest ]] || block RELEASE_MANIFEST_MISSING
  require_sealed_file "$manifest"

  while IFS= read -r line || [[ -n $line ]]; do
    [[ $line =~ ^([[:xdigit:]]{64})\ \ \./([A-Za-z0-9][A-Za-z0-9._/-]*)$ ]] || block RELEASE_MANIFEST_FORMAT_INVALID
    digest=${BASH_REMATCH[1],,}
    relative=${BASH_REMATCH[2]}
    safe_manifest_path "$relative" || block RELEASE_MANIFEST_PATH_INVALID
    [[ $relative != SHA256SUMS ]] || block RELEASE_MANIFEST_SELF_REFERENCE
    [[ -z ${MANIFEST_HASHES[$relative]+x} ]] || block RELEASE_MANIFEST_DUPLICATE
    [[ -f "$RELEASE/$relative" && ! -L "$RELEASE/$relative" ]] || block RELEASE_MANIFEST_ENTRY_MISSING
    require_sealed_file "$RELEASE/$relative"
    actual=$(sha256sum -- "$RELEASE/$relative" | awk '{print $1}')
    [[ $actual == "$digest" ]] || block RELEASE_MANIFEST_HASH_MISMATCH
    MANIFEST_HASHES[$relative]=$digest
  done <"$manifest"

  [[ ${#MANIFEST_HASHES[@]} -gt 0 ]] || block RELEASE_MANIFEST_EMPTY
  while IFS= read -r -d '' listed; do
    relative=${listed#"$RELEASE"/}
    [[ -n ${MANIFEST_HASHES[$relative]+x} ]] || block RELEASE_MANIFEST_INCOMPLETE
  done < <(find "$RELEASE" -type f ! -name SHA256SUMS -print0)
  [[ -z $(find "$RELEASE" -type l -print -quit) ]] || block RELEASE_SYMLINK_FORBIDDEN
  [[ -z $(find "$RELEASE" \( -type p -o -type b -o -type c -o -type s \) -print -quit) ]] || block RELEASE_NON_REGULAR_OBJECT
  while IFS= read -r -d '' listed; do
    require_sealed_directory "$listed"
  done < <(find "$RELEASE" -type d -print0)

  [[ -n ${MANIFEST_HASHES[bin/ramshared]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[bin/ramsharedd]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade-up.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade-down.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[systemd/ramshared-cascade.service]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -x $RELEASE/bin/ramshared && -x $RELEASE/bin/ramsharedd ]] || block RELEASE_LAYOUT_INVALID
  MANIFEST_DIGEST=$(sha256sum -- "$manifest" | awk '{print $1}')
}

config_value() {
  local key=$1 config="$RELEASE/scripts/safety/cascade.conf.example"
  [[ -f $config && ! -L $config ]] || return 1
  awk -F= -v key="$key" '
    $0 !~ /^[[:space:]]*#/ && $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
      value = $2
      sub(/^[[:space:]]*/, "", value)
      sub(/[[:space:]]*$/, "", value)
      print value
    }
  ' "$config"
}

read_vram_bytes() {
  local configured
  configured=${RAMSHARED_NBD_VRAM_MIB:-}
  if [[ -z $configured ]]; then
    configured=$(config_value VRAM_MIB || true)
  fi
  is_decimal "$configured" || block VRAM_SIZE_INVALID
  (( configured >= 1 && configured <= 1048576 )) || block VRAM_SIZE_INVALID
  VRAM_BYTES=$((configured * 1024 * 1024))
}

check_capacity() {
  local sink canonical_sink canonical_mount df_records available_kib mount_point alignment_bytes free_bytes margin required
  sink=${RAMSHARED_NBD_LOWER_SINK:-}
  if [[ -z $sink ]]; then
    sink=$(config_value NBD_LOWER_SINK || true)
  fi
  [[ -n $sink && -d $sink ]] || block LOWER_TIER_SINK_UNKNOWN
  canonical_sink=$(readlink -f -- "$sink" 2>/dev/null || true)
  [[ -n $canonical_sink && -d $canonical_sink ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  set +e
  df_records=$("$DF" -Pk -- "$canonical_sink" 2>/dev/null | awk '
    NR > 1 && $4 ~ /^[0-9]+$/ && NF >= 6 {
      mount = $6
      for (field = 7; field <= NF; field++) mount = mount " " $field
      print $4 "\t" mount
    }
  ')
  local df_status=$?
  set -e
  (( df_status == 0 )) || block LOWER_TIER_CAPACITY_UNKNOWN
  [[ $(printf '%s\n' "$df_records" | sed '/^$/d' | wc -l | tr -d '[:space:]') == 1 ]] || block LOWER_TIER_CAPACITY_AMBIGUOUS
  available_kib=${df_records%%$'\t'*}
  mount_point=${df_records#*$'\t'}
  is_decimal "$available_kib" || block LOWER_TIER_CAPACITY_UNKNOWN
  canonical_mount=$(readlink -f -- "$mount_point" 2>/dev/null || true)
  [[ -n $canonical_mount && -d $canonical_mount ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  if [[ $canonical_mount == / ]]; then
    [[ $canonical_sink == /* ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  else
    [[ $canonical_sink == "$canonical_mount" || $canonical_sink == "$canonical_mount"/* ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  fi
  (( available_kib <= 4398046511104 )) || block LOWER_TIER_CAPACITY_UNKNOWN
  alignment_bytes=$("$STAT" -fc '%s' -- "$sink" 2>/dev/null || true)
  is_decimal "$alignment_bytes" || block LOWER_TIER_ALIGNMENT_INVALID
  (( alignment_bytes >= 512 && alignment_bytes <= 1048576 )) || block LOWER_TIER_ALIGNMENT_INVALID
  free_bytes=$((available_kib * 1024))
  free_bytes=$(((free_bytes / alignment_bytes) * alignment_bytes))
  margin=$(((VRAM_BYTES + 9) / 10))
  (( margin >= 512 * 1024 * 1024 )) || margin=$((512 * 1024 * 1024))
  required=$((VRAM_BYTES + margin))
  (( free_bytes >= required )) || block LOWER_TIER_SHORTFALL
  LOWER_FREE_BYTES=$free_bytes
  LOWER_REQUIRED_BYTES=$required
}

systemctl_status() {
  local mode=$1
  set +e
  "$SYSTEMCTL" "$mode" --quiet ramsharedd.service >/dev/null 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status"
}

check_legacy_ublk() {
  local entry active enabled line filename module_state
  [[ -r $SWAPS_FILE ]] || block SWAPS_UNREADABLE
  while IFS= read -r line; do
    read -r filename _ <<<"$line"
    [[ $filename != Filename ]] || continue
    if [[ $line == *'(deleted)'* ]]; then
      block GHOST_MANAGED_SWAP
    fi
    entry=${filename#"$DEV_ROOT"/ublkb}
    if [[ $filename == "$DEV_ROOT"/ublkb* && $entry =~ ^[0-9]+$ ]]; then
      block ACTIVE_UBLK_SWAP
    fi
  done <"$SWAPS_FILE"
  [[ -d $SYS_BLOCK_ROOT ]] || block UBLK_INVENTORY_UNAVAILABLE
  while IFS= read -r entry; do
    [[ $entry =~ ^ublkb[0-9]+$ ]] && block ACTIVE_UBLK_DEVICE
  done < <(find "$SYS_BLOCK_ROOT" -mindepth 1 -maxdepth 1 -printf '%f\n' 2>/dev/null)

  active=$(systemctl_status is-active)
  case $active in
    0) block LEGACY_UBLK_SERVICE_ACTIVE ;;
    3) ;;
    *) block LEGACY_UBLK_SERVICE_UNKNOWN ;;
  esac
  enabled=$(systemctl_status is-enabled)
  case $enabled in
    0) block LEGACY_UBLK_SERVICE_ENABLED ;;
    1) ;;
    *) block LEGACY_UBLK_SERVICE_UNKNOWN ;;
  esac

  module_state=ABSENT
  if [[ -r $MODULES_FILE ]] && awk '$1 == "ublk_drv" { found = 1 } END { exit !found }' "$MODULES_FILE"; then
    module_state=LOADED_INERT
  fi
  UBLK_MODULE_STATE=$module_state
}

check_relay() {
  local relay
  relay=$RELAY_HEALTH
  if [[ -z $relay ]]; then
    relay="$RELEASE/scripts/safety/wsl-relay-health.sh"
  fi
  [[ -x $relay ]] || block RELAY_CHECK_UNAVAILABLE
  "$relay" --check >/dev/null 2>&1 || block RELAY_CHECK_FAILED
}

read_nbd_swap() {
  local line filename suffix found=''
  [[ -r $SWAPS_FILE ]] || block SWAPS_UNREADABLE
  while IFS= read -r line; do
    read -r filename _ <<<"$line"
    [[ $filename != Filename ]] || continue
    if [[ $line == *'(deleted)'* ]]; then
      block GHOST_MANAGED_SWAP
    fi
    suffix=${filename#"$DEV_ROOT"/nbd}
    if [[ $filename == "$DEV_ROOT"/nbd* && $suffix =~ ^[0-9]+$ ]]; then
      [[ -z $found ]] || block NBD_DEVICE_AMBIGUOUS
      found=$filename
    fi
  done <"$SWAPS_FILE"
  NBD_SWAP_DEVICE=$found
}

check_binary_match() {
  local pid raw_exe resolved_exe expected_hash actual_hash
  BINARY_MATCH=NOT_APPLICABLE
  if [[ ! -f $PID_FILE ]]; then
    [[ -z $NBD_SWAP_DEVICE ]] || block DAEMON_PID_MISSING
    return
  fi
  pid=$(tr -d '[:space:]' <"$PID_FILE")
  [[ $pid =~ ^[1-9][0-9]*$ ]] || block DAEMON_PID_INVALID
  [[ -d $PROC_ROOT/$pid ]] || block DAEMON_PID_STALE
  raw_exe=$(readlink -- "$PROC_ROOT/$pid/exe" 2>/dev/null || true)
  [[ -n $raw_exe && $raw_exe != *'(deleted)'* ]] || block BINARY_MATCH_FAILED
  resolved_exe=$(readlink -f -- "$PROC_ROOT/$pid/exe" 2>/dev/null || true)
  [[ $resolved_exe == "$RELEASE/bin/ramsharedd" ]] || block BINARY_MATCH_FAILED
  expected_hash=${MANIFEST_HASHES[bin/ramsharedd]}
  actual_hash=$(sha256sum -- "$resolved_exe" | awk '{print $1}')
  [[ $actual_hash == "$expected_hash" ]] || block BINARY_MATCH_FAILED
  [[ -n $NBD_SWAP_DEVICE ]] || block NBD_LIFECYCLE_INCOMPLETE
  [[ -e $NBD_SWAP_DEVICE ]] || block NBD_DEVICE_MISSING
  BINARY_MATCH=PASS
}

emit_success() {
  local state reason transport
  if [[ -n $NBD_SWAP_DEVICE ]]; then
    state=READY
    reason=all_gates_pass
    transport=nbd
  else
    state=PRODUCT_OFF
    reason=product_off
    transport=none
  fi
  printf 'NBD_RELEASE_VERSION=%s\n' "$RELEASE_VERSION"
  printf 'NBD_RELEASE_MANIFEST_SHA256=%s\n' "$MANIFEST_DIGEST"
  printf 'NBD_RELEASE_GATE=PASS\n'
  printf 'NBD_SELECTOR=PASS\n'
  printf 'NBD_LOWER_FREE_BYTES=%s\n' "$LOWER_FREE_BYTES"
  printf 'NBD_LOWER_REQUIRED_BYTES=%s\n' "$LOWER_REQUIRED_BYTES"
  printf 'NBD_LOWER_TIER_CAPACITY=PASS\n'
  printf 'NBD_RELAY_GATE=PASS\n'
  printf 'NBD_BINARY_MATCH=%s\n' "$BINARY_MATCH"
  printf 'NBD_UBLK_MODULE=%s\n' "$UBLK_MODULE_STATE"
  printf 'NBD_TRANSPORT=%s\n' "$transport"
  printf 'NBD_PRODUCT_STATE=%s\n' "$state"
  printf 'NBD_READINESS_REASON=%s\n' "$reason"
}

[[ $# -eq 1 && $1 == --check ]] || usage
is_decimal "$EXPECTED_UID" || block RELEASE_OWNER_INVALID
is_decimal "$EXPECTED_GID" || block RELEASE_OWNER_INVALID

read_sealed_release
verify_manifest
read_vram_bytes
check_capacity
check_legacy_ublk
check_relay
read_nbd_swap
check_binary_match
emit_success
