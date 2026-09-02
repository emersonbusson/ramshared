# Shared read-only probes and strict artifact parsing for wsl-kernel.sh.
# shellcheck shell=bash

# shellcheck disable=SC2034
WSL_KERNEL_LIB_LOADED=1

MIN_BZIMAGE_BYTES="${MIN_BZIMAGE_BYTES:-1048576}"
ENABLE_TIMEOUT_SEC="${ENABLE_TIMEOUT_SEC:-30}"
INTEROP_FAIL_SEC="${INTEROP_FAIL_SEC:-15}"
APPLY_TIMEOUT_SEC="${APPLY_TIMEOUT_SEC:-60}"
KERNEL_CANARY_DISTRO="${KERNEL_CANARY_DISTRO:-Ubuntu-24.04}"
APPROVED_WSL_FAILED_UNITS="${APPROVED_WSL_FAILED_UNITS:-}"
KERNEL_PAIR_MANIFEST="${KERNEL_PAIR_MANIFEST:-}"
PROMOTION_RECEIPT_WSL="${PROMOTION_RECEIPT_WSL:-/mnt/c/wsl/ramshared-receipts/promotion-current.json}"

readonly E_OK=0
readonly E_ACTION=2
readonly E_INTEROP=3
readonly E_APPLY=4
readonly E_USAGE=5

PAIR_ERROR=""
RECEIPT_ERROR=""
declare -Ag KERNEL_PAIR=()
declare -Ag READY_RECEIPT=()

