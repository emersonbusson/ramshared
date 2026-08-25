#!/usr/bin/env bash
# Attended, one-time swap-signature provisioning for an already attached
# RamShared NBD origin. Normal `ramshared up` never calls mkswap.
set -euo pipefail

manifest=/etc/ramshared/origin.conf
host_manifest=/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json
execute=0

refuse() {
  printf 'RAMSHARED_ORIGIN_PROVISION=REFUSED\n'
  printf 'RAMSHARED_ORIGIN_PROVISION_REASON=%s\n' "$1"
  exit 1
}

manifest_value() {
  local key=$1
  awk -F= -v key="$key" '$1 == key { count++; value=$2 } END { if (count == 1 && value != "") print value; else exit 1 }' "$manifest"
}

case $# in
  0) ;;
  1) [[ $1 == --execute ]] || refuse UNSUPPORTED_ARGUMENT; execute=1 ;;
  *) refuse UNSUPPORTED_ARGUMENT ;;
esac

[[ -f $manifest && ! -L $manifest ]] || refuse SEALED_MANIFEST_UNAVAILABLE
[[ $(stat -c '%u:%a' -- "$manifest") == 0:600 ]] || refuse SEALED_MANIFEST_MODE_INVALID
[[ $(stat -c '%s' -- "$manifest") -le 65536 && $(wc -l <"$manifest") -eq 12 ]] \
  || refuse SEALED_MANIFEST_LAYOUT_INVALID
[[ $(manifest_value schema_version) == 3 ]] || refuse SEALED_MANIFEST_SCHEMA_INVALID
host_manifest_sha256=$(manifest_value host_manifest_sha256) || refuse SEALED_HOST_HASH_MISSING
configuration_sha256=$(manifest_value configuration_sha256) || refuse SEALED_CONFIG_HASH_MISSING
[[ $host_manifest_sha256 =~ ^[0-9a-f]{64}$ && $configuration_sha256 =~ ^[0-9a-f]{64}$ ]] \
  || refuse SEALED_HASH_INVALID
expected_uuid=$(manifest_value expected_swap_uuid) || refuse SEALED_SWAP_UUID_MISSING
[[ $expected_uuid =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ ]] || refuse SEALED_SWAP_UUID_INVALID
logical_mib=$(manifest_value logical_capacity_mib) || refuse SEALED_CAPACITY_MISSING
[[ $logical_mib =~ ^[0-9]+$ && $logical_mib -ge 1024 && $logical_mib -le 24576 ]] || refuse SEALED_CAPACITY_INVALID
device=$(manifest_value origin_path) || refuse SEALED_ORIGIN_PATH_MISSING
partuuid=$(manifest_value partuuid) || refuse SEALED_PARTUUID_MISSING
ptuuid=$(manifest_value ptuuid) || refuse SEALED_PTUUID_MISSING
partition_dev_t=$(manifest_value partition_dev_t) || refuse SEALED_PARTITION_DEV_T_MISSING
parent_dev_t=$(manifest_value parent_dev_t) || refuse SEALED_PARENT_DEV_T_MISSING
swap_type=$(manifest_value swap_type) || refuse SEALED_SWAP_TYPE_MISSING
physical_cache_mib=$(manifest_value physical_cache_cap_mib) || refuse SEALED_CACHE_CAP_MISSING
[[ $partuuid =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ \
  && $ptuuid =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ ]] \
  || refuse SEALED_DISK_IDENTITY_INVALID
[[ $partition_dev_t =~ ^[0-9]+:[0-9]+$ && $parent_dev_t =~ ^[0-9]+:[0-9]+$ ]] \
  || refuse SEALED_DEV_T_INVALID
[[ $swap_type == swap ]] || refuse SEALED_SWAP_TYPE_INVALID
[[ $physical_cache_mib =~ ^[0-9]+$ && $physical_cache_mib -ge 1 \
  && $physical_cache_mib -le $logical_mib ]] || refuse SEALED_CACHE_CAP_INVALID
[[ $device == "/dev/disk/by-partuuid/$partuuid" ]] || refuse SEALED_ORIGIN_PATH_MISMATCH

