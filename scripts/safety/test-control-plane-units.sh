#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
systemd_dir="$root/scripts/safety/systemd"

require_file() {
  [[ -f $1 && ! -L $1 ]] || { printf 'missing regular file: %s\n' "$1" >&2; exit 1; }
}

require_text() {
  local path=$1 text=$2
  grep -Fqx -- "$text" "$path" || { printf 'missing %s in %s\n' "$text" "$path" >&2; exit 1; }
}

control="$systemd_dir/ramshared-control.slice"
workloads="$systemd_dir/ramshared-workloads.slice"
supervisor="$systemd_dir/ramshared-supervisor.service"
cascade="$systemd_dir/ramshared-cascade.service"
controller="$root/scripts/safety/cascade-controller.sh"
recovery_status="$root/scripts/safety/lifecycle-recovery-status.sh"
origin_provision="$root/scripts/safety/provision-origin-swap.sh"
health="$systemd_dir/ramshared-cascade-health.service"
daemon="$systemd_dir/ramsharedd.service"
docker_dropin="$systemd_dir/docker.service.d/10-ramshared-control.conf"
cron_template="$systemd_dir/ramshared-cron-workload.service.in"
cron_dropin="$systemd_dir/cron.service.d/10-ramshared-control.conf"
docker_slice="$systemd_dir/ramshared-workloads-docker.slice"
cron_slice="$systemd_dir/ramshared-workloads-cron.slice"
docker_config="$root/scripts/safety/docker-daemon-ramshared.json"
host_gate_unit="$systemd_dir/ramshared-host-gate.service"
host_gate_script="$root/scripts/safety/ramshared-host-gate.sh"

for file in "$control" "$workloads" "$supervisor" "$cascade" "$controller" "$recovery_status" "$origin_provision" "$health" "$daemon" "$docker_dropin" "$cron_template" "$cron_dropin" "$docker_slice" "$cron_slice" "$docker_config" "$host_gate_unit" "$host_gate_script"; do
  require_file "$file"
done

for file in "$docker_dropin" "$cron_dropin" "$systemd_dir/containerd.service.d/10-ramshared-control.conf"; do
  grep -Fq 'RamShared staging: inert until explicit attended activation transaction.' "$file" || {
    printf 'ordinary service staging is not inert: %s\n' "$file" >&2; exit 1;
  }
  if grep -Eq '^(Requires|Wants|After|Before|ConditionPathExists|Slice)=' "$file"; then
    printf 'ordinary service staging may alter ambient dependency behavior: %s\n' "$file" >&2; exit 1
  fi
done

for text in '[Slice]' 'MemoryLow=1G' 'MemoryMin=512M' 'CPUWeight=1000' 'IOWeight=1000' 'ManagedOOMPreference=avoid'; do
  require_text "$control" "$text"
done
awk '
  /^\[Unit\]$/ { section="unit"; next }
  /^\[Slice\]$/ { section="slice"; next }
  /^ManagedOOMPreference=avoid$/ { found++; if (section != "slice") exit 1 }
  END { exit(found == 1 ? 0 : 1) }
' "$control" || {
  printf 'ManagedOOMPreference=avoid must appear exactly once in [Slice]\n' >&2; exit 1;
}
if grep -Fqx 'ManagedOOMMemoryPressure=kill' "$control"; then
  printf 'control slice must not be an oomd kill candidate\n' >&2; exit 1
fi
for text in '[Slice]' 'CPUWeight=50' 'IOWeight=50' 'TasksMax=8192' 'ManagedOOMMemoryPressure=kill' 'ManagedOOMMemoryPressureLimit=10%'; do
  require_text "$workloads" "$text"
done
grep -Fq 'MemTotal-bound drop-in' "$workloads" || {
  printf 'workload slice lacks the guest-size-aware staging contract\n' >&2; exit 1;
}
if grep -Eq '^Memory(High|Max)=[0-9]+$' "$workloads"; then
  printf 'workload slice must not hardcode a guest-size-specific memory limit\n' >&2; exit 1