canonical_sha256() {
	[[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

canonical_release() {
	[[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$ ]]
}

norm_windows_path() {
	local value="$1"
	value="${value//$'\r'/}"
	value="${value//\\//}"
	while [[ "$value" == *//* ]]; do value="${value//\/\//\/}"; done
	if [[ "$value" =~ ^([A-Za-z]):(.*)$ ]]; then
		printf '%s:%s' "${BASH_REMATCH[1],,}" "${BASH_REMATCH[2]}"
	else
		printf '%s' "$value"
	fi
}

windows_to_wsl_path() {
	local value="$1"
	value="${value//\\//}"
	if [[ "$value" =~ ^([A-Za-z]):/(.*)$ ]]; then
		printf '/mnt/%s/%s' "${BASH_REMATCH[1],,}" "${BASH_REMATCH[2]}"
		return 0
	fi
	return 1
}

wsl_to_windows_path() {
	local value="$1"
	if [[ "$value" =~ ^/mnt/([A-Za-z])/(.*)$ ]]; then
		printf '%s:/%s' "${BASH_REMATCH[1],,}" "${BASH_REMATCH[2]}"
		return 0
	fi
	return 1
}

win_user() {
	local user=""
	if [[ -n "${WIN_USER:-}" ]]; then
		printf '%s' "$WIN_USER"
		return 0
	fi
	user="$(timeout "$INTEROP_FAIL_SEC" /mnt/c/Windows/System32/cmd.exe /d /c 'echo %USERNAME%' 2>/dev/null | tr -d '\0\r\n' || true)"
	if [[ -z "$user" || "$user" == *'%USERNAME%'* ]]; then
		return 1
	fi
	printf '%s' "$user"
}

wslconfig_path() {
	if [[ -n "${WSL_CONFIG:-}" ]]; then
		printf '%s' "$WSL_CONFIG"
		return 0
	fi
	local user
	user="$(win_user)" || return 1
	printf '/mnt/c/Users/%s/.wslconfig' "$user"
}

load_kernel_pair_manifest() {
	local manifest="$1"
	PAIR_ERROR=""
	KERNEL_PAIR=()
	if [[ -z "$manifest" || "$manifest" != /* || ! -f "$manifest" || -L "$manifest" ]]; then
		PAIR_ERROR="kernel-pair manifest must be an absolute regular non-symlink file"
		return 1
	fi
	local parent
	parent="$(dirname -- "$manifest")"
	if [[ -L "$parent" ]]; then
		PAIR_ERROR="kernel-pair directory must not be a symlink"
		return 1
	fi
	local line key value
	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -z "$line" || ! "$line" =~ ^([a-z][a-z0-9_]*)=([ -~]*)$ ]]; then
			PAIR_ERROR="kernel-pair manifest contains a malformed line"
			return 1
		fi
		key="${BASH_REMATCH[1]}"
		value="${BASH_REMATCH[2]}"
		if [[ -n "${KERNEL_PAIR[$key]+x}" ]]; then
			PAIR_ERROR="kernel-pair manifest contains duplicate key $key"
			return 1
		fi
		KERNEL_PAIR[$key]="$value"
	done <"$manifest"
	local expected=(
		schema pair_id release
		kernel_file kernel_sha256 kernel_size_bytes
		initramfs_file initramfs_sha256 initramfs_size_bytes
		modules_file modules_sha256 modules_size_bytes
		modules_layout layout_release_directory_count
		layout_nested_release_directory_count layout_inventory_sha256
		module_name module_vermagic minimum_wsl_version
		qemu_stamp_sha256 qemu_kernel_sha256 qemu_release
	)
	if ((${#KERNEL_PAIR[@]} != ${#expected[@]})); then
		PAIR_ERROR="kernel-pair manifest contains missing or unknown keys"
		return 1
	fi
	for key in "${expected[@]}"; do
		if [[ -z "${KERNEL_PAIR[$key]+x}" ]]; then
			PAIR_ERROR="kernel-pair manifest is missing $key"
			return 1
		fi
	done
	if [[ "${KERNEL_PAIR[schema]}" != "ramshared.kernel-pair.v1" ]] ||
		! canonical_release "${KERNEL_PAIR[release]}"; then
		PAIR_ERROR="kernel-pair schema or release is invalid"
		return 1
	fi
	if [[ "${KERNEL_PAIR[kernel_file]}" != "kernel.bzImage" ||
		"${KERNEL_PAIR[modules_file]}" != "modules.vhdx" ]]; then
		PAIR_ERROR="kernel-pair artifact names are not canonical"
		return 1
	fi
	for key in kernel_sha256 modules_sha256 layout_inventory_sha256 qemu_stamp_sha256 qemu_kernel_sha256; do
		if ! canonical_sha256 "${KERNEL_PAIR[$key]}"; then
			PAIR_ERROR="kernel-pair $key is invalid"
			return 1
		fi
	done
	if [[ ! "${KERNEL_PAIR[kernel_size_bytes]}" =~ ^[1-9][0-9]*$ ||
		! "${KERNEL_PAIR[modules_size_bytes]}" =~ ^[1-9][0-9]*$ ]]; then
		PAIR_ERROR="kernel-pair artifact size is invalid"
		return 1
	fi
	if [[ ! "$MIN_BZIMAGE_BYTES" =~ ^[0-9]+$ ]]; then
		PAIR_ERROR="MIN_BZIMAGE_BYTES is invalid"
		return 1
	fi
	if [[ "${KERNEL_PAIR[modules_layout]}" != "legacy_flat_v1" ||
		"${KERNEL_PAIR[layout_release_directory_count]}" != "0" ||
		"${KERNEL_PAIR[layout_nested_release_directory_count]}" != "0" ]]; then
		PAIR_ERROR="unified, mismatched, or double-nested modules layout is not admitted"
		return 1
	fi
	if [[ ! "${KERNEL_PAIR[module_name]}" =~ ^[A-Za-z0-9_+-]+$ ||
		"${KERNEL_PAIR[module_vermagic]%% *}" != "${KERNEL_PAIR[release]}" ||
		! "${KERNEL_PAIR[minimum_wsl_version]}" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
		PAIR_ERROR="kernel-pair module metadata or runtime minimum is invalid"
		return 1
	fi
	local expected_pair_id="v1-${KERNEL_PAIR[kernel_sha256]:0:16}-${KERNEL_PAIR[modules_sha256]:0:16}"
	if [[ "${KERNEL_PAIR[pair_id]}" != "$expected_pair_id" ||
		"${KERNEL_PAIR[qemu_kernel_sha256]}" != "${KERNEL_PAIR[kernel_sha256]}" ||
		"${KERNEL_PAIR[qemu_release]}" != "${KERNEL_PAIR[release]}" ]]; then
		PAIR_ERROR="kernel-pair identity or QEMU binding is inconsistent"
		return 1
	fi
	KERNEL_PAIR[manifest_path]="$manifest"
	KERNEL_PAIR[manifest_sha256]="$(sha256sum -- "$manifest" | awk '{print $1}')"
	KERNEL_PAIR[kernel_path]="$parent/${KERNEL_PAIR[kernel_file]}"
	KERNEL_PAIR[modules_path]="$parent/${KERNEL_PAIR[modules_file]}"
	KERNEL_PAIR[layout_inventory_path]="$parent/modules-layout.manifest"
	KERNEL_PAIR[qemu_stamp_path]="$parent/qemu-pass.stamp"
	for key in kernel_path modules_path layout_inventory_path qemu_stamp_path; do
		if [[ ! -f "${KERNEL_PAIR[$key]}" || -L "${KERNEL_PAIR[$key]}" ]]; then
			PAIR_ERROR="sealed pair artifact is missing or symlinked: ${KERNEL_PAIR[$key]}"
			return 1
		fi
	done
	if [[ "$(stat -c '%s' -- "${KERNEL_PAIR[kernel_path]}")" != "${KERNEL_PAIR[kernel_size_bytes]}" ||
		"$(stat -c '%s' -- "${KERNEL_PAIR[modules_path]}")" != "${KERNEL_PAIR[modules_size_bytes]}" ||
		"$(sha256sum -- "${KERNEL_PAIR[kernel_path]}" | awk '{print $1}')" != "${KERNEL_PAIR[kernel_sha256]}" ||
		"$(sha256sum -- "${KERNEL_PAIR[modules_path]}" | awk '{print $1}')" != "${KERNEL_PAIR[modules_sha256]}" ||
		"$(sha256sum -- "${KERNEL_PAIR[layout_inventory_path]}" | awk '{print $1}')" != "${KERNEL_PAIR[layout_inventory_sha256]}" ||
		"$(sha256sum -- "${KERNEL_PAIR[qemu_stamp_path]}" | awk '{print $1}')" != "${KERNEL_PAIR[qemu_stamp_sha256]}" ]]; then
		PAIR_ERROR="sealed pair artifact size or hash readback failed"
		return 1
	fi
	if ((${KERNEL_PAIR[kernel_size_bytes]} <= MIN_BZIMAGE_BYTES)); then
		PAIR_ERROR="sealed kernel image is below the minimum size"
		return 1
	fi
	declare -A inventory=()
	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -z "$line" || ! "$line" =~ ^([a-z][a-z0-9_]*)=([ -~]*)$ ]]; then
			PAIR_ERROR="modules layout inventory contains a malformed line"
			return 1
		fi
		key="${BASH_REMATCH[1]}"
		[[ -z "${inventory[$key]+x}" ]] || {
			PAIR_ERROR="modules layout inventory contains duplicate key $key"
			return 1
		}
		inventory[$key]="${BASH_REMATCH[2]}"
	done <"${KERNEL_PAIR[layout_inventory_path]}"
	local inventory_keys=(schema layout release release_directory_count nested_release_directory_count modules_sha256 modules_size_bytes)
	if ((${#inventory[@]} != ${#inventory_keys[@]})); then
		PAIR_ERROR="modules layout inventory contains missing or unknown keys"
		return 1
	fi
	for key in "${inventory_keys[@]}"; do
		[[ -n "${inventory[$key]+x}" ]] || {
			PAIR_ERROR="modules layout inventory is missing $key"
			return 1
		}
	done
	if [[ "${inventory[schema]}" != "ramshared.modules-layout.v1" ||
		"${inventory[layout]}" != "${KERNEL_PAIR[modules_layout]}" ||
		"${inventory[release]}" != "${KERNEL_PAIR[release]}" ||
		"${inventory[release_directory_count]}" != "${KERNEL_PAIR[layout_release_directory_count]}" ||
		"${inventory[nested_release_directory_count]}" != "${KERNEL_PAIR[layout_nested_release_directory_count]}" ||
		"${inventory[modules_sha256]}" != "${KERNEL_PAIR[modules_sha256]}" ||
		"${inventory[modules_size_bytes]}" != "${KERNEL_PAIR[modules_size_bytes]}" ]]; then
		PAIR_ERROR="modules layout inventory differs from the sealed pair manifest"
		return 1
	fi
	declare -A stamp=()
	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -z "$line" || ! "$line" =~ ^([A-Z][A-Z0-9_]*)=([ -~]*)$ ]]; then
			PAIR_ERROR="QEMU stamp contains a malformed line"
			return 1
		fi
		key="${BASH_REMATCH[1]}"
		[[ -z "${stamp[$key]+x}" ]] || {
			PAIR_ERROR="QEMU stamp contains duplicate key $key"
			return 1
		}
		stamp[$key]="${BASH_REMATCH[2]}"
	done <"${KERNEL_PAIR[qemu_stamp_path]}"
	local stamp_keys=(REL KERNEL_SHA256 HEAD DATE VALIDATE)
	if ((${#stamp[@]} != ${#stamp_keys[@]})); then
		PAIR_ERROR="QEMU stamp contains missing or unknown keys"
		return 1
	fi
	for key in "${stamp_keys[@]}"; do
		[[ -n "${stamp[$key]+x}" ]] || {
			PAIR_ERROR="QEMU stamp is missing $key"
			return 1
		}
	done
	if [[ "${stamp[REL]}" != "${KERNEL_PAIR[release]}" ||
		"${stamp[KERNEL_SHA256]}" != "${KERNEL_PAIR[kernel_sha256]}" ||
		! "${stamp[HEAD]}" =~ ^[0-9a-f]{7,64}$ ||
		! "${stamp[DATE]}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T ||
		"${stamp[VALIDATE]}" != "qemu-validate.sh" ]]; then
		PAIR_ERROR="QEMU stamp semantics differ from the sealed pair"
		return 1
	fi
	return 0
}

load_ready_receipt() {
	local receipt="$1"
	RECEIPT_ERROR=""
	READY_RECEIPT=()
	if [[ ! -f "$receipt" || -L "$receipt" ]]; then
		RECEIPT_ERROR="READY promotion receipt is missing"
		return 1
	fi
	local parsed
	if ! parsed="$(python3 - "$receipt" <<'PY'
import json
import re
import sys

path = sys.argv[1]
with open(path, "rb") as handle:
    raw = handle.read()
if not raw or len(raw) > 1024 * 1024:
    raise SystemExit("receipt size is invalid")
data = json.loads(raw.decode("utf-8"))
top_keys = {
    "schema", "transaction_id", "status", "started_at_utc",
    "completed_at_utc", "distro", "pair_id", "manifest_sha256",
    "kernel_path", "kernel_sha256", "modules_path", "modules_sha256",
    "modules_layout", "release", "module_name", "module_vermagic",
    "original_wslconfig_existed", "original_wslconfig_sha256",
    "original_wslconfig_snapshot", "bundled_wslconfig_sha256",
    "candidate_wslconfig_sha256", "approved_failed_units", "host",
    "baseline", "candidate", "rollback", "failure",
}
if not isinstance(data, dict) or set(data) != top_keys:
    raise SystemExit("receipt contains missing or unknown top-level fields")
if data["schema"] != "ramshared.kernel-promotion-receipt.v1" or data["status"] != "READY":
    raise SystemExit("receipt is not READY")
if data["rollback"] is not None or data["failure"] is not None:
    raise SystemExit("READY receipt contains rollback or failure state")
sha = re.compile(r"^[0-9a-f]{64}$")
for key in (
    "manifest_sha256", "kernel_sha256", "modules_sha256",
    "original_wslconfig_sha256", "bundled_wslconfig_sha256",
    "candidate_wslconfig_sha256",
):
    if not isinstance(data[key], str) or not sha.fullmatch(data[key]):
        raise SystemExit(f"invalid {key}")
if data["modules_layout"] != "legacy_flat_v1":
    raise SystemExit("receipt layout is not admitted")
if not isinstance(data["original_wslconfig_existed"], bool):
    raise SystemExit("original_wslconfig_existed is not boolean")
if not isinstance(data["approved_failed_units"], list) or any(
    not isinstance(unit, str) or not re.fullmatch(r"[A-Za-z0-9_.@:-]+\.service", unit)
    for unit in data["approved_failed_units"]
):
    raise SystemExit("approved failed-unit set is malformed")
if data["approved_failed_units"] != sorted(set(data["approved_failed_units"])):
    raise SystemExit("approved failed-unit set is not canonical")
if not isinstance(data["transaction_id"], str) or not re.fullmatch(
    r"[0-9]{8}T[0-9]{9}Z-[0-9a-f]{32}", data["transaction_id"]
):
    raise SystemExit("transaction identity is malformed")
timestamp = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.+-]+Z$")
for key in ("started_at_utc", "completed_at_utc"):
    if not isinstance(data[key], str) or not timestamp.fullmatch(data[key]):
        raise SystemExit(f"receipt timestamp is malformed: {key}")
if not isinstance(data["distro"], str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._ -]{0,127}", data["distro"]):
    raise SystemExit("receipt distro is malformed")
if not isinstance(data["pair_id"], str) or not re.fullmatch(r"v1-[0-9a-f]{16}-[0-9a-f]{16}", data["pair_id"]):
    raise SystemExit("receipt pair identity is malformed")
if not isinstance(data["release"], str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}", data["release"]):
    raise SystemExit("receipt release is malformed")
if not isinstance(data["module_name"], str) or not re.fullmatch(r"[A-Za-z0-9_+-]+", data["module_name"]):
    raise SystemExit("receipt module name is malformed")
if not isinstance(data["module_vermagic"], str) or data["module_vermagic"].split()[0] != data["release"]:
    raise SystemExit("receipt module vermagic differs from release")
for key in ("kernel_path", "modules_path", "original_wslconfig_snapshot"):
    if not isinstance(data[key], str) or not re.fullmatch(r"[A-Za-z]:[\\/].+", data[key]):
        raise SystemExit(f"receipt path is malformed: {key}")

host_keys = {
    "windows_product_name", "windows_display_version", "windows_current_build",
    "windows_ubr", "wsl_version", "wsl_kernel_version", "wslg_version",
    "wsl_reported_windows_version", "wsl_version_output_sha256",
}
host = data["host"]
if not isinstance(host, dict) or set(host) != host_keys:
    raise SystemExit("host identity contains missing or unknown fields")
for key, value in host.items():
    if not isinstance(value, str) or not value or any(char in value for char in "\r\n\t"):
        raise SystemExit(f"host identity value is malformed: {key}")
if not sha.fullmatch(host["wsl_version_output_sha256"]):
    raise SystemExit("host WSL version output hash is malformed")
for key in ("wsl_version", "wsl_kernel_version", "wslg_version", "wsl_reported_windows_version"):
    if not re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}(?:[-+][A-Za-z0-9._-]+)?", host[key]):
        raise SystemExit(f"host version is malformed: {key}")

canary_keys = {
    "CANARY_SCHEMA", "CANARY_PHASE", "CANARY_WSL_EXIT", "CANARY_BOOT_ID",
    "CANARY_UNAME", "CANARY_SYSTEMD", "CANARY_FAILED_UNITS",
    "CANARY_DXG_NODE", "CANARY_DXG_DEV_T", "CANARY_DXG_COUNT",
    "CANARY_XWAYLAND_COUNT_BEFORE", "CANARY_XWAYLAND_COUNT_AFTER",
    "CANARY_WSLG_TRANSACTION", "CANARY_GPU_DRIVER", "CANARY_DXG_PROBE",
    "CANARY_MODULES", "CANARY_MODULE_VERMAGIC", "CANARY_MODULE_TREE",
    "CANARY_DISTRO_ID", "CANARY_DISTRO_VERSION_ID",
    "CANARY_DMESG_READABLE", "CANARY_DMESG_SHA256",
    "CANARY_DXG_FORTIFY_WARNINGS", "CANARY_WAIT_FOR_BOOT_FAILURES",
    "CANARY_JOURNAL_UNCLEAN", "CANARY_P9_CANCELLED",
    "CANARY_KERNEL_FATALS", "CANARY_DXG_QUERY_ERRORS",
}
uuid = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
numeric_keys = (
    "CANARY_DXG_COUNT", "CANARY_XWAYLAND_COUNT_BEFORE",
    "CANARY_XWAYLAND_COUNT_AFTER", "CANARY_DXG_FORTIFY_WARNINGS",
    "CANARY_WAIT_FOR_BOOT_FAILURES", "CANARY_JOURNAL_UNCLEAN",
    "CANARY_P9_CANCELLED", "CANARY_KERNEL_FATALS", "CANARY_DXG_QUERY_ERRORS",
)
hard_zero_keys = (
    "CANARY_DXG_FORTIFY_WARNINGS", "CANARY_WAIT_FOR_BOOT_FAILURES",
    "CANARY_JOURNAL_UNCLEAN", "CANARY_P9_CANCELLED", "CANARY_KERNEL_FATALS",
)

def validate_canary(value, phase):
    if not isinstance(value, dict) or set(value) != canary_keys:
        raise SystemExit(f"{phase} canary contains missing or unknown fields")
    if any(not isinstance(item, str) or any(char in item for char in "\r\n\t") for item in value.values()):
        raise SystemExit(f"{phase} canary contains a malformed value")
    if value["CANARY_SCHEMA"] != "1" or value["CANARY_PHASE"] != phase or value["CANARY_WSL_EXIT"] != "0":
        raise SystemExit(f"{phase} canary identity is invalid")
    if not uuid.fullmatch(value["CANARY_BOOT_ID"]):
        raise SystemExit(f"{phase} boot ID is invalid")
    for key in numeric_keys:
        if not re.fullmatch(r"[0-9]+", value[key]):
            raise SystemExit(f"{phase} numeric canary field is invalid: {key}")
    if value["CANARY_DXG_NODE"] != "char" or not re.fullmatch(r"[0-9a-f]+:[0-9a-f]+", value["CANARY_DXG_DEV_T"]):
        raise SystemExit(f"{phase} DXG identity is invalid")
    if int(value["CANARY_DXG_COUNT"]) != 1:
        raise SystemExit(f"{phase} DXG cardinality is invalid")
    if value["CANARY_WSLG_TRANSACTION"] != "ok" or int(value["CANARY_XWAYLAND_COUNT_AFTER"]) < 1:
        raise SystemExit(f"{phase} WSLg transaction is invalid")
    if value["CANARY_DXG_PROBE"] != "ok" or not re.fullmatch(r"[A-Za-z0-9._,-]+", value["CANARY_GPU_DRIVER"]):
        raise SystemExit(f"{phase} driver probe is invalid")
    if value["CANARY_DMESG_READABLE"] != "1" or not sha.fullmatch(value["CANARY_DMESG_SHA256"]):
        raise SystemExit(f"{phase} dmesg evidence is invalid")
    if any(int(value[key]) != 0 for key in hard_zero_keys):
        raise SystemExit(f"{phase} canary contains a hard rollback signal")
    failed = [] if value["CANARY_FAILED_UNITS"] == "none" else sorted(set(value["CANARY_FAILED_UNITS"].split(",")))
    if value["CANARY_SYSTEMD"] == "running":
        if failed:
            raise SystemExit(f"{phase} running systemd reports failed units")
    elif value["CANARY_SYSTEMD"] == "degraded":
        if not data["approved_failed_units"] or failed != data["approved_failed_units"]:
            raise SystemExit(f"{phase} degraded units differ from explicit approval")
    else:
        raise SystemExit(f"{phase} systemd state is invalid")
    if not re.fullmatch(r"[a-z0-9._-]+", value["CANARY_DISTRO_ID"]) or not re.fullmatch(r"[A-Za-z0-9._-]+", value["CANARY_DISTRO_VERSION_ID"]):
        raise SystemExit(f"{phase} distro identity is invalid")
    return value

baseline = validate_canary(data["baseline"], "bundled")
candidate = validate_canary(data["candidate"], "candidate")
if candidate["CANARY_BOOT_ID"] == baseline["CANARY_BOOT_ID"]:
    raise SystemExit("candidate boot ID is not fresh")
for key in ("CANARY_DISTRO_ID", "CANARY_DISTRO_VERSION_ID", "CANARY_GPU_DRIVER", "CANARY_DXG_COUNT"):
    if candidate[key] != baseline[key]:
        raise SystemExit(f"candidate differs from bundled baseline: {key}")
if int(candidate["CANARY_DXG_QUERY_ERRORS"]) > int(baseline["CANARY_DXG_QUERY_ERRORS"]):
    raise SystemExit("candidate DXG errors exceed the bundled baseline")
if (
    candidate["CANARY_UNAME"] != data["release"]
    or candidate["CANARY_MODULES"] != "ok"
    or candidate["CANARY_MODULE_TREE"] != "ok"
    or candidate["CANARY_MODULE_VERMAGIC"] != data["module_vermagic"]
):
    raise SystemExit("candidate kernel/modules evidence differs from the receipt pair")
values = {
    "pair_id": data["pair_id"],
    "manifest_sha256": data["manifest_sha256"],
    "kernel_path": data["kernel_path"],
    "kernel_sha256": data["kernel_sha256"],
    "modules_path": data["modules_path"],
    "modules_sha256": data["modules_sha256"],
    "modules_layout": data["modules_layout"],
    "release": data["release"],
    "module_name": data["module_name"],
    "module_vermagic": data["module_vermagic"],
    "config_sha256": data["candidate_wslconfig_sha256"],
    "candidate_uname": candidate["CANARY_UNAME"],
    "candidate_vermagic": candidate["CANARY_MODULE_VERMAGIC"],
    "candidate_boot_id": candidate["CANARY_BOOT_ID"],
    "distro": data["distro"],
    "host_wsl_version": host["wsl_version"],
    "snapshot_path": data["original_wslconfig_snapshot"],
    "snapshot_sha256": data["original_wslconfig_sha256"],
}
for key, value in values.items():
    if not isinstance(value, str) or "\n" in value or "\t" in value:
        raise SystemExit(f"invalid receipt value {key}")
    print(f"{key}\t{value}")
PY
)"; then
		RECEIPT_ERROR="READY promotion receipt failed strict parsing"
		return 1
	fi
	local key value
	while IFS=$'\t' read -r key value; do
		[[ -n "$key" && -z "${READY_RECEIPT[$key]+x}" ]] || {
			RECEIPT_ERROR="READY receipt output is duplicate or malformed"
			return 1
		}
		READY_RECEIPT[$key]="$value"
	done <<<"$parsed"
	return 0
}

config_pair_values() {
	local cfg="$1"
	[[ -f "$cfg" && ! -L "$cfg" ]] || return 1
	local line key value
	local kernel_count=0 modules_count=0 wsl2_count=0 kernel_value="" modules_value=""
	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ "$line" =~ ^[[:space:]]*\[wsl2\][[:space:]]*$ ]]; then
			((wsl2_count += 1))
		elif [[ "$line" =~ ^[[:space:]]*kernel[[:space:]]*=[[:space:]]*(.*)$ ]]; then
			((kernel_count += 1))
			kernel_value="${BASH_REMATCH[1]}"
		elif [[ "$line" =~ ^[[:space:]]*kernelModules[[:space:]]*=[[:space:]]*(.*)$ ]]; then
			((modules_count += 1))
			modules_value="${BASH_REMATCH[1]}"
		fi
	done <"$cfg"
	[[ $wsl2_count -eq 1 && $kernel_count -eq 1 && $modules_count -eq 1 ]] || return 1
	printf '%s\n%s\n' "$(norm_windows_path "$kernel_value")" "$(norm_windows_path "$modules_value")"
}

probe_ready_contract() {
	load_ready_receipt "$PROMOTION_RECEIPT_WSL" || return 1
	local kernel_wsl modules_wsl manifest_wsl snapshot_wsl cfg kernel_windows modules_windows current_boot_id
	if [[ "${READY_RECEIPT[distro]}" != "$KERNEL_CANARY_DISTRO" ]]; then
		RECEIPT_ERROR="READY receipt belongs to a different exact distro"
		return 1
	fi
	kernel_wsl="$(windows_to_wsl_path "${READY_RECEIPT[kernel_path]}")" || {
		RECEIPT_ERROR="receipt kernel path is not a Windows drive path"; return 1;
	}
	modules_wsl="$(windows_to_wsl_path "${READY_RECEIPT[modules_path]}")" || {
		RECEIPT_ERROR="receipt modules path is not a Windows drive path"; return 1;
	}
	snapshot_wsl="$(windows_to_wsl_path "${READY_RECEIPT[snapshot_path]}")" || {
		RECEIPT_ERROR="receipt snapshot path is not a Windows drive path"; return 1;
	}
	if [[ ! -f "$snapshot_wsl" || -L "$snapshot_wsl" ||
		"$(sha256sum -- "$snapshot_wsl" 2>/dev/null | awk '{print $1}')" != "${READY_RECEIPT[snapshot_sha256]}" ]]; then
		RECEIPT_ERROR="fresh original .wslconfig snapshot is missing or hash-mismatched"
		return 1
	fi
	manifest_wsl="$(dirname -- "$kernel_wsl")/kernel-pair.manifest"
	if ! load_kernel_pair_manifest "$manifest_wsl"; then
		RECEIPT_ERROR="$PAIR_ERROR"
		return 1
	fi
	kernel_windows="$(wsl_to_windows_path "${KERNEL_PAIR[kernel_path]}")" || {
		RECEIPT_ERROR="installed kernel is not on a mounted Windows drive"; return 1;
	}
	modules_windows="$(wsl_to_windows_path "${KERNEL_PAIR[modules_path]}")" || {
		RECEIPT_ERROR="installed modules are not on a mounted Windows drive"; return 1;
	}
	if [[ "${KERNEL_PAIR[manifest_sha256]}" != "${READY_RECEIPT[manifest_sha256]}" ||
		"${KERNEL_PAIR[pair_id]}" != "${READY_RECEIPT[pair_id]}" ||
		"${KERNEL_PAIR[kernel_sha256]}" != "${READY_RECEIPT[kernel_sha256]}" ||
		"${KERNEL_PAIR[modules_sha256]}" != "${READY_RECEIPT[modules_sha256]}" ||
		"${KERNEL_PAIR[release]}" != "${READY_RECEIPT[release]}" ||
		"${KERNEL_PAIR[modules_layout]}" != "${READY_RECEIPT[modules_layout]}" ||
		"${KERNEL_PAIR[module_vermagic]}" != "${READY_RECEIPT[module_vermagic]}" ||
		"$(norm_windows_path "$kernel_windows")" != "$(norm_windows_path "${READY_RECEIPT[kernel_path]}")" ||
		"$(norm_windows_path "$modules_windows")" != "$(norm_windows_path "${READY_RECEIPT[modules_path]}")" ]]; then
		RECEIPT_ERROR="READY receipt differs from the sealed installed pair"
		return 1
	fi
	cfg="$(wslconfig_path)" || {
		RECEIPT_ERROR="cannot resolve .wslconfig"; return 1;
	}
	if [[ "$(sha256sum -- "$cfg" 2>/dev/null | awk '{print $1}')" != "${READY_RECEIPT[config_sha256]}" ]]; then
		RECEIPT_ERROR="current .wslconfig hash differs from the qualified candidate"
		return 1
	fi
	local -a pair_values=()
	mapfile -t pair_values < <(config_pair_values "$cfg") || true
	if [[ ${#pair_values[@]} -ne 2 ||
		"${pair_values[0]}" != "$(norm_windows_path "${READY_RECEIPT[kernel_path]}")" ||
		"${pair_values[1]}" != "$(norm_windows_path "${READY_RECEIPT[modules_path]}")" ]]; then
		RECEIPT_ERROR="current .wslconfig does not select the exact qualified pair"
		return 1
	fi
	if [[ "$(uname -r)" != "${READY_RECEIPT[release]}" ||
		"${READY_RECEIPT[candidate_uname]}" != "${READY_RECEIPT[release]}" ]]; then
		RECEIPT_ERROR="running kernel release differs from the qualified pair"
		return 1
	fi
	current_boot_id="$(tr -d '\r\n' </proc/sys/kernel/random/boot_id 2>/dev/null || true)"
	if [[ "$current_boot_id" != "${READY_RECEIPT[candidate_boot_id]}" ]]; then
		RECEIPT_ERROR="current boot ID differs from the qualified candidate canary"
		return 1
	fi
	local vermagic
	vermagic="$(modinfo -F vermagic -- "${READY_RECEIPT[module_name]}" 2>/dev/null | head -n 1 || true)"
	if [[ "$vermagic" != "${READY_RECEIPT[module_vermagic]}" ||
		"${READY_RECEIPT[candidate_vermagic]}" != "${READY_RECEIPT[module_vermagic]}" ||
		! -r "/lib/modules/${READY_RECEIPT[release]}/modules.dep" ||
		-d "/lib/modules/${READY_RECEIPT[release]}/${READY_RECEIPT[release]}" ]]; then
		RECEIPT_ERROR="running modules release, vermagic, or layout differs from the qualified pair"
		return 1
	fi
	return 0
}

probe_ublk() {
	if [[ -d /sys/module/ublk_drv ]] || lsmod 2>/dev/null | grep -q '^ublk_drv'; then
		return 0
	fi
	if modprobe -n ublk_drv >/dev/null 2>&1; then
		return 1
	fi
	return 2
}

resolve_state() {
	STATE="NEED_BUILD"
	if ! probe_ready_contract; then
		if [[ -f "$PROMOTION_RECEIPT_WSL" ]]; then
			STATE="BROKEN"
		else
			STATE="NEED_ARM"
		fi
		return 0
	fi
	local module_state=0
	probe_ublk || module_state=$?
	case "$module_state" in
	0) STATE="READY" ;;
	1) STATE="NEED_MODULE" ;;
	*) STATE="BROKEN"; RECEIPT_ERROR="qualified module is not loadable" ;;
	esac
}

print_status_lines() {
	local cfg
	cfg="$(wslconfig_path 2>/dev/null || printf '(unknown)')"
	printf 'STATE=%s\n' "$STATE"
	printf 'uname=%s\n' "$(uname -r)"
	printf 'wslconfig=%s\n' "$cfg"
	printf 'promotion_receipt=%s\n' "$PROMOTION_RECEIPT_WSL"
	if [[ -n "${READY_RECEIPT[pair_id]:-}" ]]; then
		printf 'pair_id=%s\n' "${READY_RECEIPT[pair_id]}"
		printf 'release=%s\n' "${READY_RECEIPT[release]}"
		printf 'modules_layout=%s\n' "${READY_RECEIPT[modules_layout]}"
	fi
	if [[ -n "$RECEIPT_ERROR" ]]; then
		printf 'verification_error=%s\n' "$RECEIPT_ERROR"
	fi
}

next_step_msg() {
	case "$STATE" in
	NEED_BUILD|NEED_ARM)
		echo "NEXT: seal an immutable kernel/modules pair, then run attended apply"
		;;
	NEED_REBOOT)
		echo "NEXT: do not use a natural reboot; re-run attended apply or disarm"
		;;
	NEED_MODULE)
		echo "NEXT: enable may load the already qualified ublk_drv module"
		;;
	BROKEN)
		echo "NEXT: keep LIVE-NO-GO; inspect the exact verification_error and requalify"
		;;
	READY)
		echo "NEXT: nothing; the exact promotion contract is READY"
		;;
	*)
		echo "NEXT: unknown state; keep LIVE-NO-GO"
		;;
	esac
}
