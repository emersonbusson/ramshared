#!/usr/bin/env python3
"""Allocate deterministic memory and verify it before a graceful exit."""

import argparse
import hashlib
import json
import os
import signal
import sys
import time


stop_requested = False


def request_stop(_signum: int, _frame: object) -> None:
    global stop_requested
    stop_requested = True


def chunk_pattern(index: int, size: int) -> bytearray:
    seed = hashlib.sha256(f"ramshared:{index}".encode("ascii")).digest()
    repeats, remainder = divmod(size, len(seed))
    return bytearray(seed * repeats + seed[:remainder])


def digest_chunks(chunks: list[bytearray]) -> str:
    digest = hashlib.sha256()
    for chunk in chunks:
        digest.update(chunk)
    return digest.hexdigest()


def write_result(path: str, result: dict[str, object]) -> None:
    temporary = f"{path}.tmp.{os.getpid()}"
    with open(temporary, "w", encoding="utf-8") as target:
        json.dump(result, target, separators=(",", ":"), sort_keys=True)
        target.write("\n")
    os.replace(temporary, path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    size = parser.add_mutually_exclusive_group(required=True)
    size.add_argument("--allocate-mib", type=int)
    size.add_argument("--allocate-gib", type=float)
    parser.add_argument("--chunk-mib", type=int, default=64)
    parser.add_argument("--result", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    allocate_mib = (
        args.allocate_mib
        if args.allocate_mib is not None
        else int(args.allocate_gib * 1024)
    )
    if allocate_mib <= 0 or args.chunk_mib <= 0:
        raise ValueError("allocation and chunk sizes must be positive")

    signal.signal(signal.SIGTERM, request_stop)
    signal.signal(signal.SIGINT, request_stop)

    chunks: list[bytearray] = []
    allocation_digest = hashlib.sha256()
    allocated_mib = 0
    try:
        while allocated_mib < allocate_mib:
            if stop_requested:
                break
            current_mib = min(args.chunk_mib, allocate_mib - allocated_mib)
            chunk = chunk_pattern(len(chunks), current_mib * 1024 * 1024)
            chunks.append(chunk)
            allocation_digest.update(chunk)
            allocated_mib += current_mib
            if allocated_mib % 512 == 0 or allocated_mib == allocate_mib:
                print(f"ALLOC {allocated_mib} MiB", flush=True)
    except MemoryError:
        write_result(
            args.result,
            {
                "status": "FAIL",
                "reason": "memory_error",
                "allocated_mib": allocated_mib,
            },
        )
        return 1

    if not chunks:
        write_result(
            args.result,
            {
                "status": "FAIL",
                "reason": "interrupted_before_allocation",
                "allocated_mib": 0,
                "verified_chunks": 0,
            },
        )
        return 1

    checksum_before = allocation_digest.hexdigest()
    print(f"HOLD checksum={checksum_before}", flush=True)
    while not stop_requested:
        time.sleep(0.2)

    checksum_after = digest_chunks(chunks)
    status = "PASS" if checksum_after == checksum_before else "FAIL"
    write_result(
        args.result,
        {
            "status": status,
            "reason": "checksum_match" if status == "PASS" else "checksum_mismatch",
            "allocated_mib": allocated_mib,
            "verified_chunks": len(chunks),
            "checksum_before": checksum_before,
            "checksum_after": checksum_after,
        },
    )
    print(f"INTEGRITY {status}", flush=True)
    return 0 if status == "PASS" else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"integrity worker: {error}", file=sys.stderr)
        sys.exit(2)