fi
require_text "$supervisor" 'Slice=ramshared-control.slice'
require_text "$supervisor" 'ExecStart=/opt/ramshared/current/bin/ramshared supervise'
for file in "$cascade" "$health" "$daemon"; do
  require_text "$file" 'Slice=ramshared-control.slice'
done
for text in \
  'Type=simple' \
  'ExecStart=/opt/ramshared/current/scripts/safety/cascade-controller.sh --execute' \
  'KillMode=process' \
  'SendSIGKILL=no' \
  'TimeoutStopSec=infinity'; do
  require_text "$cascade" "$text"
done
if grep -Eq '^ExecStop=' "$cascade" || grep -Eq '^TimeoutStopSec=[0-9]' "$cascade"; then
  printf 'cascade unit may terminate the backend before clean shutdown proof\n' >&2; exit 1
fi
for text in \
  'RAMSHARED_NBD_CONTROLLER_APPROVAL' \
  'recover:$version' \
  'RAMSHARED_NBD_LIFECYCLE_APPROVAL="deactivate:$version"' \
  '/mnt/c/ProgramData/RamShared/lifecycle-recovery' \
  'while ! RAMSHARED_NBD_LIFECYCLE_APPROVAL=' \
  'while ! clean_shutdown_proven' \
  'strict_managed_swaps_absent || return 1' \
  'rm -f -- "$marker"'; do
  grep -Fq "$text" "$controller" || {
    printf 'cascade controller lacks durable swapoff-before-stop contract: %s\n' "$text" >&2; exit 1;
  }
done
if grep -Fq "! awk" "$controller"; then
  printf 'cascade controller retains ambiguous negated awk swap proof\n' >&2; exit 1
fi
for text in \
  'LIFECYCLE_RECOVERY_STATE=CLEAN' \
  'LIFECYCLE_RECOVERY_STATE=PENDING' \
  'LIFECYCLE_RECOVERY_MANAGED_SWAP_COUNT' \
  'LIFECYCLE_RECOVERY_DEVICE_ATTACHED' \
  'RAMSHARED_LIFECYCLE_TEST_ROOT'; do
  grep -Fq "$text" "$recovery_status" || {
    printf 'host recovery status lacks read-only terminal proof: %s\n' "$text" >&2; exit 1;
  }
done
for text in \
  'RAMSHARED_ORIGIN_PROVISION_APPROVAL' \
  'SEALED_MANIFEST_LAYOUT_INVALID' \
  'HOST_MANIFEST_HASH_MISMATCH' \
  'sha256sum -- "$host_manifest"' \
  'configuration_sha256' \
  'prove_origin_not_critical' \
  'ORIGIN_ALIASES_ROOT_OR_ACTIVE_SWAP' \
  'flock -x -n "$origin_fd"' \
  'origin_handle="/proc/$$/fd/$origin_fd"' \
  'PREWRITE_HANDLE_IDENTITY_MISMATCH' \
  'POSTWRITE_HANDLE_IDENTITY_MISMATCH' \
  '/sbin/mkswap -L RAMSHARED -U "$expected_uuid" -- "$origin_handle"'; do
  grep -Fq "$text" "$origin_provision" || {
    printf 'origin provisioner lacks exact destructive identity gate: %s\n' "$text" >&2; exit 1;
  }
done
if grep -Eq '/sbin/mkswap .*\$resolved' "$origin_provision"; then
  printf 'origin provisioner can still retarget mkswap through a mutable pathname\n' >&2; exit 1