if (( execute == 0 )); then
  printf 'RAMSHARED_ORIGIN_PROVISION=PLAN\n'
  printf 'RAMSHARED_ORIGIN_PROVISION_DEVICE=%s\n' "$device"
  printf 'RAMSHARED_ORIGIN_PROVISION_UUID=%s\n' "$expected_uuid"
  printf 'RAMSHARED_ORIGIN_PROVISION_MUTATION=none\n'
  exit 0
fi

(( EUID == 0 )) || refuse ROOT_REQUIRED
approval="provision:${expected_uuid}:${device}"
[[ ${RAMSHARED_ORIGIN_PROVISION_APPROVAL:-} == "$approval" ]] || refuse EXACT_APPROVAL_REQUIRED
[[ -b $device ]] || refuse ORIGIN_DEVICE_INVALID
resolved=$(readlink -f -- "$device") || refuse ORIGIN_RESOLUTION_FAILED
[[ $(blkid -s PARTUUID -o value "$resolved" | tr '[:upper:]' '[:lower:]') == "$partuuid" ]] || refuse PARTUUID_REVALIDATION_FAILED
partition_hex=$(stat -c '%t:%T' -- "$resolved") || refuse PARTITION_DEV_T_UNAVAILABLE
partition_actual="$((16#${partition_hex%%:*})):$((16#${partition_hex##*:}))"
[[ $partition_actual == "$partition_dev_t" ]] || refuse PARTITION_DEV_T_MISMATCH
name=${resolved##*/}
sysfs=$(readlink -f -- "/sys/class/block/$name") || refuse PARTITION_SYSFS_UNAVAILABLE
[[ -f $sysfs/partition ]] || refuse ORIGIN_IS_NOT_A_PARTITION
parent_name=$(basename -- "$(dirname -- "$sysfs")")
parent="/dev/$parent_name"
parent_hex=$(stat -c '%t:%T' -- "$parent") || refuse PARENT_DEV_T_UNAVAILABLE
parent_actual="$((16#${parent_hex%%:*})):$((16#${parent_hex##*:}))"
[[ $parent_actual == "$parent_dev_t" ]] || refuse PARENT_DEV_T_MISMATCH
[[ $(blkid -s PTUUID -o value "$parent" | tr '[:upper:]' '[:lower:]') == "$ptuuid" ]] || refuse PTUUID_REVALIDATION_FAILED

device_dev_t() {
  local path=$1 hex
  [[ -b $path ]] || return 1
  hex=$(stat -c '%t:%T' -- "$path") || return 1
  printf '%s:%s\n' "$((16#${hex%%:*}))" "$((16#${hex##*:}))"
}

device_parent_dev_t() {
  local path=$1 canonical block_sysfs block_parent
  canonical=$(readlink -f -- "$path") || return 1
  block_sysfs=$(readlink -f -- "/sys/class/block/${canonical##*/}") || return 1
  if [[ -f $block_sysfs/partition ]]; then
    block_parent="/dev/$(basename -- "$(dirname -- "$block_sysfs")")"
    device_dev_t "$block_parent"
  else
    device_dev_t "$canonical"
  fi
}

strict_swap_sources() {
  [[ -r /proc/swaps && ! -L /proc/swaps ]] || return 1
  awk '
    NR == 1 {
      if (NF != 5 || $1 != "Filename" || $2 != "Type" || $3 != "Size" ||
          $4 != "Used" || $5 != "Priority") exit 2
      header = 1
      next
    }
    {
      if (NF != 5 || $2 !~ /^(file|partition)$/ || $3 !~ /^[0-9]+$/ ||
          $4 !~ /^[0-9]+$/ || $5 !~ /^-?[0-9]+$/ || seen[$1]++) exit 2
      print $1
    }
    END { if (!header) exit 2 }
  ' /proc/swaps
}

prove_origin_not_critical() {
  local root_source swap_sources critical critical_resolved critical_dev_t critical_parent_dev_t
  root_source=$(findmnt -n -o SOURCE -T /) || return 1
  swap_sources=$(strict_swap_sources) || return 1
  while IFS= read -r critical; do
    [[ -n $critical ]] || continue
    critical=${critical%\[deleted\]}
    [[ $critical == /dev/* ]] || continue
    critical_resolved=$(readlink -f -- "$critical") || return 1
    [[ -b $critical_resolved ]] || return 1
    critical_dev_t=$(device_dev_t "$critical_resolved") || return 1
    critical_parent_dev_t=$(device_parent_dev_t "$critical_resolved") || return 1
    [[ $critical_dev_t != "$partition_dev_t" && $critical_dev_t != "$parent_dev_t" \
      && $critical_parent_dev_t != "$parent_dev_t" ]] || return 1
  done <<<"$root_source"$'\n'"$swap_sources"
}

verify_host_manifest_hashes() {
  [[ -f $host_manifest && ! -L $host_manifest ]] || return 1
  [[ $(stat -c '%s' -- "$host_manifest") -gt 0 \
    && $(stat -c '%s' -- "$host_manifest") -le 65536 ]] || return 1
  local actual_host_hash
  actual_host_hash=$(sha256sum -- "$host_manifest") || return 1
  actual_host_hash=${actual_host_hash%% *}
  [[ $actual_host_hash == "$host_manifest_sha256" ]] || return 1
  command -v python3 >/dev/null 2>&1 || return 1
  python3 - "$host_manifest" "$configuration_sha256" "$partuuid" "$ptuuid" \
    "$expected_uuid" "$logical_mib" "$physical_cache_mib" <<'PY'
import hashlib
import json
import re
import sys

path, sealed_hash, partuuid, ptuuid, swap_uuid, logical, physical = sys.argv[1:]

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result

with open(path, "r", encoding="utf-8-sig") as stream:
    manifest = json.load(stream, object_pairs_hook=unique_object)

expected_keys = {
    "schema_version", "origin_vhdx", "fixed_size_bytes", "logical_capacity_mib",
    "physical_cache_cap_mib", "chunk_mib", "gpu_reserve_min_mib",
    "gpu_reserve_percent", "partuuid", "disk_guid", "expected_swap_uuid",
    "ownership_proof_schema", "existing_wsl_swap_vhdx", "configuration_sha256",
}
if not isinstance(manifest, dict) or set(manifest) != expected_keys:
    raise ValueError("schema")
for field in (
    "fixed_size_bytes", "logical_capacity_mib", "physical_cache_cap_mib", "chunk_mib",
    "gpu_reserve_min_mib", "gpu_reserve_percent", "ownership_proof_schema",
):
    if isinstance(manifest[field], bool) or not isinstance(manifest[field], int):
        raise ValueError("numeric type")
for field in (
    "origin_vhdx", "partuuid", "disk_guid", "expected_swap_uuid",
    "existing_wsl_swap_vhdx", "configuration_sha256",
):
    if not isinstance(manifest[field], str):
        raise ValueError("string type")
if manifest["schema_version"] != 3 or manifest["ownership_proof_schema"] != 1:
    raise ValueError("version")
if not re.fullmatch(r"[A-Za-z]:[\\/].+", manifest["origin_vhdx"]):
    raise ValueError("origin path")
if not re.fullmatch(r"[A-Za-z]:[\\/].+", manifest["existing_wsl_swap_vhdx"]):
    raise ValueError("swap path")
if manifest["partuuid"].lower() != partuuid or manifest["disk_guid"].lower() != ptuuid:
    raise ValueError("disk identity")
if manifest["expected_swap_uuid"].lower() != swap_uuid:
    raise ValueError("swap identity")
if manifest["logical_capacity_mib"] != int(logical) or manifest["physical_cache_cap_mib"] != int(physical):
    raise ValueError("capacity")
canonical = (
    f"schema=3\n"
    f"origin_vhdx={manifest['origin_vhdx']}\n"
    f"fixed_size_bytes={manifest['fixed_size_bytes']}\n"
    f"logical_capacity_mib={manifest['logical_capacity_mib']}\n"
    f"physical_cache_cap_mib={manifest['physical_cache_cap_mib']}\n"
    f"chunk_mib={manifest['chunk_mib']}\n"
    f"gpu_reserve_min_mib={manifest['gpu_reserve_min_mib']}\n"
    f"gpu_reserve_percent={manifest['gpu_reserve_percent']}\n"
    f"partuuid={manifest['partuuid']}\n"
    f"disk_guid={manifest['disk_guid']}\n"
    f"expected_swap_uuid={manifest['expected_swap_uuid']}\n"
    f"ownership_proof_schema={manifest['ownership_proof_schema']}\n"
    f"existing_wsl_swap_vhdx={manifest['existing_wsl_swap_vhdx']}\n"
)
actual = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
if actual != manifest["configuration_sha256"] or actual != sealed_hash:
    raise ValueError("configuration hash")
PY
}

verify_host_manifest_hashes || refuse HOST_MANIFEST_HASH_MISMATCH

exec {origin_fd}<>"$resolved" || refuse ORIGIN_HANDLE_OPEN_FAILED
flock -x -n "$origin_fd" || refuse ORIGIN_EXCLUSIVE_LOCK_UNAVAILABLE
origin_handle="/proc/$$/fd/$origin_fd"
exec {parent_fd}<"$parent" || refuse PARENT_HANDLE_OPEN_FAILED
parent_handle="/proc/$$/fd/$parent_fd"

prove_origin_handle_identity() {
  local observed_sysfs
  [[ -b $origin_handle && -b $parent_handle ]] || return 1
  [[ $(device_dev_t "$origin_handle") == "$partition_dev_t" ]] || return 1
  [[ $(device_dev_t "$parent_handle") == "$parent_dev_t" ]] || return 1
  observed_sysfs=$(readlink -f -- "/sys/dev/block/$partition_dev_t") || return 1
  [[ $observed_sysfs == "$sysfs" ]] || return 1
  [[ $(device_parent_dev_t "$origin_handle") == "$parent_dev_t" ]] || return 1
  [[ $(blkid -s PARTUUID -o value "$origin_handle" | tr '[:upper:]' '[:lower:]') == "$partuuid" ]] || return 1
  [[ $(blkid -s PTUUID -o value "$parent_handle" | tr '[:upper:]' '[:lower:]') == "$ptuuid" ]] || return 1
}

prove_origin_handle_identity || refuse ORIGIN_HANDLE_IDENTITY_MISMATCH

prove_origin_not_critical || refuse ORIGIN_ALIASES_ROOT_OR_ACTIVE_SWAP

awk -v wanted="$partition_dev_t" '$3 == wanted { found=1 } END { exit(found ? 0 : 1) }' \
  /proc/self/mountinfo && refuse ORIGIN_IS_MOUNTED

actual_bytes=$(blockdev --getsize64 "$origin_handle") || refuse ORIGIN_CAPACITY_UNAVAILABLE
expected_bytes=$((logical_mib * 1024 * 1024))
(( actual_bytes >= expected_bytes )) || refuse ORIGIN_CAPACITY_MISMATCH
existing_type=$(blkid -s TYPE -o value "$origin_handle" 2>/dev/null || true)
existing_uuid=$(blkid -s UUID -o value "$origin_handle" 2>/dev/null || true)
if [[ $existing_type == swap && ${existing_uuid,,} == "$expected_uuid" ]]; then
  printf 'RAMSHARED_ORIGIN_PROVISION=ALREADY_PROVISIONED\n'
  exit 0
fi
[[ -z $existing_type && -z $existing_uuid ]] || refuse FOREIGN_SIGNATURE_PRESENT

prove_origin_not_critical || refuse PREWRITE_ACTIVE_SWAP_REVALIDATION_FAILED
awk -v wanted="$partition_dev_t" '$3 == wanted { found=1 } END { exit(found ? 0 : 1) }' \
  /proc/self/mountinfo && refuse PREWRITE_MOUNT_REVALIDATION_FAILED
prove_origin_handle_identity || refuse PREWRITE_HANDLE_IDENTITY_MISMATCH
/sbin/mkswap -L RAMSHARED -U "$expected_uuid" -- "$origin_handle"
prove_origin_handle_identity || refuse POSTWRITE_HANDLE_IDENTITY_MISMATCH
[[ $(blkid -s TYPE -o value "$origin_handle") == swap ]] || refuse POSTWRITE_TYPE_MISMATCH
[[ $(blkid -s UUID -o value "$origin_handle" | tr '[:upper:]' '[:lower:]') == "$expected_uuid" ]] || refuse POSTWRITE_UUID_MISMATCH
printf 'RAMSHARED_ORIGIN_PROVISION=PROVISIONED\n'
