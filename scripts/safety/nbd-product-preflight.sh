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
ACTION=
SEALED_RELEASE_ROOT=
EXPECTED_RELEASE_VERSION=
EXPECTED_SOURCE_COMMIT=
EXPECTED_MANIFEST_SHA256=
EXPLICIT_BINDING=0
INPUT_BUNDLE_MANIFEST_SHA256=
readonly PROC_STAT_PF_KTHREAD=2097152
declare -a PRODUCT_RELEASE_DAEMON_PATHS=()
declare -a PRODUCT_RELEASE_DAEMON_IDENTITIES=()
PROC_STAT_STATE=
PROC_STAT_FLAGS=
PROC_STATUS_STATE=

declare -A MANIFEST_HASHES=()

block() {
  printf 'NBD_PRODUCT_STATE=BLOCKED\n'
  printf 'NBD_READINESS_REASON=%s\n' "$1"
  exit 1
}

usage() {
  block UNSUPPORTED_ARGUMENT
}

is_sha256() {
  [[ $1 =~ ^[[:xdigit:]]{64}$ ]]
}

is_source_commit() {
  [[ $1 =~ ^[[:xdigit:]]{40}$ ]]
}

is_release_version() {
  [[ $1 =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]]
}

is_product_release_daemon_path() {
  local path=$1 relative version
  [[ $RELEASE_ROOT == /* && $path == "$RELEASE_ROOT"/*/bin/ramsharedd ]] || return 1
  relative=${path#"$RELEASE_ROOT"/}
  [[ $relative == */bin/ramsharedd ]] || return 1
  version=${relative%/bin/ramsharedd}
  [[ -n $version && $version != */* ]] || return 1
  is_release_version "$version" || return 1
  [[ $path == "$RELEASE_ROOT/$version/bin/ramsharedd" ]]
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
  local target resolved version expected_root
  [[ -d $PRODUCT_ROOT && -d $RELEASE_ROOT ]] || block RELEASE_ROOT_MISSING
  require_owner_mode "$PRODUCT_ROOT" 755 PRODUCT_ROOT_OWNER_MODE_INVALID
  require_owner_mode "$RELEASE_ROOT" 755 RELEASE_ROOT_OWNER_MODE_INVALID

  if (( EXPLICIT_BINDING == 1 )); then
    is_release_version "$EXPECTED_RELEASE_VERSION" || block RELEASE_VERSION_INVALID
    expected_root="$RELEASE_ROOT/$EXPECTED_RELEASE_VERSION"
    [[ $SEALED_RELEASE_ROOT == "$expected_root" && -d $SEALED_RELEASE_ROOT && ! -L $SEALED_RELEASE_ROOT ]] \
      || block REVIEWED_RELEASE_BINDING_INVALID
    resolved=$(readlink -f -- "$SEALED_RELEASE_ROOT" 2>/dev/null || true)
    [[ $resolved == "$expected_root" ]] || block REVIEWED_RELEASE_BINDING_INVALID
    RELEASE=$expected_root
    RELEASE_VERSION=$EXPECTED_RELEASE_VERSION
  else
    [[ -L $SELECTOR ]] || block RELEASE_SELECTOR_MISSING
    [[ $(selector_owner_mode "$SELECTOR") == "$EXPECTED_UID:$EXPECTED_GID:777" ]] || block RELEASE_SELECTOR_OWNER_MODE_INVALID
    target=$(readlink -- "$SELECTOR" 2>/dev/null || true)
    [[ $target =~ ^releases/[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || block RELEASE_SELECTOR_INVALID
    version=${target#releases/}
    resolved=$(readlink -f -- "$SELECTOR" 2>/dev/null || true)
    RELEASE="$RELEASE_ROOT/$version"
    [[ $resolved == "$RELEASE" && -d $RELEASE ]] || block RELEASE_SELECTOR_INVALID
    RELEASE_VERSION=$version
  fi
  require_sealed_directory "$RELEASE"
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
  done < <(find "$RELEASE" -type f ! -name SHA256SUMS ! -name INSTALLED_MANIFEST_SHA256 -print0)
  [[ -z $(find "$RELEASE" -type l -print -quit) ]] || block RELEASE_SYMLINK_FORBIDDEN
  [[ -z $(find "$RELEASE" \( -type p -o -type b -o -type c -o -type s \) -print -quit) ]] || block RELEASE_NON_REGULAR_OBJECT
  while IFS= read -r -d '' listed; do
    require_sealed_directory "$listed"
  done < <(find "$RELEASE" -type d -print0)

  [[ -n ${MANIFEST_HASHES[bin/ramshared]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[bin/ramsharedd]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/uninstall-cascade-boot.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade-up.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade-down.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade-health.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/nbd-product-preflight.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/nbd-benchmark-cell.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/nbd-benchmark-cgroup-launch.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/nbd-benchmark-lib.sh]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade_pressure_integrity_worker.py]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[scripts/safety/cascade.conf.example]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[systemd/ramshared-cascade.service]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[systemd/ramshared-cascade-health.service]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[systemd/ramshared-workloads.slice]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[SOURCE_COMMIT]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[SOURCE_BRANCH]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[SOURCE_TREE_STATE]+x} ]] || block RELEASE_LAYOUT_INVALID
  [[ -n ${MANIFEST_HASHES[INSTALL_PROVENANCE.json]+x} ]] || block INSTALL_PROVENANCE_UNMANIFESTED
  [[ -n ${MANIFEST_HASHES[INPUT_BUNDLE_SHA256SUMS]+x} ]] || block INPUT_BUNDLE_MANIFEST_UNMANIFESTED
  [[ -x $RELEASE/bin/ramshared && -x $RELEASE/bin/ramsharedd ]] || block RELEASE_LAYOUT_INVALID
  MANIFEST_DIGEST=$(sha256sum -- "$manifest" | awk '{print $1}')

  [[ -f $RELEASE/INSTALL_PROVENANCE.json && ! -L $RELEASE/INSTALL_PROVENANCE.json ]] || block INSTALL_PROVENANCE_MISSING
  [[ -f $RELEASE/INPUT_BUNDLE_SHA256SUMS && ! -L $RELEASE/INPUT_BUNDLE_SHA256SUMS ]] || block INPUT_BUNDLE_MANIFEST_MISSING
  [[ -f $RELEASE/INSTALLED_MANIFEST_SHA256 && ! -L $RELEASE/INSTALLED_MANIFEST_SHA256 ]] || block INSTALLED_MANIFEST_RECEIPT_MISSING
  require_sealed_file "$RELEASE/INSTALL_PROVENANCE.json"
  require_sealed_file "$RELEASE/INPUT_BUNDLE_SHA256SUMS"
  require_sealed_file "$RELEASE/INSTALLED_MANIFEST_SHA256"
  local installed_receipt
  installed_receipt=$(tr -d '[:space:]' <"$RELEASE/INSTALLED_MANIFEST_SHA256")
  is_sha256 "$installed_receipt" || block INSTALLED_MANIFEST_RECEIPT_MISMATCH
  [[ ${installed_receipt,,} == ${MANIFEST_DIGEST,,} ]] || block INSTALLED_MANIFEST_RECEIPT_MISMATCH

  SOURCE_COMMIT=$(tr -d '[:space:]' <"$RELEASE/SOURCE_COMMIT")
  SOURCE_BRANCH=$(tr -d '[:space:]' <"$RELEASE/SOURCE_BRANCH")
  SOURCE_TREE_STATE=$(tr -d '[:space:]' <"$RELEASE/SOURCE_TREE_STATE")
  is_source_commit "$SOURCE_COMMIT" || block RELEASE_SOURCE_IDENTITY_INVALID
  [[ $SOURCE_BRANCH =~ ^[A-Za-z0-9._/-]{1,200}$ && ( $SOURCE_TREE_STATE == clean || $SOURCE_TREE_STATE == dirty ) ]] \
    || block RELEASE_SOURCE_IDENTITY_INVALID
  if (( EXPLICIT_BINDING == 1 )); then
    [[ ${SOURCE_COMMIT,,} == ${EXPECTED_SOURCE_COMMIT,,} ]] || block REVIEWED_SOURCE_COMMIT_MISMATCH
    [[ ${MANIFEST_DIGEST,,} == ${EXPECTED_MANIFEST_SHA256,,} ]] || block REVIEWED_MANIFEST_MISMATCH
  fi
}

config_value() {
  local key=$1 config="$RELEASE/scripts/safety/cascade.conf.example"
  [[ -f $config && ! -L $config ]] || return 1
  awk -F= -v key="$key" '
    $0 !~ /^[[:space:]]*#/ && $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
      value = $2
      sub(/^[[:space:]]*/, "", value)
      sub(/[[:space:]]*$/, "", value)
      values[++count] = value
    }
    END {
      if (count != 1) exit 1
      print values[1]
    }
  ' "$config"
}

verify_installed_provenance() {
  local values input_digest commit branch tree_state sink identity fs_block available
  values=$(python3 - "$RELEASE/INSTALL_PROVENANCE.json" <<'PY'
import json
import re
import sys

path = sys.argv[1]
try:
    with open(path, encoding="utf-8") as source:
        record = json.load(source)
    expected = {
        "schema_version", "input_bundle_manifest_sha256", "source_commit",
        "source_branch", "source_tree_state", "lower_sink",
    }
    if set(record) != expected or record["schema_version"] != "ramshared-installed-release-provenance/v1":
        raise ValueError("schema")
    lower = record["lower_sink"]
    lower_expected = {
        "canonical_path", "identity_sha256", "filesystem_block_bytes", "available_kib_at_bind",
    }
    if not isinstance(lower, dict) or set(lower) != lower_expected:
        raise ValueError("lower_schema")
    digest = record["input_bundle_manifest_sha256"]
    commit = record["source_commit"]
    branch = record["source_branch"]
    tree_state = record["source_tree_state"]
    sink = lower["canonical_path"]
    identity = lower["identity_sha256"]
    fs_block = lower["filesystem_block_bytes"]
    available = lower["available_kib_at_bind"]
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
        raise ValueError("input_digest")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", commit):
        raise ValueError("source_commit")
    if not isinstance(branch, str) or not re.fullmatch(r"[A-Za-z0-9._/-]{1,200}", branch):
        raise ValueError("source_branch")
    if tree_state not in ("clean", "dirty"):
        raise ValueError("source_tree_state")
    if not isinstance(sink, str) or not re.fullmatch(r"/[A-Za-z0-9._/-]{1,480}", sink):
        raise ValueError("sink")
    if not isinstance(identity, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", identity):
        raise ValueError("identity")
    if type(fs_block) is not int or fs_block < 512 or fs_block > 1048576:
        raise ValueError("fs_block")
    if type(available) is not int or available < 0:
        raise ValueError("available")
    print("\t".join((digest.lower(), commit.lower(), branch, tree_state, sink, identity.lower(), str(fs_block), str(available))))
except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
    raise SystemExit(f"installed provenance invalid: {exc}")
PY
  ) || block INSTALL_PROVENANCE_INVALID
  IFS=$'\t' read -r input_digest commit branch tree_state sink identity fs_block available <<<"$values"
  [[ -n $input_digest && -n $commit && -n $branch && -n $tree_state && -n $sink && -n $identity ]] \
    || block INSTALL_PROVENANCE_INVALID
  [[ $(sha256sum -- "$RELEASE/INPUT_BUNDLE_SHA256SUMS" | awk '{print $1}') == "$input_digest" ]] \
    || block INPUT_BUNDLE_MANIFEST_DIGEST_MISMATCH
  [[ $SOURCE_COMMIT == "$commit" && $SOURCE_BRANCH == "$branch" && $SOURCE_TREE_STATE == "$tree_state" ]] \
    || block INSTALL_PROVENANCE_SOURCE_IDENTITY_MISMATCH
  [[ $(config_value NBD_LOWER_SINK) == "$sink" \
    && $(config_value NBD_LOWER_SINK_TYPE) == directory \
    && $(config_value NBD_LOWER_SINK_IDENTITY_SHA256) == "$identity" \
    && $(config_value NBD_LOWER_SINK_FS_BLOCK_BYTES) == "$fs_block" \
    && $(config_value NBD_LOWER_SINK_BINDING) == bound ]] \
    || block INSTALL_PROVENANCE_BINDING_MISMATCH
  INPUT_BUNDLE_MANIFEST_SHA256=$input_digest
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
  local sink configured_sink configured_type configured_identity configured_fs_block configured_binding canonical_sink canonical_mount df_records
  local available_kib mount_point alignment_bytes free_bytes margin required metadata actual_type actual_identity
  [[ ! -v RAMSHARED_NBD_LOWER_SINK ]] || block LOWER_TIER_ENV_OVERRIDE_FORBIDDEN
  configured_sink=$(config_value NBD_LOWER_SINK) || block LOWER_TIER_RELEASE_UNBOUND
  configured_type=$(config_value NBD_LOWER_SINK_TYPE) || block LOWER_TIER_RELEASE_UNBOUND
  configured_identity=$(config_value NBD_LOWER_SINK_IDENTITY_SHA256) || block LOWER_TIER_RELEASE_UNBOUND
  configured_fs_block=$(config_value NBD_LOWER_SINK_FS_BLOCK_BYTES) || block LOWER_TIER_RELEASE_UNBOUND
  configured_binding=$(config_value NBD_LOWER_SINK_BINDING) || block LOWER_TIER_RELEASE_UNBOUND
  [[ $configured_binding == bound ]] || block LOWER_TIER_RELEASE_UNBOUND
  sink=$configured_sink
  [[ -n $sink && -d $sink && ! -L $sink ]] || block LOWER_TIER_SINK_UNKNOWN
  canonical_sink=$(readlink -f -- "$sink" 2>/dev/null || true)
  [[ -n $canonical_sink && -d $canonical_sink ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  actual_type=$($STAT -c '%F' -- "$canonical_sink" 2>/dev/null || true)
  [[ $actual_type == directory ]] || block LOWER_TIER_TYPE_INVALID
  metadata=$($STAT -c '%d:%i:%u:%g:%a:%F' -- "$canonical_sink" 2>/dev/null || true)
  [[ $metadata =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+:[0-7]{3,4}:directory$ ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
  actual_identity=$(printf '%s\0%s\0%s' "$canonical_sink" "$actual_type" "$metadata" | sha256sum | awk '{print $1}')
  [[ $configured_type == directory ]] || block LOWER_TIER_TYPE_INVALID
  is_sha256 "$configured_identity" || block LOWER_TIER_SINK_IDENTITY_INVALID
  [[ ${actual_identity,,} == ${configured_identity,,} ]] || block LOWER_TIER_SINK_IDENTITY_INVALID
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
  [[ $configured_fs_block == "$alignment_bytes" ]] || block LOWER_TIER_ALIGNMENT_INVALID
  free_bytes=$((available_kib * 1024))
  free_bytes=$(((free_bytes / alignment_bytes) * alignment_bytes))
  margin=$(((VRAM_BYTES + 9) / 10))
  (( margin >= 512 * 1024 * 1024 )) || margin=$((512 * 1024 * 1024))
  required=$((VRAM_BYTES + margin))
  (( free_bytes >= required )) || block LOWER_TIER_SHORTFALL
  LOWER_FREE_BYTES=$free_bytes
  LOWER_REQUIRED_BYTES=$required
  LOWER_TIER_TYPE=$actual_type
  LOWER_TIER_IDENTITY_SHA256=$actual_identity
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
  MANAGED_ZRAM_PRESENT=0
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
    suffix=${filename#"$DEV_ROOT"/zram}
    if [[ $filename == "$DEV_ROOT"/zram* && $suffix =~ ^[0-9]+$ ]]; then
      MANAGED_ZRAM_PRESENT=1
    fi
  done <"$SWAPS_FILE"
  NBD_SWAP_DEVICE=$found
}

proc_entry_is_gone() {
  [[ ! -e $1 && ! -L $1 ]]
}

maybe_disappear_manufactured_proc_entry() {
  local proc_dir=$1 pid=$2
  [[ ${RAMSHARED_NBD_ALLOW_MANUFACTURED_PROC_TEST:-} == 1 &&
    ${RAMSHARED_NBD_TEST_PROC_DISAPPEAR_AFTER_EXISTS_PID:-} == "$pid" &&
    $PROC_ROOT != /proc && -d $PROC_ROOT && ! -L $PROC_ROOT ]] || return 0
  rmdir -- "$proc_dir" 2>/dev/null
}

read_proc_status_kthread_flag() {
  local proc_dir=$1 value
  [[ -r $proc_dir/status ]] || return 2
  value=$(awk -F: '
    $1 == "Kthread" {
      count++
      candidate = $2
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", candidate)
      if (candidate !~ /^[01]$/) bad = 1
      value = candidate
    }
    END {
      if (count != 1 || bad) exit 2
      print value
    }
  ' "$proc_dir/status" 2>/dev/null) || return 2
  [[ $value == 0 || $value == 1 ]] || return 2
  printf '%s\n' "$value"
}

read_proc_stat_fields() {
  local proc_dir=$1 expected_pid=$2 line tail state ppid pgrp session tty_nr tpgid flags
  PROC_STAT_STATE=
  PROC_STAT_FLAGS=
  [[ -r $proc_dir/stat ]] || return 2
  IFS= read -r line <"$proc_dir/stat" || [[ -n $line ]] || return 2
  [[ $line == "$expected_pid ("* && $line == *') '* ]] || return 2
  tail=${line##*) }
  read -r state ppid pgrp session tty_nr tpgid flags _ <<<"$tail"
  [[ $state =~ ^[RSDTZWI]$ && $ppid =~ ^[0-9]+$ && $pgrp =~ ^[0-9]+$ \
    && $session =~ ^[0-9]+$ && $tty_nr =~ ^[0-9]+$ && $tpgid =~ ^-?[0-9]+$ \
    && $flags =~ ^[0-9]+$ ]] || return 2
  PROC_STAT_STATE=$state
  PROC_STAT_FLAGS=$flags
}

read_proc_stat_kthread_flag() {
  local proc_dir=$1 expected_pid=${proc_dir##*/}
  read_proc_stat_fields "$proc_dir" "$expected_pid" || return 2
  if (( PROC_STAT_FLAGS & PROC_STAT_PF_KTHREAD )); then
    printf '1\n'
  else
    printf '0\n'
  fi
}

read_proc_status_state() {
  local proc_dir=$1 expected_pid=$2 line value
  local state_count=0 pid_count=0 tgid_count=0 kthread_count=0
  PROC_STATUS_STATE=
  [[ -r $proc_dir/status ]] || return 2
  while IFS= read -r line || [[ -n $line ]]; do
    case $line in
      State:*)
        (( state_count += 1 ))
        value=${line#State:}
        while [[ $value == [[:space:]]* ]]; do value=${value#?}; done
        while [[ $value == *[[:space:]] ]]; do value=${value%?}; done
        [[ $value == 'Z (zombie)' || $value =~ ^[RSDTWI][[:space:]]+\([^()]+\)$ ]] || return 2
        PROC_STATUS_STATE=${value:0:1}
        ;;
      Pid:*)
        (( pid_count += 1 ))
        value=${line#Pid:}
        while [[ $value == [[:space:]]* ]]; do value=${value#?}; done
        while [[ $value == *[[:space:]] ]]; do value=${value%?}; done
        [[ $value == "$expected_pid" ]] || return 2
        ;;
      Tgid:*)
        (( tgid_count += 1 ))
        value=${line#Tgid:}
        while [[ $value == [[:space:]]* ]]; do value=${value#?}; done
        while [[ $value == *[[:space:]] ]]; do value=${value%?}; done
        [[ $value == "$expected_pid" ]] || return 2
        ;;
      Kthread:*)
        (( kthread_count += 1 ))
        value=${line#Kthread:}
        while [[ $value == [[:space:]]* ]]; do value=${value#?}; done
        while [[ $value == *[[:space:]] ]]; do value=${value%?}; done
        [[ $value == 0 ]] || return 2
        ;;
    esac
  done <"$proc_dir/status" || return 2
  (( state_count == 1 && pid_count == 1 && tgid_count == 1 && kthread_count == 1 )) || return 2
  [[ $PROC_STATUS_STATE =~ ^[RSDTZWI]$ ]]
}

read_proc_stat_state() {
  read_proc_stat_fields "$1" "$2"
}

proc_entry_is_verified_userspace_zombie() {
  local proc_dir=$1 pid=$2
  read_proc_status_state "$proc_dir" "$pid" || return 1
  [[ $PROC_STATUS_STATE == Z ]] || return 1
  read_proc_stat_state "$proc_dir" "$pid" || return 1
  [[ $PROC_STAT_STATE == Z ]] || return 1
  read_proc_status_state "$proc_dir" "$pid" || return 1
  [[ $PROC_STATUS_STATE == Z ]]
}

proc_entry_is_kernel_thread() {
  local proc_dir=$1 status_flag stat_flag
  # Kthread is the kernel's explicit PF_KTHREAD export.  The stat fallback
  # covers kernels predating that status field and uses stat field 9, whose
  # PF_KTHREAD bit is stable for this purpose.
  if status_flag=$(read_proc_status_kthread_flag "$proc_dir"); then
    [[ $status_flag == 1 ]]
    return
  fi
  if stat_flag=$(read_proc_stat_kthread_flag "$proc_dir"); then
    [[ $stat_flag == 1 ]]
    return
  fi
  return 1
}

is_sealed_release_daemon() {
  local path=$1 metadata mode
  [[ -f $path && ! -L $path && -x $path ]] || return 1
  metadata=$(owner_mode "$path")
  [[ $metadata =~ ^${EXPECTED_UID}:${EXPECTED_GID}:([0-7]{3,4})$ ]] || return 1
  mode=${BASH_REMATCH[1]}
  (( (8#$mode & 0222) == 0 ))
}

collect_product_release_identities() {
  local candidate version daemon identity
  PRODUCT_RELEASE_DAEMON_PATHS=()
  PRODUCT_RELEASE_DAEMON_IDENTITIES=()
  for candidate in "$RELEASE_ROOT"/*; do
    [[ -e $candidate || -L $candidate ]] || continue
    [[ -d $candidate && ! -L $candidate ]] || continue
    version=${candidate##*/}
    is_release_version "$version" || continue
    daemon="$candidate/bin/ramsharedd"
    [[ $(owner_mode "$candidate") == "$EXPECTED_UID:$EXPECTED_GID:555" ]] || continue
    PRODUCT_RELEASE_DAEMON_PATHS+=("$daemon")
    if is_sealed_release_daemon "$daemon"; then
      identity=$(stat -Lc '%d:%i' -- "$daemon" 2>/dev/null || true)
      [[ $identity =~ ^[0-9]+:[0-9]+$ ]] || block PROC_EXE_UNREADABLE
      PRODUCT_RELEASE_DAEMON_IDENTITIES+=("$identity")
    else
      PRODUCT_RELEASE_DAEMON_IDENTITIES+=("")
    fi
  done
  (( ${#PRODUCT_RELEASE_DAEMON_PATHS[@]} > 0 )) || block RELEASE_LAYOUT_INVALID
}

find_live_exact_daemon_pids() {
  local proc_dir pid raw_exe raw_exe_without_deleted_suffix executable_identity
  local index matched
  EXACT_DAEMON_PIDS=()
  collect_product_release_identities
  for proc_dir in "$PROC_ROOT"/[1-9]*; do
    [[ -e $proc_dir || -L $proc_dir ]] || continue
    pid=${proc_dir##*/}
    [[ $pid =~ ^[1-9][0-9]*$ ]] || block PROC_ENTRY_MALFORMED
    maybe_disappear_manufactured_proc_entry "$proc_dir" "$pid" || block PROC_ENTRY_MALFORMED
    proc_entry_is_gone "$proc_dir" && continue
    [[ -d $proc_dir && ! -L $proc_dir ]] || block PROC_ENTRY_MALFORMED
    # A userspace zombie has no executable code and cannot serve NBD. Require
    # matching status/stat Z observations before omitting it from liveness.
    proc_entry_is_verified_userspace_zombie "$proc_dir" "$pid" && continue
    if [[ ! -L $proc_dir/exe && -e $proc_dir/exe ]]; then
      block PROC_EXE_MALFORMED
    fi
    if [[ ! -L $proc_dir/exe ]]; then
      proc_entry_is_kernel_thread "$proc_dir" && continue
      proc_entry_is_gone "$proc_dir" && continue
      block PROC_EXE_UNREADABLE
    fi
    raw_exe=$(readlink -- "$proc_dir/exe" 2>/dev/null || true)
    if [[ -z $raw_exe ]]; then
      proc_entry_is_kernel_thread "$proc_dir" && continue
      proc_entry_is_gone "$proc_dir" && continue
      block PROC_EXE_UNREADABLE
    fi
    [[ $raw_exe == /* ]] || block PROC_EXE_MALFORMED
    raw_exe_without_deleted_suffix=${raw_exe% (deleted)}
    executable_identity=$(stat -Lc '%d:%i' -- "$proc_dir/exe" 2>/dev/null || true)
    proc_entry_is_gone "$proc_dir" && continue
    matched=0
    if is_product_release_daemon_path "$raw_exe_without_deleted_suffix"; then
      matched=1
    else
      for index in "${!PRODUCT_RELEASE_DAEMON_PATHS[@]}"; do
        if [[ $raw_exe_without_deleted_suffix == "${PRODUCT_RELEASE_DAEMON_PATHS[$index]}" \
          || ( -n "${PRODUCT_RELEASE_DAEMON_IDENTITIES[$index]}" \
            && $executable_identity == "${PRODUCT_RELEASE_DAEMON_IDENTITIES[$index]}" ) ]]; then
          matched=1
          break
        fi
      done
    fi
    if (( matched == 1 )); then
      EXACT_DAEMON_PIDS+=("$pid")
    fi
  done
}

check_binary_match() {
  local pid raw_exe resolved_exe expected_hash actual_hash
  BINARY_MATCH=NOT_APPLICABLE
  [[ -d $PROC_ROOT && ! -L $PROC_ROOT ]] || block PROC_ROOT_UNREADABLE
  find_live_exact_daemon_pids
  if [[ ! -f $PID_FILE ]]; then
    case ${#EXACT_DAEMON_PIDS[@]} in
      0)
        [[ -z $NBD_SWAP_DEVICE ]] || block DAEMON_PID_MISSING
        (( MANAGED_ZRAM_PRESENT == 0 )) || block MANAGED_ZRAM_PRESENT
        ;;
      1) block DAEMON_PID_MISSING ;;
      *) block DAEMON_PID_AMBIGUOUS ;;
    esac
    return
  fi
  (( ${#EXACT_DAEMON_PIDS[@]} <= 1 )) || block DAEMON_PID_AMBIGUOUS
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
  printf 'NBD_RELEASE_SOURCE_COMMIT=%s\n' "$SOURCE_COMMIT"
  printf 'NBD_RELEASE_MANIFEST_SHA256=%s\n' "$MANIFEST_DIGEST"
  printf 'NBD_INPUT_BUNDLE_MANIFEST_SHA256=%s\n' "$INPUT_BUNDLE_MANIFEST_SHA256"
  printf 'NBD_INSTALL_PROVENANCE=PASS\n'
  printf 'NBD_RELEASE_GATE=PASS\n'
  printf 'NBD_SELECTOR=PASS\n'
  printf 'NBD_LOWER_FREE_BYTES=%s\n' "$LOWER_FREE_BYTES"
  printf 'NBD_LOWER_REQUIRED_BYTES=%s\n' "$LOWER_REQUIRED_BYTES"
  printf 'NBD_LOWER_TIER_TYPE=%s\n' "$LOWER_TIER_TYPE"
  printf 'NBD_LOWER_TIER_IDENTITY_SHA256=%s\n' "$LOWER_TIER_IDENTITY_SHA256"
  printf 'NBD_LOWER_TIER_BINDING=bound\n'
  printf 'NBD_LOWER_TIER_CAPACITY=PASS\n'
  printf 'NBD_RELAY_GATE=PASS\n'
  printf 'NBD_BINARY_MATCH=%s\n' "$BINARY_MATCH"
  printf 'NBD_UBLK_MODULE=%s\n' "$UBLK_MODULE_STATE"
  printf 'NBD_TRANSPORT=%s\n' "$transport"
  printf 'NBD_PRODUCT_STATE=%s\n' "$state"
  printf 'NBD_READINESS_REASON=%s\n' "$reason"
}

while [[ $# -gt 0 ]]; do
  case $1 in
    --check)
      [[ -z $ACTION ]] || usage
      ACTION=check
      ;;
    --sealed-release-root)
      SEALED_RELEASE_ROOT=${2:-}
      EXPLICIT_BINDING=1
      shift
      ;;
    --release-version)
      EXPECTED_RELEASE_VERSION=${2:-}
      EXPLICIT_BINDING=1
      shift
      ;;
    --expected-source-commit)
      EXPECTED_SOURCE_COMMIT=${2:-}
      EXPLICIT_BINDING=1
      shift
      ;;
    --expected-manifest-sha256)
      EXPECTED_MANIFEST_SHA256=${2:-}
      EXPLICIT_BINDING=1
      shift
      ;;
    *) usage ;;
  esac
  shift
done
[[ $ACTION == check ]] || usage
if (( EXPLICIT_BINDING == 1 )); then
  [[ -n $SEALED_RELEASE_ROOT && -n $EXPECTED_RELEASE_VERSION && -n $EXPECTED_SOURCE_COMMIT && -n $EXPECTED_MANIFEST_SHA256 ]] \
    || block REVIEWED_RELEASE_BINDING_INCOMPLETE
  is_source_commit "$EXPECTED_SOURCE_COMMIT" || block REVIEWED_SOURCE_COMMIT_INVALID
  is_sha256 "$EXPECTED_MANIFEST_SHA256" || block REVIEWED_MANIFEST_INVALID
fi
is_decimal "$EXPECTED_UID" || block RELEASE_OWNER_INVALID
is_decimal "$EXPECTED_GID" || block RELEASE_OWNER_INVALID

read_sealed_release
verify_manifest
verify_installed_provenance
read_vram_bytes
check_capacity
check_legacy_ublk
check_relay
read_nbd_swap
check_binary_match
emit_success