fi
printf '%s\n' 'PASS provisioner_mkswap_is_fd_bound_and_never_executed_by_tests'
printf '%s\n' 'PASS cascade_controller_owns_stop_until_swapoff_and_detach_are_proven'
printf '%s\n' 'PASS backend_lifecycle_has_no_pre_swapoff_kill_path'
require_text "$daemon" 'RefuseManualStart=yes'
require_text "$daemon" 'ExecStart=/usr/bin/false'
if grep -Eq '^(ExecStartPre|ExecStart)=.*(preflight|ramsharedd|ublk|ramshared up)' "$daemon" ||
  grep -Eq '^WantedBy=' "$daemon"; then
  printf 'legacy daemon unit may still activate a device or boot lifecycle\n' >&2; exit 1
fi
printf '%s\n' 'PASS legacy_ramsharedd_unit_is_staging_refused_and_cannot_activate'
grep -Fq -- '--interval-ms 1000' "$health" || {
  printf 'guest heartbeat is not sampled every second\n' >&2; exit 1;
}
require_text "$cron_template" 'Slice=ramshared-workloads-cron.slice'
grep -Fq '"cgroup-parent": "ramshared-workloads-docker.slice"' "$docker_config" || {
  printf 'Docker cgroup parent is not sealed to its workload child slice\n' >&2; exit 1;
}
grep -Fq 'ConditionPathExists=!/var/lib/ramshared/safe-mode.json' "$cron_template" || {
  printf 'cron template lacks safe-mode gate\n' >&2; exit 1;
}
for file in "$cron_template"; do
  grep -Fq 'ConditionPathExists=/run/ramshared/host-resume-lease.json' "$file" || {
    printf 'heavy unit lacks boot-bound resume lease gate: %s\n' "$file" >&2; exit 1;
  }
done
require_text "$host_gate_unit" 'Slice=ramshared-control.slice'
grep -Fq '/mnt/c/ProgramData/RamShared/safe-mode' "$host_gate_script" || {
  printf 'guest host-gate importer lacks durable Windows gate path\n' >&2; exit 1;
}
grep -Fq 'Windows safe-mode authority is unavailable' "$host_gate_script" || {
  printf 'guest host-gate importer fails open when Windows authority is unavailable\n' >&2; exit 1;
}
for text in \
  '/mnt/c/ProgramData/RamShared/guardian-state' \
  'RAMSHARED_HOST_GATE_TEST_ROOT' \
  'guardian_policy' \
  'config["schema_version"] != 2' \
  'health["schema_version"] != 3' \
  'guardian health proof is unavailable' \
  'guardian health proof is stale' \
  'guardian health proof policy mismatch' \
  'guardian health proof identity mismatch' \
  'guardian health proof is not healthy'; do
  grep -Fq "$text" "$host_gate_script" || {
    printf 'guest host-gate importer lacks fail-closed guardian proof: %s\n' "$text" >&2; exit 1;
  }
done
for text in 'health["schema_version"] != 3' 'health["boot_id"] != boot_id' 'PTUUID' 'identity.returncode != 0' 'resolved_parent_device' 'origin_identity_fixture' 'origin GPT disk GUID does not match'; do
  grep -Fq "$text" "$host_gate_script" || {
    printf 'guest host-gate importer lacks boot/GPT identity binding: %s\n' "$text" >&2; exit 1;
  }
done
if grep -Fq 'source":"host_gate_absent' "$host_gate_script"; then
  printf 'guest host-gate importer may not mint a lease merely because a safe-mode gate is absent\n' >&2; exit 1
fi
grep -Fq 'orphan guest safe-mode marker requires ramshared recover --resume' "$host_gate_script" || {
  printf 'guest host-gate importer clears safe mode outside recover --resume\n' >&2; exit 1;
}
for text in 'ramshared-origin-manifest.json' 'configuration_sha256' '/etc/ramshared/origin.conf' '/dev/disk/by-partuuid/'; do
  grep -Fq "$text" "$host_gate_script" || {
    printf 'guest host-gate importer lacks sealed origin contract: %s\n' "$text" >&2; exit 1;
  }
done

