#!/usr/bin/env bash
# Install one already-built, sealed NBD release. The no-argument path is a
# read-only plan; every filesystem or systemd write needs exact version scope.
set -euo pipefail

SOURCE_RELEASE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
PRODUCT_ROOT=/opt/ramshared
RELEASE_ROOT="$PRODUCT_ROOT/releases"
UNIT_PATH=/etc/systemd/system/ramshared-cascade.service
HEALTH_UNIT_PATH=/etc/systemd/system/ramshared-cascade-health.service
WORKLOADS_SLICE_PATH=/etc/systemd/system/ramshared-workloads.slice
CURRENT_SELECTOR="$PRODUCT_ROOT/current"
APPROVED_VERSION=
LEGACY_UNIT_APPROVED_HASH=
LOWER_SINK=
INPUT_BUNDLE_MANIFEST_SHA256=
INSTALLED_MANIFEST_SHA256=
BOUND_LOWER_SINK=
BOUND_LOWER_SINK_IDENTITY_SHA256=
BOUND_LOWER_SINK_FS_BLOCK_BYTES=
BOUND_LOWER_SINK_AVAILABLE_KIB=
declare -A INSTALL_MANIFEST_HASHES=()
DESTINATION=
STAGING=
SELECTOR_STAGING=
UNIT_STAGING=
ROLLBACK_UNIT_STAGING=
LEGACY_BACKUP_ROOT=
LEGACY_BACKUP=
LEGACY_BACKUP_STAGING=
ROLLBACK_SELECTOR_STAGING=
PRIOR_SELECTOR_TARGET=
PUBLISHED_DESTINATION=0
UNIT_CREATED=0
LEGACY_UNIT_REPLACED=0
LEGACY_UNIT_RELOAD_REQUIRED=0
declare -a AUXILIARY_UNIT_STAGING_PATHS=()
declare -a AUXILIARY_UNIT_CREATED_PATHS=()
declare -a AUXILIARY_UNIT_CREATED_SOURCES=()

refuse() {
  printf 'NBD_INSTALL_STATE=REFUSED\n'
  printf 'NBD_INSTALL_REASON=%s\n' "$1"
  exit 1
}

usage() {
  cat <<'EOF'
Usage:
  install-cascade-boot.sh [--plan]
  install-cascade-boot.sh --approve-nbd-product-install <version> \
    --lower-sink <safe-absolute-directory>
  install-cascade-boot.sh --approve-nbd-product-install <version> \\
    --approve-legacy-unit-replacement <sha256>

The default is a read-only plan. Approval must name exactly the sealed release
version in this bundle. An attended installation must bind one existing,
canonical non-symlink lower sink. This source-only slice installs its unit
disabled: a separate scoped lifecycle approval is required before it can
activate or deactivate a cascade.
EOF
}

