# IMPL — Microsoft-native VRAM memory tier: host-authoritative N3 RFC

## Status

**SSDV3 Step 3: `PARTIAL` / `REFUSED_HOST_CONTRACT` for host claims.** The
implemented surface is a pure Rust model. It performs no Windows, WSL, GPU,
device, driver, kernel, process, filesystem, network, or durable-storage I/O.
No host E2E or restart run exists.

| Local gate | Exact result |
| --- | --- |
| `cargo test -p ramshared-tier --all-targets` | 52 passed (shared package suite) |
| `cargo test -p ramshared-tier --test n3_state` | 26 passed |
| Named restart gate | `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL` passed |
| Refusal/atomic restore gate | `restart_record_rejects_non_host_or_truncated_input_without_partial_history` passed |
| `cargo clippy -p ramshared-tier --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |
| `n3_state.rs` line coverage | 86.2% (922/1069), minimum 80% pass |

## Implemented local ownership

| PRD/SPEC item | Exact local path and evidence |
| --- | --- |
| Pure N3 protocol model | `crates/ramshared-tier/src/n3_state.rs` |
| Module export glue | `crates/ramshared-tier/src/lib.rs` under the existing N3/NBD differential owner |
| Protocol/refusal integration tests | `crates/ramshared-tier/tests/n3_state.rs` (26 tests) |
| Durable restart input | `RestartRecord` and `GenerationCheckpoint`: bounded canonical `RSN3` bytes, schema, host marker, non-zero epoch, and ordered opaque lease/generation checkpoints |
| Fresh-process stale refusal | A new `LeaseMachine` restores the record, rejects generation 1 for the restored lease, and accepts only generation 2 in the named test |

`RestartRecord` is useful host-authoritative input, not a host adapter: callers
own durable acquisition and authentication. Decode validates the entire bounded
record before `LeaseMachine` replaces generation history, so malformed input
cannot partially seed a fresh model.

## Evidence matrix and open gaps

| Required evidence | Local result | Live/E2E result | Status |
| --- | --- | --- | --- |
| `N3_RUST_DURABLE_RESTART_GENERATION_REFUSAL` | Pure fresh-model serialization/restore refusal passed | Not a WSL restart | `PARTIAL` local semantic evidence |
| `N3_HOST_WSL_RESTART_RECOVERY` | No host adapter or restart execution | Not run | `PARTIAL` / environment-bound |
| `N3_HOST_BUDGET_REVOKE_DRAIN` | Pure transitions only | Not run | `PARTIAL` / `REFUSED_HOST_CONTRACT` |
| GPU reset/TDR, suspend, zeroing, owner/public-contract rows | Pure refusal/fallback only | Not run | `REFUSED_HOST_CONTRACT` or `PARTIAL` per SPEC |
| `BINARY_MATCH` | **N/A / not run**; N3 has no daemon/binary host boundary | Not run | `N/A` |

Open gaps are a versioned public host contract, host authentication and durable
restart-record acquisition, a host adapter, isolated host/GPU evidence, and an
approved before → action → after restart/revoke test. Root `validation.md` is
intentionally not updated.

## Numeric rollback trigger

Fail closed and retain prior generation history if serialized restart input is
larger than **18,705 bytes**, has more than **256** checkpoints, zero epoch or
generation, non-host authority, unknown schema, malformed length, or unordered
or duplicate lease identity. For a restored lease, any grant with
`generation <= last_generation`, a generation gap, or `host_epoch <
restored_epoch` is refused. These numeric local safeguards do not establish
host durability or residency.

## Traceability

`PRD.md` RF-N3-1..11 and NFR-N3-1..8 → `SPEC.md` ITEM-1..6 / DT-N3-11 → this
explicit partial record.