guardian_fixture=$(mktemp -d)
trap 'rm -rf -- "$guardian_fixture"' EXIT
mkdir -p "$guardian_fixture/windows/safe-mode" "$guardian_fixture/windows/guardian-state"
guardian_health="$guardian_fixture/windows/guardian-state/Ubuntu-24.04.health.json"
guardian_config="$guardian_fixture/windows/guardian-config.json"
guardian_lease="$guardian_fixture/run/ramshared/host-resume-lease.json"
origin_config="$guardian_fixture/etc/ramshared/origin.conf"

write_guardian_fixture() {
  local health_sid=$1 config_sid=$2 config_artifact_root=$3
  local current_boot_id
  current_boot_id=$(tr -d '[:space:]' </proc/sys/kernel/random/boot_id)
  python3 - "$guardian_health" "$guardian_config" "$health_sid" "$config_sid" "$config_artifact_root" "$current_boot_id" <<'PY'
import datetime as dt
import json
import sys

health_path, config_path, health_sid, config_sid, config_artifact_root, boot_id = sys.argv[1:]
policy = {
    "heartbeat_path": r"C:\manufactured\heartbeat.json",
    "artifact_root": r"C:\manufactured\artifacts",
    "stale_after_seconds": 17,
    "poll_seconds": 3,
    "guest_command_timeout_seconds": 7,
}
health = {
    "schema_version": 3,
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
    "distro": "Ubuntu-24.04",
    "user_sid": health_sid,
    "state": "HEALTHY",
    "reason": "watching",
    "boot_id": boot_id,
    "guardian_policy": policy,
}
config_policy = dict(policy)
config_policy["artifact_root"] = config_artifact_root
config = {
    "schema_version": 2,
    "distro": "Ubuntu-24.04",
    "user_sid": config_sid,
    "task_name": "RamSharedWslGuardian.v1",
    "guardian_policy": config_policy,
}
for path, value in ((health_path, health), (config_path, config)):
    with open(path, "w", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True, separators=(",", ":"))
        stream.write("\n")
PY
}

write_origin_identity_fixture() {
  local disk_guid=$1
  python3 - "$guardian_fixture/windows/ramshared-origin-manifest.json" \
    "$guardian_fixture/windows/origin-identity.json" "$disk_guid" <<'PY'
import hashlib
import json
import sys

manifest_path, identity_path, disk_guid = sys.argv[1:]
partuuid = "11111111-2222-4333-8444-555555555555"
expected_swap_uuid = "99999999-8888-4777-8666-555555555555"
configuration = (
    "schema=3\norigin_vhdx=I:\\RamShared\\ramshared-origin.vhdx\n"
    f"fixed_size_bytes={25 * 1024**3}\nlogical_capacity_mib=2048\nphysical_cache_cap_mib=1024\n"
    "chunk_mib=128\ngpu_reserve_min_mib=2048\ngpu_reserve_percent=20\n"
    f"partuuid={partuuid}\ndisk_guid={disk_guid}\nexpected_swap_uuid={expected_swap_uuid}\n"
    "ownership_proof_schema=1\n"
    "existing_wsl_swap_vhdx=I:\\wsl_swap\\swap.vhdx\n"
)
manifest = {
    "schema_version": 3,
    "origin_vhdx": r"I:\RamShared\ramshared-origin.vhdx",
    "fixed_size_bytes": 25 * 1024**3,
    "logical_capacity_mib": 2048,
    "physical_cache_cap_mib": 1024,
    "chunk_mib": 128,
    "gpu_reserve_min_mib": 2048,
    "gpu_reserve_percent": 20,
    "partuuid": partuuid,
    "disk_guid": disk_guid,
    "expected_swap_uuid": expected_swap_uuid,
    "ownership_proof_schema": 1,
    "existing_wsl_swap_vhdx": r"I:\wsl_swap\swap.vhdx",
    "configuration_sha256": hashlib.sha256(configuration.encode()).hexdigest(),
}
identity = {
    "partuuid": partuuid,
    "resolved_partition": "/dev/nbd0p1",
    "resolved_parent_device": "/dev/nbd0",
    "ptuuid": disk_guid,
    "partition_dev_t": "43:1",
    "parent_dev_t": "43:0",
    "swap_uuid": "",
    "swap_type": "",
    "blkid_returncode": 0,
    "critical_dev_ts": ["8:0", "8:1"],
}
for path, record in ((manifest_path, manifest), (identity_path, identity)):
    with open(path, "w", encoding="utf-8") as stream:
        json.dump(record, stream, sort_keys=True, separators=(",", ":"))
        stream.write("\n")
PY
}