read_release_version() {
  local version trailing
  [[ -f $SOURCE_RELEASE/RELEASE_VERSION && ! -L $SOURCE_RELEASE/RELEASE_VERSION ]] || refuse RELEASE_VERSION_MISSING
  IFS= read -r version <"$SOURCE_RELEASE/RELEASE_VERSION" || refuse RELEASE_VERSION_MISSING
  trailing=$(sed -n '2p' "$SOURCE_RELEASE/RELEASE_VERSION")
  [[ -z $trailing && $version =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || refuse RELEASE_VERSION_INVALID
  RELEASE_VERSION=$version
}

release_config_value() {
  local root=$1 key=$2
  awk -F= -v key="$key" '$0 !~ /^[[:space:]]*#/ && $1 == key { print $2; exit }' \
    "$root/scripts/safety/cascade.conf.example"
}

is_sha256() {
  [[ $1 =~ ^[[:xdigit:]]{64}$ ]]
}

is_source_commit() {
  [[ $1 =~ ^[[:xdigit:]]{40}$ ]]
}

verify_source_identity() {
  local root=$1 commit branch tree_state
  commit=$(tr -d '[:space:]' <"$root/SOURCE_COMMIT")
  branch=$(tr -d '[:space:]' <"$root/SOURCE_BRANCH")
  tree_state=$(tr -d '[:space:]' <"$root/SOURCE_TREE_STATE")
  is_source_commit "$commit" || refuse RELEASE_SOURCE_IDENTITY_INVALID
  [[ $branch =~ ^[A-Za-z0-9._/-]{1,200}$ && ( $tree_state == clean || $tree_state == dirty ) ]] \
    || refuse RELEASE_SOURCE_IDENTITY_INVALID
}

verify_generic_bundle_config() {
  local root=$1
  [[ $(release_config_value "$root" NBD_LOWER_SINK) == '' ]] || refuse GENERIC_BUNDLE_LOWER_SINK_BOUND
  [[ $(release_config_value "$root" NBD_LOWER_SINK_TYPE) == directory ]] || refuse GENERIC_BUNDLE_LOWER_SINK_INVALID
  [[ $(release_config_value "$root" NBD_LOWER_SINK_IDENTITY_SHA256) == '' ]] || refuse GENERIC_BUNDLE_LOWER_SINK_BOUND
  [[ $(release_config_value "$root" NBD_LOWER_SINK_FS_BLOCK_BYTES) == '' ]] || refuse GENERIC_BUNDLE_LOWER_SINK_BOUND
  [[ $(release_config_value "$root" NBD_LOWER_SINK_BINDING) == unbound ]] || refuse GENERIC_BUNDLE_LOWER_SINK_INVALID
}

inspect_lower_sink() {
  local sink=$1 canonical metadata fs_block identity df_records available
  [[ $sink == /* && $sink =~ ^/[A-Za-z0-9._/-]{1,480}$ ]] || refuse LOWER_SINK_INVALID
  [[ ! -L $sink && -d $sink ]] || refuse LOWER_SINK_INVALID
  canonical=$(readlink -f -- "$sink" 2>/dev/null || true)
  [[ $canonical == "$sink" && -d $canonical && ! -L $canonical ]] || refuse LOWER_SINK_INVALID
  metadata=$(stat -c '%d:%i:%u:%g:%a:%F' -- "$canonical" 2>/dev/null || true)
  [[ $metadata =~ ^[0-9]+:[0-9]+:[0-9]+:[0-9]+:[0-7]{3,4}:directory$ ]] || refuse LOWER_SINK_METADATA_INVALID
  fs_block=$(stat -fc '%s' -- "$canonical" 2>/dev/null || true)
  [[ $fs_block =~ ^[0-9]+$ && $fs_block -ge 512 && $fs_block -le 1048576 ]] || refuse LOWER_SINK_CAPACITY_METADATA_INVALID
  set +e
  df_records=$(df -Pk -- "$canonical" 2>/dev/null | awk 'NR > 1 && $4 ~ /^[0-9]+$/ && NF >= 6 { print $4 }')
  local df_status=$?
  set -e
  (( df_status == 0 )) || refuse LOWER_SINK_CAPACITY_METADATA_INVALID
  [[ $(printf '%s\n' "$df_records" | sed '/^$/d' | wc -l | tr -d '[:space:]') == 1 ]] || refuse LOWER_SINK_CAPACITY_METADATA_INVALID
  available=${df_records//$'\n'/}
  [[ $available =~ ^[0-9]+$ ]] || refuse LOWER_SINK_CAPACITY_METADATA_INVALID
  identity=$(printf '%s\0%s\0%s' "$canonical" directory "$metadata" | sha256sum | awk '{print $1}')

  BOUND_LOWER_SINK=$canonical
  BOUND_LOWER_SINK_IDENTITY_SHA256=$identity
  BOUND_LOWER_SINK_FS_BLOCK_BYTES=$fs_block
  BOUND_LOWER_SINK_AVAILABLE_KIB=$available
}

bind_staged_lower_sink() {
  local root=$1
  [[ -n $BOUND_LOWER_SINK && $BOUND_LOWER_SINK_IDENTITY_SHA256 =~ ^[[:xdigit:]]{64}$ ]] \
    || refuse LOWER_SINK_BINDING_MISSING
  python3 - "$root/scripts/safety/cascade.conf.example" "$BOUND_LOWER_SINK" \
    "$BOUND_LOWER_SINK_IDENTITY_SHA256" "$BOUND_LOWER_SINK_FS_BLOCK_BYTES" <<'PY'
import os
import sys

path, sink, identity, fs_block = sys.argv[1:]
values = {
    "NBD_LOWER_SINK": sink,
    "NBD_LOWER_SINK_TYPE": "directory",
    "NBD_LOWER_SINK_IDENTITY_SHA256": identity,
    "NBD_LOWER_SINK_FS_BLOCK_BYTES": fs_block,
    "NBD_LOWER_SINK_BINDING": "bound",
}
seen = set()
temporary = path + ".tmp"
with open(path, encoding="utf-8") as source, open(temporary, "w", encoding="utf-8") as target:
    for line in source:
        key, sep, _ = line.partition("=")
        if sep and key in values:
            target.write(key + "=" + values[key] + "\n")
            seen.add(key)
        else:
            target.write(line)
if seen != set(values):
    os.unlink(temporary)
    raise SystemExit("lower sink configuration template is incomplete")
os.replace(temporary, path)
PY
}

record_installed_provenance() {
  local root=$1
  python3 - "$root/INSTALL_PROVENANCE.json" "$INPUT_BUNDLE_MANIFEST_SHA256" \
    "$(tr -d '[:space:]' <"$root/SOURCE_COMMIT")" \
    "$(tr -d '[:space:]' <"$root/SOURCE_BRANCH")" \
    "$(tr -d '[:space:]' <"$root/SOURCE_TREE_STATE")" \
    "$BOUND_LOWER_SINK" "$BOUND_LOWER_SINK_IDENTITY_SHA256" \
    "$BOUND_LOWER_SINK_FS_BLOCK_BYTES" "$BOUND_LOWER_SINK_AVAILABLE_KIB" <<'PY'
import json
import os
import sys

out, input_digest, commit, branch, tree_state, sink, identity, fs_block, available = sys.argv[1:]
record = {
    "schema_version": "ramshared-installed-release-provenance/v1",
    "input_bundle_manifest_sha256": input_digest,
    "source_commit": commit,
    "source_branch": branch,
    "source_tree_state": tree_state,
    "lower_sink": {
        "canonical_path": sink,
        "identity_sha256": identity,
        "filesystem_block_bytes": int(fs_block),
        "available_kib_at_bind": int(available),
    },
}
temporary = out + ".tmp"
with open(temporary, "w", encoding="utf-8") as target:
    json.dump(record, target, sort_keys=True, separators=(",", ":"))
    target.write("\n")
os.replace(temporary, out)
PY
}

regenerate_installed_manifest() {
  local root=$1
  (
    cd "$root"
    find . -type f ! -name SHA256SUMS ! -name INSTALLED_MANIFEST_SHA256 -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
  )
  chmod 0644 "$root/SHA256SUMS"
  INSTALLED_MANIFEST_SHA256=$(sha256sum -- "$root/SHA256SUMS" | awk '{print $1}')
}

record_installed_manifest_receipt() {
  local root=$1
  is_sha256 "$INSTALLED_MANIFEST_SHA256" || refuse INSTALLED_MANIFEST_DIGEST_INVALID
  printf '%s\n' "$INSTALLED_MANIFEST_SHA256" >"$root/INSTALLED_MANIFEST_SHA256"
  chmod 0644 "$root/INSTALLED_MANIFEST_SHA256"
}

verify_installed_provenance() {
  local root=$1 values input_digest commit branch tree_state sink identity fs_block available
  [[ -n ${INSTALL_MANIFEST_HASHES[INSTALL_PROVENANCE.json]+x} ]] || refuse INSTALL_PROVENANCE_UNMANIFESTED
  [[ -n ${INSTALL_MANIFEST_HASHES[INPUT_BUNDLE_SHA256SUMS]+x} ]] || refuse INPUT_BUNDLE_MANIFEST_UNMANIFESTED
  [[ -f $root/INSTALL_PROVENANCE.json && ! -L $root/INSTALL_PROVENANCE.json ]] || refuse INSTALL_PROVENANCE_MISSING
  [[ -f $root/INPUT_BUNDLE_SHA256SUMS && ! -L $root/INPUT_BUNDLE_SHA256SUMS ]] || refuse INPUT_BUNDLE_MANIFEST_MISSING

  values=$(python3 - "$root/INSTALL_PROVENANCE.json" <<'PY'
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
    raise SystemExit(f"invalid installed provenance: {exc}")
PY
  ) || refuse INSTALL_PROVENANCE_INVALID
  IFS=$'\t' read -r input_digest commit branch tree_state sink identity fs_block available <<<"$values"
  [[ -n $input_digest && -n $commit && -n $branch && -n $tree_state && -n $sink && -n $identity ]] \
    || refuse INSTALL_PROVENANCE_INVALID
  [[ $(sha256sum -- "$root/INPUT_BUNDLE_SHA256SUMS" | awk '{print $1}') == "$input_digest" ]] \
    || refuse INPUT_BUNDLE_MANIFEST_DIGEST_MISMATCH
  [[ $(tr -d '[:space:]' <"$root/SOURCE_COMMIT") == "$commit" \
    && $(tr -d '[:space:]' <"$root/SOURCE_BRANCH") == "$branch" \
    && $(tr -d '[:space:]' <"$root/SOURCE_TREE_STATE") == "$tree_state" ]] \
    || refuse INSTALL_PROVENANCE_SOURCE_IDENTITY_MISMATCH
  [[ $(release_config_value "$root" NBD_LOWER_SINK) == "$sink" \
    && $(release_config_value "$root" NBD_LOWER_SINK_TYPE) == directory \
    && $(release_config_value "$root" NBD_LOWER_SINK_IDENTITY_SHA256) == "$identity" \
    && $(release_config_value "$root" NBD_LOWER_SINK_FS_BLOCK_BYTES) == "$fs_block" \
    && $(release_config_value "$root" NBD_LOWER_SINK_BINDING) == bound ]] \
    || refuse INSTALL_PROVENANCE_BINDING_MISMATCH
}

verify_release_tree() {
  local root=$1 kind=${2:-generic} manifest line digest relative actual listed required receipt
  [[ -f $root/SHA256SUMS && ! -L $root/SHA256SUMS ]] || refuse RELEASE_MANIFEST_MISSING
  [[ -z $(find "$root" -type l -print -quit) ]] || refuse RELEASE_SYMLINK_FORBIDDEN
  [[ -z $(find "$root" \( -type p -o -type b -o -type c -o -type s \) -print -quit) ]] || refuse RELEASE_NON_REGULAR_OBJECT
  manifest="$root/SHA256SUMS"
  INSTALL_MANIFEST_HASHES=()
  while IFS= read -r line || [[ -n $line ]]; do
    [[ $line =~ ^([[:xdigit:]]{64})\ \ \./([A-Za-z0-9][A-Za-z0-9._/-]*)$ ]] || refuse RELEASE_MANIFEST_FORMAT_INVALID
    digest=${BASH_REMATCH[1],,}
    relative=${BASH_REMATCH[2]}
    [[ $relative != SHA256SUMS && $relative != ../* && $relative != */../* && $relative != *'/..' && $relative != *'//' && $relative != *'/./'* ]] || refuse RELEASE_MANIFEST_PATH_INVALID
    [[ -z ${INSTALL_MANIFEST_HASHES[$relative]+x} ]] || refuse RELEASE_MANIFEST_DUPLICATE
    [[ -f $root/$relative && ! -L $root/$relative ]] || refuse RELEASE_MANIFEST_ENTRY_MISSING
    actual=$(sha256sum -- "$root/$relative" | awk '{print $1}')
    [[ $actual == "$digest" ]] || refuse RELEASE_MANIFEST_HASH_MISMATCH
    INSTALL_MANIFEST_HASHES[$relative]=$digest
  done <"$manifest"
  [[ ${#INSTALL_MANIFEST_HASHES[@]} -gt 0 ]] || refuse RELEASE_MANIFEST_EMPTY
  while IFS= read -r -d '' listed; do
    relative=${listed#"$root"/}
    [[ -n ${INSTALL_MANIFEST_HASHES[$relative]+x} ]] || refuse RELEASE_MANIFEST_INCOMPLETE
  done < <(find "$root" -type f ! -name SHA256SUMS ! -name INSTALLED_MANIFEST_SHA256 -print0)
  for required in \
    bin/ramshared \
    bin/ramsharedd \
    scripts/safety/install-cascade-boot.sh \
    scripts/safety/uninstall-cascade-boot.sh \
    scripts/safety/nbd-product-preflight.sh \
    scripts/safety/nbd-benchmark-cell.sh \
    scripts/safety/nbd-benchmark-cgroup-launch.sh \
    scripts/safety/nbd-benchmark-lib.sh \
    scripts/safety/cascade_pressure_integrity_worker.py \
    scripts/safety/cascade-up.sh \
    scripts/safety/cascade-down.sh \
    scripts/safety/cascade-controller.sh \
    scripts/safety/provision-origin-swap.sh \
    scripts/safety/lifecycle-recovery-status.sh \
    scripts/safety/cascade-health.sh \
    scripts/safety/wsl-relay-health.sh \
    scripts/safety/manage-control-plane.sh \
    scripts/safety/ramshared-host-gate.sh \
    scripts/safety/ramshared-session-launcher.sh \
    scripts/safety/docker-daemon-ramshared.json \
    scripts/safety/cascade.conf.example \
    SOURCE_COMMIT \
    SOURCE_BRANCH \
    SOURCE_TREE_STATE \
    systemd/ramshared-cascade.service \
    systemd/ramshared-cascade-health.service \
    systemd/ramshared-workloads.slice \
    systemd/ramshared-control.slice \
    systemd/ramshared-workloads-docker.slice \
    systemd/ramshared-workloads-cron.slice \
    systemd/ramshared-host-gate.service \
    systemd/ramshared-supervisor.service \
    systemd/ramshared-cron-workload.service.in \
    systemd/docker.service.d/10-ramshared-control.conf \
    systemd/containerd.service.d/10-ramshared-control.conf \
    systemd/cron.service.d/10-ramshared-control.conf; do
    [[ -f $root/$required && ! -L $root/$required ]] || refuse RELEASE_LAYOUT_INVALID
  done
  [[ -x $root/bin/ramshared && -x $root/bin/ramsharedd ]] || refuse RELEASE_LAYOUT_INVALID
  (cd "$root" && sha256sum -c --status SHA256SUMS) || refuse RELEASE_MANIFEST_HASH_MISMATCH
  verify_source_identity "$root"
  if [[ $kind == generic ]]; then
    verify_generic_bundle_config "$root"
    [[ ! -e $root/INSTALL_PROVENANCE.json && ! -e $root/INSTALLED_MANIFEST_SHA256 ]] || refuse GENERIC_BUNDLE_DERIVATION_PRESENT
  elif [[ $kind == installed ]]; then
    [[ -f $root/INSTALL_PROVENANCE.json && ! -L $root/INSTALL_PROVENANCE.json ]] || refuse INSTALL_PROVENANCE_MISSING
    [[ -f $root/INPUT_BUNDLE_SHA256SUMS && ! -L $root/INPUT_BUNDLE_SHA256SUMS ]] || refuse INPUT_BUNDLE_MANIFEST_MISSING
    [[ -f $root/INSTALLED_MANIFEST_SHA256 && ! -L $root/INSTALLED_MANIFEST_SHA256 ]] || refuse INSTALLED_MANIFEST_RECEIPT_MISSING
    receipt=$(tr -d '[:space:]' <"$root/INSTALLED_MANIFEST_SHA256")
    [[ $receipt == "$(sha256sum -- "$root/SHA256SUMS" | awk '{print $1}')" ]] || refuse INSTALLED_MANIFEST_RECEIPT_MISMATCH
    is_sha256 "$receipt" || refuse INSTALLED_MANIFEST_RECEIPT_MISMATCH
    verify_installed_provenance "$root"
  else
    refuse RELEASE_KIND_INVALID
  fi
}

require_sealed_node() {
  local path=$1 metadata mode
  metadata=$(stat -c '%u:%g:%a' -- "$path" 2>/dev/null || true)
  [[ $metadata =~ ^0:0:([0-7]{3,4})$ ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
  mode=${BASH_REMATCH[1]}
  (( (8#$mode & 0222) == 0 )) || refuse RELEASE_SEAL_OWNER_MODE_INVALID

  if [[ -d $path ]]; then
    [[ $mode == 555 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
  elif [[ -f $path ]]; then
    if [[ -x $path ]]; then
      [[ $mode == 555 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
    else
      [[ $mode == 444 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
    fi
  else
    refuse RELEASE_NON_REGULAR_OBJECT
  fi
}

verify_sealed_release_tree() {
  local root=$1 path
  while IFS= read -r -d '' path; do
    require_sealed_node "$path"
  done < <(find "$root" -type d -print0)
  while IFS= read -r -d '' path; do
    require_sealed_node "$path"
  done < <(find "$root" -type f -print0)
}

systemctl_status() {
  local operation=$1 unit=$2 status
  set +e
  systemctl "$operation" --quiet "$unit" >/dev/null 2>&1
  status=$?
  set -e
  printf '%s\n' "$status"
}

is_sha256() {
  [[ $1 =~ ^[[:xdigit:]]{64}$ ]]
}

check_unit_inert() {
  local unit=$1 active enabled
  active=$(systemctl_status is-active "$unit")
  case $active in
    3|4) ;;
    0) refuse PRODUCT_UNIT_ACTIVE ;;
    *) refuse PRODUCT_UNIT_ACTIVITY_UNKNOWN ;;
  esac
  enabled=$(systemctl_status is-enabled "$unit")
  case $enabled in
    1) ;;
    0) refuse PRODUCT_UNIT_ENABLED ;;
    *) refuse PRODUCT_UNIT_ENABLEMENT_UNKNOWN ;;
  esac
}

check_existing_unit_file() {
  local expected_unit=${1:-"$SOURCE_RELEASE/systemd/ramshared-cascade.service"}
  [[ ! -L $UNIT_PATH ]] || refuse PRODUCT_UNIT_CONFLICT
  if [[ -e $UNIT_PATH ]]; then
    [[ -f $UNIT_PATH ]] || refuse PRODUCT_UNIT_CONFLICT
    if ! cmp -s "$expected_unit" "$UNIT_PATH"; then
      [[ -n $LEGACY_UNIT_APPROVED_HASH ]] || refuse LEGACY_UNIT_APPROVAL_MISSING
      is_sha256 "$LEGACY_UNIT_APPROVED_HASH" || refuse LEGACY_UNIT_APPROVAL_INVALID
      [[ $(stat -c '%u:%g:%a' -- "$UNIT_PATH" 2>/dev/null || true) == '0:0:644' ]] || refuse LEGACY_UNIT_METADATA_INVALID
      [[ $(sha256sum -- "$UNIT_PATH" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_HASH_MISMATCH
    fi
  fi
}

legacy_unit_replacement_required() {
  [[ -f $UNIT_PATH && ! -L $UNIT_PATH ]] && ! cmp -s "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_PATH"
}

verify_legacy_backup_or_refuse() {
  [[ -n $LEGACY_BACKUP && -f $LEGACY_BACKUP && ! -L $LEGACY_BACKUP ]] || refuse LEGACY_UNIT_BACKUP_CONFLICT
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP" 2>/dev/null || true) == '0:0:444' ]] || refuse LEGACY_UNIT_BACKUP_CONFLICT
  [[ $(sha256sum -- "$LEGACY_BACKUP" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_BACKUP_HASH_MISMATCH
}

prepare_legacy_backup_root_or_refuse() {
  if [[ -e $LEGACY_BACKUP_ROOT || -L $LEGACY_BACKUP_ROOT ]]; then
    [[ -d $LEGACY_BACKUP_ROOT && ! -L $LEGACY_BACKUP_ROOT ]] || refuse LEGACY_BACKUP_ROOT_INVALID
    [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP_ROOT" 2>/dev/null || true) == '0:0:755' ]] || refuse LEGACY_BACKUP_ROOT_INVALID
  else
    install -d -m 0755 "$LEGACY_BACKUP_ROOT"
  fi
  [[ -d $LEGACY_BACKUP_ROOT && ! -L $LEGACY_BACKUP_ROOT ]] || refuse LEGACY_BACKUP_ROOT_INVALID
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP_ROOT" 2>/dev/null || true) == '0:0:755' ]] || refuse LEGACY_BACKUP_ROOT_INVALID
}

backup_legacy_unit_if_required() {
  local observed_hash
  legacy_unit_replacement_required || return 0
  observed_hash=$(sha256sum -- "$UNIT_PATH" | awk '{print $1}')
  [[ $observed_hash == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_HASH_MISMATCH
  [[ $(stat -c '%u:%g:%a' -- "$UNIT_PATH" 2>/dev/null || true) == '0:0:644' ]] || refuse LEGACY_UNIT_METADATA_INVALID

  LEGACY_BACKUP_ROOT="$PRODUCT_ROOT/legacy-units"
  LEGACY_BACKUP="$LEGACY_BACKUP_ROOT/ramshared-cascade.service.$observed_hash.bak"
  LEGACY_BACKUP_STAGING="$LEGACY_BACKUP_ROOT/.ramshared-cascade.service.$observed_hash.$$.staging"
  prepare_legacy_backup_root_or_refuse
  if [[ -e $LEGACY_BACKUP || -L $LEGACY_BACKUP ]]; then
    verify_legacy_backup_or_refuse
    return 0
  fi

  install -m 0444 "$UNIT_PATH" "$LEGACY_BACKUP_STAGING"
  chown root:root "$LEGACY_BACKUP_STAGING"
  [[ $(sha256sum -- "$LEGACY_BACKUP_STAGING" | awk '{print $1}') == "$observed_hash" ]] || refuse LEGACY_UNIT_BACKUP_HASH_MISMATCH
  mv -T "$LEGACY_BACKUP_STAGING" "$LEGACY_BACKUP"
  verify_legacy_backup_or_refuse
  # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-backed-up
}

path_exists_or_link() {
  [[ -e $1 || -L $1 ]]
}

capture_prior_selector() {
  local target resolved version
  if [[ -L $CURRENT_SELECTOR ]]; then
    target=$(readlink -- "$CURRENT_SELECTOR" 2>/dev/null || true)
    [[ $target =~ ^releases/[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || refuse CURRENT_SELECTOR_INVALID
    version=${target#releases/}
    resolved=$(readlink -f -- "$CURRENT_SELECTOR" 2>/dev/null || true)
    [[ $resolved == "$RELEASE_ROOT/$version" && -d $resolved ]] || refuse CURRENT_SELECTOR_INVALID
    PRIOR_SELECTOR_TARGET=$target
  elif [[ -e $CURRENT_SELECTOR ]]; then
    refuse CURRENT_SELECTOR_CONFLICT
  fi
}

selector_points_to_destination() {
  local resolved
  [[ -n $DESTINATION && -L $CURRENT_SELECTOR ]] || return 1
  resolved=$(readlink -f -- "$CURRENT_SELECTOR" 2>/dev/null || true)
  [[ $resolved == "$DESTINATION" ]]
}

remove_path_if_present() {
  local path=$1
  [[ -n $path ]] || return 0
  path_exists_or_link "$path" || return 0
  rm -rf -- "$path"
}

restore_prior_selector() {
  if [[ -n $PRIOR_SELECTOR_TARGET ]]; then
    path_exists_or_link "$ROLLBACK_SELECTOR_STAGING" && return 1
    ln -s "$PRIOR_SELECTOR_TARGET" "$ROLLBACK_SELECTOR_STAGING" || return 1
    chown -h root:root "$ROLLBACK_SELECTOR_STAGING" || return 1
    mv -Tf "$ROLLBACK_SELECTOR_STAGING" "$CURRENT_SELECTOR" || return 1
    return 0
  fi

  rm -f -- "$CURRENT_SELECTOR"
}

remove_created_unit_if_owned() {
  (( UNIT_CREATED )) || return 0
  if [[ -f $UNIT_PATH ]] && cmp -s "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_PATH"; then
    rm -f -- "$UNIT_PATH"
  fi
  UNIT_CREATED=0
}

remove_created_auxiliary_units_if_owned() {
  local index target expected staging
  for staging in "${AUXILIARY_UNIT_STAGING_PATHS[@]}"; do
    remove_path_if_present "$staging"
  done
  for index in "${!AUXILIARY_UNIT_CREATED_PATHS[@]}"; do
    target=${AUXILIARY_UNIT_CREATED_PATHS[$index]}
    expected=${AUXILIARY_UNIT_CREATED_SOURCES[$index]}
    if [[ -f $target && ! -L $target ]] && cmp -s "$expected" "$target"; then
      rm -f -- "$target"
    else
      printf 'NBD_INSTALL_ROLLBACK=AUXILIARY_UNIT_REMOVE_FAILED target=%s\n' "$target" >&2
    fi
  done
  AUXILIARY_UNIT_STAGING_PATHS=()
  AUXILIARY_UNIT_CREATED_PATHS=()
  AUXILIARY_UNIT_CREATED_SOURCES=()
}

restore_legacy_unit_if_replaced() {
  (( LEGACY_UNIT_REPLACED )) || return 0
  [[ -n $LEGACY_BACKUP && -f $LEGACY_BACKUP && ! -L $LEGACY_BACKUP ]] || return 1
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP" 2>/dev/null || true) == '0:0:444' ]] || return 1
  [[ $(sha256sum -- "$LEGACY_BACKUP" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || return 1
  ROLLBACK_UNIT_STAGING="$(dirname -- "$UNIT_PATH")/.ramshared-cascade.rollback.$$"
  install -m 0644 "$LEGACY_BACKUP" "$ROLLBACK_UNIT_STAGING" || return 1
  chown root:root "$ROLLBACK_UNIT_STAGING" || return 1
  [[ $(sha256sum -- "$ROLLBACK_UNIT_STAGING" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || return 1
  mv -Tf "$ROLLBACK_UNIT_STAGING" "$UNIT_PATH" || return 1
  if (( LEGACY_UNIT_RELOAD_REQUIRED )); then
    systemctl daemon-reload || return 1
    LEGACY_UNIT_RELOAD_REQUIRED=0
  fi
  LEGACY_UNIT_REPLACED=0
}

rollback_after_failure() {
  local status=$?
  trap - EXIT
  if (( status != 0 )); then
    set +e
    remove_path_if_present "$STAGING"
    remove_path_if_present "$SELECTOR_STAGING"
    if selector_points_to_destination; then
      restore_prior_selector || printf 'NBD_INSTALL_ROLLBACK=SELECTOR_RESTORE_FAILED\n' >&2
    fi
    restore_legacy_unit_if_replaced || printf 'NBD_INSTALL_ROLLBACK=LEGACY_UNIT_RESTORE_FAILED\n' >&2
    remove_created_auxiliary_units_if_owned
    remove_created_unit_if_owned
    remove_path_if_present "$UNIT_STAGING"
    remove_path_if_present "$ROLLBACK_UNIT_STAGING"
    remove_path_if_present "$LEGACY_BACKUP_STAGING"
    if (( PUBLISHED_DESTINATION )) && ! selector_points_to_destination; then
      remove_path_if_present "$DESTINATION"
      PUBLISHED_DESTINATION=0
    fi
    remove_path_if_present "$ROLLBACK_SELECTOR_STAGING"
  fi
  exit "$status"
}

install_unit_if_absent() {
  if path_exists_or_link "$UNIT_PATH"; then
    if ! legacy_unit_replacement_required; then
      check_existing_unit_file "$DESTINATION/systemd/ramshared-cascade.service"
      return
    fi
    backup_legacy_unit_if_required
    verify_legacy_backup_or_refuse
    path_exists_or_link "$UNIT_STAGING" && refuse INSTALL_STAGING_EXISTS
    install -m 0644 "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_STAGING"
    # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-staged
    chown root:root "$UNIT_STAGING"
    mv -Tf "$UNIT_STAGING" "$UNIT_PATH"
    LEGACY_UNIT_REPLACED=1
    # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-replaced
    return
  fi

  path_exists_or_link "$UNIT_STAGING" && refuse INSTALL_STAGING_EXISTS
  install -m 0644 "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_STAGING"
  # NBD_INSTALL_POST_WRITE_PHASE=unit-staged
  ln "$UNIT_STAGING" "$UNIT_PATH" || refuse PRODUCT_UNIT_CONFLICT
  UNIT_CREATED=1
  # NBD_INSTALL_POST_WRITE_PHASE=unit-linked
  rm -f -- "$UNIT_STAGING"
  # NBD_INSTALL_POST_WRITE_PHASE=unit-staging-removed
}

check_auxiliary_unit_file() {
  local expected=$1 target=$2
  [[ -f $expected && ! -L $expected ]] || refuse RELEASE_LAYOUT_INVALID
  [[ ! -L $target ]] || refuse AUXILIARY_UNIT_CONFLICT
  if [[ -e $target ]]; then
    [[ -f $target ]] || refuse AUXILIARY_UNIT_CONFLICT
    [[ $(stat -c '%u:%g:%a' -- "$target" 2>/dev/null || true) == '0:0:644' ]] \
      || refuse AUXILIARY_UNIT_METADATA_INVALID
    cmp -s "$expected" "$target" || refuse AUXILIARY_UNIT_CONFLICT
  fi
}

install_auxiliary_unit_if_absent() {
  local expected=$1 target=$2 label=$3 staging target_dir
  check_auxiliary_unit_file "$expected" "$target"
  path_exists_or_link "$target" && return 0
  target_dir=$(dirname -- "$target")
  [[ -d $target_dir && ! -L $target_dir ]] || refuse AUXILIARY_UNIT_DIRECTORY_INVALID
  staging="$target_dir/.${label}.${RELEASE_VERSION}.$$"
  path_exists_or_link "$staging" && refuse INSTALL_STAGING_EXISTS
  AUXILIARY_UNIT_STAGING_PATHS+=("$staging")
  install -m 0644 "$expected" "$staging"
  chown root:root "$staging"
  ln "$staging" "$target" || refuse AUXILIARY_UNIT_CONFLICT
  AUXILIARY_UNIT_CREATED_PATHS+=("$target")
  AUXILIARY_UNIT_CREATED_SOURCES+=("$expected")
  rm -f -- "$staging"
}

while (($# > 0)); do
  case "$1" in
    --plan)
      shift
      ;;
    --approve-nbd-product-install)
      (($# >= 2)) || refuse APPROVAL_SCOPE_MISSING
      APPROVED_VERSION=$2
      shift 2
      ;;
    --lower-sink)
      (($# >= 2)) || refuse LOWER_SINK_APPROVAL_REQUIRED
      LOWER_SINK=$2
      shift 2
      ;;
    --approve-legacy-unit-replacement)
      (($# >= 2)) || refuse LEGACY_UNIT_APPROVAL_MISSING
      LEGACY_UNIT_APPROVED_HASH=$2
      shift 2
      ;;
    --enable)
      refuse BOOT_ENABLE_REQUIRES_LIFECYCLE_APPROVAL
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) refuse UNSUPPORTED_ARGUMENT ;;
  esac
done

read_release_version
verify_release_tree "$SOURCE_RELEASE" generic

if [[ -z $APPROVED_VERSION ]]; then
  printf 'NBD_INSTALL_STATE=PLAN\n'
  printf 'NBD_INSTALL_RELEASE=%s\n' "$RELEASE_VERSION"
  printf 'NBD_INSTALL_TARGET=%s\n' "$RELEASE_ROOT/$RELEASE_VERSION"
  printf 'NBD_INSTALL_SELECTOR=%s/current\n' "$PRODUCT_ROOT"
  exit 0
fi

[[ $APPROVED_VERSION == "$RELEASE_VERSION" ]] || refuse APPROVAL_SCOPE_INVALID
[[ -n $LOWER_SINK ]] || refuse LOWER_SINK_APPROVAL_REQUIRED
[[ -z $LEGACY_UNIT_APPROVED_HASH || $LEGACY_UNIT_APPROVED_HASH =~ ^[[:xdigit:]]{64}$ ]] || refuse LEGACY_UNIT_APPROVAL_INVALID
inspect_lower_sink "$LOWER_SINK"
[[ $(id -u) -eq 0 ]] || refuse ROOT_REQUIRED
command -v systemctl >/dev/null 2>&1 || refuse SYSTEMD_UNAVAILABLE
check_unit_inert ramshared-cascade.service
check_unit_inert ramsharedd.service
check_existing_unit_file
capture_prior_selector

DESTINATION="$RELEASE_ROOT/$RELEASE_VERSION"
[[ ! -e $DESTINATION && ! -L $DESTINATION ]] || refuse RELEASE_VERSION_EXISTS
STAGING="$RELEASE_ROOT/.${RELEASE_VERSION}.staging.$$"
SELECTOR_STAGING="$PRODUCT_ROOT/.current.${RELEASE_VERSION}.$$"
UNIT_STAGING="$(dirname -- "$UNIT_PATH")/.ramshared-cascade.${RELEASE_VERSION}.$$"
ROLLBACK_SELECTOR_STAGING="$PRODUCT_ROOT/.rollback-current.${RELEASE_VERSION}.$$"
[[ ! -e $STAGING && ! -L $STAGING && ! -e $SELECTOR_STAGING && ! -L $SELECTOR_STAGING && ! -e $ROLLBACK_SELECTOR_STAGING && ! -L $ROLLBACK_SELECTOR_STAGING ]] || refuse INSTALL_STAGING_EXISTS

trap rollback_after_failure EXIT
install -d -m 0755 "$PRODUCT_ROOT" "$RELEASE_ROOT"
# NBD_INSTALL_POST_WRITE_PHASE=release-roots-prepared

umask 022
install -d -m 0755 "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-directory-created
cp -a "$SOURCE_RELEASE/." "$STAGING/"
# NBD_INSTALL_POST_WRITE_PHASE=release-copied
INPUT_BUNDLE_MANIFEST_SHA256=$(sha256sum -- "$SOURCE_RELEASE/SHA256SUMS" | awk '{print $1}')
is_sha256 "$INPUT_BUNDLE_MANIFEST_SHA256" || refuse INPUT_BUNDLE_MANIFEST_DIGEST_INVALID
install -m 0644 "$SOURCE_RELEASE/SHA256SUMS" "$STAGING/INPUT_BUNDLE_SHA256SUMS"
# NBD_INSTALL_POST_WRITE_PHASE=input-bundle-manifest-copied
inspect_lower_sink "$LOWER_SINK"
bind_staged_lower_sink "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=lower-sink-bound
record_installed_provenance "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=installed-provenance-recorded
regenerate_installed_manifest "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=installed-manifest-regenerated
record_installed_manifest_receipt "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=installed-manifest-receipt-recorded
chown -R root:root "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-owner-normalized
find "$STAGING" -type d -exec chmod 0555 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-directories-sealed
find "$STAGING" -type f -perm /111 -exec chmod 0555 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-executables-sealed
find "$STAGING" -type f ! -perm /111 -exec chmod 0444 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-files-sealed
verify_release_tree "$STAGING" installed
# NBD_INSTALL_POST_WRITE_PHASE=staging-manifest-verified
verify_sealed_release_tree "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-seal-verified

# Rename only a new, verified version directory. Existing sealed versions are
# never rewritten, and the selector is the only object replaced atomically.
mv -T "$STAGING" "$DESTINATION"
PUBLISHED_DESTINATION=1
# NBD_INSTALL_POST_WRITE_PHASE=destination-published
install_unit_if_absent
install_auxiliary_unit_if_absent \
  "$DESTINATION/systemd/ramshared-cascade-health.service" \
  "$HEALTH_UNIT_PATH" \
  ramshared-cascade-health
# NBD_INSTALL_POST_WRITE_PHASE=health-unit-installed
install_auxiliary_unit_if_absent \
  "$DESTINATION/systemd/ramshared-workloads.slice" \
  "$WORKLOADS_SLICE_PATH" \
  ramshared-workloads
# NBD_INSTALL_POST_WRITE_PHASE=workloads-slice-installed
ln -s "releases/$RELEASE_VERSION" "$SELECTOR_STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=selector-staged
chown -h root:root "$SELECTOR_STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=selector-owner-normalized
mv -Tf "$SELECTOR_STAGING" "$CURRENT_SELECTOR"
# NBD_INSTALL_POST_WRITE_PHASE=selector-published
if (( LEGACY_UNIT_REPLACED )); then
  LEGACY_UNIT_RELOAD_REQUIRED=1
fi
systemctl daemon-reload
# NBD_INSTALL_POST_WRITE_PHASE=daemon-reloaded

trap - EXIT
printf 'NBD_INSTALL_STATE=INSTALLED\n'
printf 'NBD_INSTALL_RELEASE=%s\n' "$RELEASE_VERSION"
printf 'NBD_INSTALL_SELECTOR=%s/current\n' "$PRODUCT_ROOT"
printf 'NBD_INSTALL_ENABLED=0\n'
