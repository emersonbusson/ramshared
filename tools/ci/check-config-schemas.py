#!/usr/bin/env python3
import sys
import json
import os

EX_USAGE = 64
EX_UNAVAILABLE = 69
EX_IOERR = 74
EX_CONFIG = 78

# Physical limits sanity checks
MAX_CAPACITY_BYTES = 100 * 1024**4 # 100 TB
MIN_CAPACITY_BYTES = 64 * 1024**2 # 64 MB
MAX_BLOCK_SIZE = 1024 * 1024 # 1 MB
MIN_BLOCK_SIZE = 4096 # 4 KB

def fail(msg, code=EX_CONFIG):
    print(msg, file=sys.stderr)
    sys.exit(code)

try:
    import tomllib
except ImportError:
    fail("tomllib is required (Python 3.11+)", EX_UNAVAILABLE)

def check_winbroker(path):
    if not os.path.exists(path):
        return
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except Exception as e:
        fail(f"failed to parse {path}: {e}")
    if "local_broker" not in data:
        fail(f"{path}: missing local_broker")
    lb = data["local_broker"]
    if not isinstance(lb.get("schema"), int):
        fail(f"{path}: invalid schema")

    cap = lb.get("capacity_bytes")
    if not isinstance(cap, int):
        fail(f"{path}: invalid capacity_bytes")
    if cap < MIN_CAPACITY_BYTES or cap > MAX_CAPACITY_BYTES:
        fail(f"{path}: capacity_bytes {cap} out of physical limits ({MIN_CAPACITY_BYTES}-{MAX_CAPACITY_BYTES})")

    if not isinstance(lb.get("allowed_tenant"), str):
        fail(f"{path}: invalid allowed_tenant")
    if not isinstance(lb.get("evidence_path"), str):
        fail(f"{path}: invalid evidence_path")

def check_winsvc(path):
    if not os.path.exists(path):
        return
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except Exception as e:
        fail(f"failed to parse {path}: {e}")
    if "win_drive" not in data:
        fail(f"{path}: missing win_drive")
    wd = data["win_drive"]

    size = wd.get("size_bytes")
    if not isinstance(size, int) or size < MIN_CAPACITY_BYTES or size > MAX_CAPACITY_BYTES:
        fail(f"{path}: size_bytes out of limits")

    bs = wd.get("block_size")
    if not isinstance(bs, int) or bs < MIN_BLOCK_SIZE or bs > MAX_BLOCK_SIZE:
        fail(f"{path}: block_size out of limits")

    req = ["cuda_device", "reserve_bytes", "queue_depth", "max_io_bytes", "broker_ready_timeout_secs", "heartbeat_secs"]
    for k in req:
        if not isinstance(wd.get(k), int):
            fail(f"{path}: invalid {k}")

    req_s = ["evidence_path", "volume_letter", "broker_pipe", "tenant"]
    for k in req_s:
        if not isinstance(wd.get(k), str):
            fail(f"{path}: invalid {k}")

def check_docker_daemon(path):
    if not os.path.exists(path):
        return
    try:
        with open(path, "r") as f:
            data = json.load(f)
    except Exception as e:
        fail(f"failed to parse {path}: {e}")
    if not isinstance(data.get("cgroup-parent"), str):
        fail(f"{path}: invalid cgroup-parent")

def check_file(filepath):
    # Only validate files we explicitly consider RamShared config files
    if "broker.example.toml" in filepath:
        check_winbroker(filepath)
    elif "winsvc.example.toml" in filepath:
        check_winsvc(filepath)
    elif "docker-daemon-ramshared.json" in filepath:
        check_docker_daemon(filepath)

if len(sys.argv) < 2:
    fail("Usage: check-config-schemas.py <root_dir>", EX_USAGE)

root_dir = sys.argv[1]
if not os.path.isdir(root_dir):
    fail(f"Directory not found: {root_dir}", EX_IOERR)

for root, _, files in os.walk(root_dir):
    if "node_modules" in root or "target" in root:
        continue
    for file in files:
        if file.endswith(".toml") or file.endswith(".json"):
            check_file(os.path.join(root, file))

print("All config schemas OK")
sys.exit(0)