write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-100' 'C:\manufactured\artifacts'
write_origin_identity_fixture 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'

python3 - "$guardian_fixture/windows/origin-identity.json" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as stream:
    record = json.load(stream)
record["blkid_returncode"] = 1
with open(sys.argv[1], "w", encoding="utf-8") as stream:
    json.dump(record, stream, sort_keys=True, separators=(",", ":"))
    stream.write("\n")
PY
rm -f -- "$guardian_lease" "$origin_config"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'nonzero parent-disk blkid unexpectedly minted authority\n' >&2; exit 1
fi
[[ ! -e $guardian_lease && ! -e $origin_config ]] || {
  printf 'nonzero parent-disk blkid retained origin authority\n' >&2; exit 1;
}
write_origin_identity_fixture 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null
[[ -f $guardian_lease && ! -L $guardian_lease ]] || {
  printf 'fresh exact schema-v2 guardian proof did not mint a lease\n' >&2; exit 1;
}
python3 - "$guardian_lease" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    lease = json.load(stream)
if lease.get("schema_version") != 1 or lease.get("source") != "fresh_sealed_guardian_proof":
    raise SystemExit("lease is not bound to the sealed guardian proof")
with open("/proc/sys/kernel/random/boot_id", encoding="utf-8") as stream:
    boot_id = stream.read().strip()
if lease.get("boot_id") != boot_id:
    raise SystemExit("lease is not bound to this boot")
PY
grep -Fqx 'origin_path=/dev/disk/by-partuuid/11111111-2222-4333-8444-555555555555' "$origin_config" || {
  printf 'origin fixture did not bind the selected partition path\n' >&2; exit 1;
}
grep -Fqx 'ptuuid=aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee' "$origin_config" || {
  printf 'origin fixture did not bind GPT identity to resolved parent disk\n' >&2; exit 1;
}
grep -Fqx 'parent_dev_t=43:0' "$origin_config" || {
  printf 'origin fixture did not bind parent dev_t identity\n' >&2; exit 1;
}

# R4-HOST-04: even an otherwise valid origin may not be published before a
# later safe-mode gate is checked. The actual host-gate execution must leave
# both the lease and origin authority absent.
rm -f -- "$guardian_lease" "$origin_config"
cat >"$guardian_fixture/windows/safe-mode/Ubuntu-24.04.json" <<'EOF'
{"incident_id":"0123456789abcdef0123456789abcdef","distro":"Ubuntu-24.04"}
EOF
RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null
[[ ! -e $guardian_lease && ! -e $origin_config ]] || {
  printf 'safe-mode rejection retained origin authority minted before all gates\n' >&2; exit 1;
}
rm -f -- "$guardian_fixture/windows/safe-mode/Ubuntu-24.04.json" "$guardian_fixture/var/lib/ramshared/safe-mode.json"
write_origin_identity_fixture 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'

# A later guardian-proof rejection has the same authority rule as safe mode:
# a candidate origin must be discarded before the host-gate returns failure.
write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-101' 'C:\manufactured\artifacts'
rm -f -- "$guardian_lease" "$origin_config"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'invalid later guardian proof unexpectedly accepted origin authority\n' >&2; exit 1
fi
[[ ! -e $guardian_lease && ! -e $origin_config ]] || {
  printf 'later guardian rejection retained origin authority minted before all gates\n' >&2; exit 1;
}
write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-100' 'C:\manufactured\artifacts'

