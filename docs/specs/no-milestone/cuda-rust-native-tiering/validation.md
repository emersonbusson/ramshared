# Validation — CUDA-Rust native tiering investigation

## Scope and status

SSDV3 Step 3 is **partial**. This record validates the existing CUDA Driver
API baseline and the correction of architectural claims; it does not validate
the proposed `cuda-core`/`cuda-async` backend, a `cutile` or `cuda-oxide`
kernel, compression, or installation of a new host binary. The current
[AUDIT-2.5.md](AUDIT-2.5.md) verdict is `no-go` for production migration.

## Local checks — 2026-09-21

| Gate | Observed result |
| :--- | :--- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy -p ramshared-block -p ramshared-agent -p ramshared-cuda --all-targets -- -D warnings` | PASS |
| `cargo test -p ramshared-cuda` | 16 unit PASS, 1 GPU unit ignored, 2 GPU integration tests ignored, 2 doctests PASS |
| `cargo test -p ramshared-block` | 95 unit PASS |
| `cargo test -p ramshared-agent` | 56 library, 16 main, 7 CLI PASS |
| `./scripts/docs-check.sh` | PASS; localization checker separately reports `PARTIAL` for translation state |
| `bash scripts/safety/wslconfig-ctl.sh selftest` | PASS; no host `.wslconfig` changed |

These tests do not exercise the proposed GPU compute path. The CUDA tests
explicitly ignored by the harness remain unqualified; a passing default
`cargo test` must not be used as evidence for them. A prior focused
`sparse_vram.rs` slice gate reported 93.1% line coverage, but there is no new
CUDA backend file against which to run a Step 3 coverage gate.

## Before → action → after

- Before: the installed host ran the older uncompressed CUDA Driver API path;
  `cuda-core`/`cuda-async` were declared but unused, and no Tile kernel or
  compressed swap representation was present.
- Action: audited dependencies and architecture, corrected PRD/SPEC and the
  2.5 verdict, and ran local static/unit checks. No package or kernel was
  installed, no swap device was detached, and no host service was replaced.
- After: the same runtime remains installed. No `BINARY_MATCH`, live GPU
  pressure/recovery, `sm_80+` Tile execution, or host replacement claim is
  made for the proposed implementation.

## Remaining gates

1. Close the ownership and crash-consistency blockers in the current 2.5
   audit, then add named tests before any production backend change.
2. Run per-file line coverage at or above 80% on each new business-logic
   file, plus fault, cancellation, and recovery tests.
3. Qualify the `sm_75` path on the local GPU and any Tile path on a separate
   `sm_80+` host with a supported toolkit, workload-specific benchmarks, and
   artifact provenance.
4. Only after controlled live before/action/after, swapoff-first recovery,
   a reproducible release build, and installed `BINARY_MATCH` may the IMPL record be
   considered DONE for a host surface.

## Verdict

**PARTIAL / NO-GO for deployment.** The existing uncompressed CUDA path is
retained. Proposed GPU compute and compression remain research work.