python3 - "$guardian_fixture/windows/origin-identity.json" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as stream:
    record = json.load(stream)
record["resolved_parent_device"] = record["resolved_partition"]
with open(sys.argv[1], "w", encoding="utf-8") as stream:
    json.dump(record, stream, sort_keys=True, separators=(",", ":"))
    stream.write("\n")
PY
rm -f -- "$guardian_lease" "$origin_config"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'partition-path GPT identity fixture unexpectedly minted authority\n' >&2; exit 1
fi
[[ ! -e $guardian_lease && ! -e $origin_config ]] || {
  printf 'rejected partition-path identity retained origin authority\n' >&2; exit 1;
}
write_origin_identity_fixture 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'

python3 - "$guardian_health" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    health = json.load(stream)
health.pop("boot_id")
with open(sys.argv[1], "w", encoding="utf-8") as stream:
    json.dump(health, stream, sort_keys=True, separators=(",", ":"))
    stream.write("\n")
PY
rm -f -- "$guardian_lease"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'bootless guardian proof unexpectedly minted a lease\n' >&2; exit 1
fi
[[ ! -e $guardian_lease ]] || { printf 'bootless guardian proof left a lease\n' >&2; exit 1; }

write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-100' 'C:\manufactured\artifacts'
python3 - "$guardian_health" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    health = json.load(stream)
health["boot_id"] = "00000000-0000-0000-0000-000000000000"
with open(sys.argv[1], "w", encoding="utf-8") as stream:
    json.dump(health, stream, sort_keys=True, separators=(",", ":"))
    stream.write("\n")
PY
rm -f -- "$guardian_lease"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'foreign boot guardian proof unexpectedly minted a lease\n' >&2; exit 1
fi
[[ ! -e $guardian_lease ]] || { printf 'foreign boot guardian proof left a lease\n' >&2; exit 1; }

python3 - "$guardian_health" <<'PY'
import datetime as dt
import json
import sys

with open(sys.argv[1], encoding="utf-8") as stream:
    health = json.load(stream)
health["timestamp_utc"] = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(seconds=61)).isoformat().replace("+00:00", "Z")
with open(sys.argv[1], "w", encoding="utf-8") as stream:
    json.dump(health, stream, sort_keys=True, separators=(",", ":"))
    stream.write("\n")
PY
rm -f -- "$guardian_lease"
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'stale guardian proof unexpectedly minted a lease\n' >&2; exit 1
fi
[[ ! -e $guardian_lease ]] || { printf 'stale guardian proof left a lease\n' >&2; exit 1; }

write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-101' 'C:\manufactured\artifacts'
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'foreign guardian SID unexpectedly minted a lease\n' >&2; exit 1
fi
[[ ! -e $guardian_lease ]] || { printf 'foreign guardian SID left a lease\n' >&2; exit 1; }

write_guardian_fixture 'S-1-5-21-100' 'S-1-5-21-100' 'C:\manufactured\other-artifacts'
if RAMSHARED_HOST_GATE_TEST_ROOT="$guardian_fixture" bash "$host_gate_script" >/dev/null 2>&1; then
  printf 'guardian policy mismatch unexpectedly minted a lease\n' >&2; exit 1
fi
[[ ! -e $guardian_lease ]] || { printf 'guardian policy mismatch left a lease\n' >&2; exit 1; }

recovery_fixture="$guardian_fixture/recovery"
mkdir -p "$recovery_fixture/proc" "$recovery_fixture/run/ramshared" \
  "$recovery_fixture/sys/class/block/nbd7" \
  "$recovery_fixture/windows/lifecycle-recovery"
printf 'Filename Type Size Used Priority\n' >"$recovery_fixture/proc/swaps"
recovery_output=$(RAMSHARED_LIFECYCLE_TEST_ROOT="$recovery_fixture" \
  RAMSHARED_WSL_DISTRO=Manufactured-Ubuntu bash "$recovery_status")
grep -Fqx 'LIFECYCLE_RECOVERY_STATE=CLEAN' <<<"$recovery_output" || {
  printf 'empty lifecycle recovery fixture was not clean\n' >&2; exit 1;
}
cat >"$recovery_fixture/windows/lifecycle-recovery/Manufactured-Ubuntu.pending" <<'EOF'
schema_version=1
distro=Manufactured-Ubuntu
release_version=manufactured-v1
boot_id=11111111-2222-4333-8444-555555555555
phase=stopping
managed_device=/dev/nbd7
EOF
printf '/dev/nbd7 partition 1048572 128 -2\n' >>"$recovery_fixture/proc/swaps"
printf '/dev/nbd7\n' >"$recovery_fixture/run/ramshared/swap-dev"
printf '4242\n' >"$recovery_fixture/run/ramshared/ramsharedd.pid"
mkdir -p "$recovery_fixture/proc/4242"
printf '4242 (ramsharedd) S 1 1 1 0 -1 0\n' >"$recovery_fixture/proc/4242/stat"
printf '4242\n' >"$recovery_fixture/sys/class/block/nbd7/pid"
if recovery_output=$(RAMSHARED_LIFECYCLE_TEST_ROOT="$recovery_fixture" \
  RAMSHARED_WSL_DISTRO=Manufactured-Ubuntu bash "$recovery_status"); then
  printf 'live manufactured lifecycle residue was reported clean\n' >&2; exit 1
fi
grep -Fqx 'LIFECYCLE_RECOVERY_STATE=PENDING' <<<"$recovery_output" || {
  printf 'live manufactured lifecycle residue was not pending\n' >&2; exit 1;
}
for expected in \
  'LIFECYCLE_RECOVERY_MANAGED_SWAP_COUNT=1' \
  'LIFECYCLE_RECOVERY_DAEMON_RUNNING=1' \
  'LIFECYCLE_RECOVERY_DEVICE_ATTACHED=1'; do
  grep -Fqx "$expected" <<<"$recovery_output" || {
    printf 'lifecycle recovery fixture omitted proof: %s\n' "$expected" >&2; exit 1;
  }
done
rm -f -- "$recovery_fixture/windows/lifecycle-recovery/Manufactured-Ubuntu.pending" \
  "$recovery_fixture/run/ramshared/swap-dev" "$recovery_fixture/run/ramshared/ramsharedd.pid" \
  "$recovery_fixture/proc/4242/stat" "$recovery_fixture/sys/class/block/nbd7/pid"
printf 'Filename Type Size Used Priority\n' >"$recovery_fixture/proc/swaps"
recovery_output=$(RAMSHARED_LIFECYCLE_TEST_ROOT="$recovery_fixture" \
  RAMSHARED_WSL_DISTRO=Manufactured-Ubuntu bash "$recovery_status")
grep -Fqx 'LIFECYCLE_RECOVERY_STATE=CLEAN' <<<"$recovery_output" || {
  printf 'cleared lifecycle recovery fixture was not replay-clean\n' >&2; exit 1;
}

printf 'PASS control_and_workload_limits_are_sealed\n'
printf 'PASS malformed_missing_or_stale_guardian_proof_never_issues_resume_lease\n'
printf 'PASS fresh_schema_v2_guardian_proof_mints_boot_bound_lease\n'
printf 'PASS stale_foreign_or_policy_mismatched_guardian_proof_never_leases\n'
printf 'PASS bootless_or_foreign_boot_guardian_proof_never_leases\n'
printf 'PASS origin_gpt_identity_binds_resolved_parent_disk_not_partition\n'
printf 'PASS origin_parent_blkid_nonzero_is_refused_even_when_stdout_matches\n'
printf 'PASS origin_authority_waits_for_safe_mode_and_guardian_gates\n'
printf 'PASS lifecycle_recovery_requires_marker_swap_daemon_and_detach_terminal_proof\n'
