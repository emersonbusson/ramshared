# IMPL — WSL2 NBD-only product readiness

## Status

**SSDV3 Step 3: `PARTIAL` (source/static/manufactured plus one supervised
live WSL2 NBD pilot).** The 1 GiB NBD lifecycle, Relay before/action/after,
daemon identity, device, and swap ordering are live-verified. The required
1/2/4 GiB benchmark matrix remains unrun, so this is not DONE.

| Local gate | Exact result |
| --- | --- |
| `cargo test -p ramshared-tier --all-targets` | 52 passed (shared package suite) |
| `cargo test -p ramshared-cli --all-targets` | 95 passed (90 unit + 5 integration) |
| Exact CLI lifecycle contracts | 2 passed: `swapoff_completes_before_nbd_disconnect`, `failed_swapoff_keeps_daemon_and_device_alive` |
| `bash scripts/safety/test-nbd-product-preflight.sh` | 22 named suites passed, including one aggregate that injects all 20 post-write rollback phases and exact legacy-unit migration/backup refusal coverage |
| `cargo clippy -p ramshared-tier --all-targets -- -D warnings` | pass |
| `cargo clippy -p ramshared-cli --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |
| `nbd_readiness.rs` line coverage | 83.9% (251/299), minimum 80% pass |
| `cascade_io.rs` line coverage | 85.5% (1255/1467), minimum 80% pass |

## Implemented local ownership

| PRD/SPEC item | Exact local path and evidence |
| --- | --- |
| Pure NBD readiness/capacity/refusal model | `crates/ramshared-tier/src/nbd_readiness.rs`; exact 80% gate above |
| NBD transport integration | `crates/ramshared-tier/src/cascade.rs`; covered by the shared tier suite |
| Real teardown executor | `crates/ramshared-cli/src/cascade/cascade_io.rs`; injected `NbdLifecyclePlan` / `NbdLifecycleExecutor` proves every `swapoff` precedes NBD disconnect and daemon stop |
| Failed swapoff refusal | `failed_swapoff_keeps_daemon_and_device_alive` records only `swapoff`; it records neither disconnect nor daemon stop |
| Installer rollback | `scripts/safety/install-cascade-boot.sh` has 20 named post-write markers; `scripts/safety/test-nbd-product-preflight.sh` injects a failure after each marker in a temporary fixture |
| Legacy unit migration | A conflicting unit remains a refusal unless a second approval supplies its exact SHA-256. The manufactured migration test proves absent/stale approvals do not mutate it; an injected final-phase failure restores the old unit from a sealed backup. |
| Read-only product preflight | `scripts/safety/nbd-product-preflight.sh` and the same 22-suite manufactured harness |
| Live 1 GiB activation | `evidence/2026-08-12-live/{before.txt,action-sudo.txt,after-active.txt,binary-match.txt}`; `/dev/zram0` priority 200, `/dev/nbd0` priority 100, disk priority -2, Relay clean, and `BINARY_MATCH=PASS` |

## Evidence matrix and open gaps

| Required evidence | Local result | Live/E2E result | Status |
| --- | --- | --- | --- |
| `nbd_lifecycle_before_action_after` | Pure injected ordering/refusal | Live 1 GiB pilot passed: before disk-only, action created zram/NBD, after priorities were 200/100/-2 with no ghost | `PARTIAL` / benchmark matrix remains |
| `relay_gate_before_action_after` | Read-only manufactured refusal | Live Relay checks were `CLEAN` before and after, with zero candidates | `PASS` for the 1 GiB pilot |
| `NBD_BENCHMARK_MATRIX` | Schema only | No 1/2/4 GiB cells or n≥3 statistics | `PARTIAL` / environment-bound |
| `BINARY_MATCH` | Static stale/deleted daemon refusal | Live `ramsharedd` executable and selected sealed binary both resolved to `/opt/ramshared/releases/v0.8.0-8-g0b09518/bin/ramsharedd` | `PASS` for the 1 GiB pilot |
| Sealed installer transaction | 20-phase rollback and legacy-unit migration manufactured tests | Attended install migrated the approved legacy unit, retained a root-owned immutable backup, selected `v0.8.0-8-g0b09518`, and kept the service disabled | `PASS` for the installed release |

The open gap is the ordered 1/2/4 GiB benchmark matrix with n>=3 and
median/p99/deviation. `validation.md` records the supervised 1 GiB
before → action → after evidence; no benchmark claim is inferred from it.

## Numeric rollback trigger

Rollback/refusal is mandatory when **any one** (`>= 1`) of these occurs:
`swapoff` returns an error; an active NBD/ublk swap entry remains; any one of
the **20/20** manufactured post-write rollback phases fails to restore prior
selector/unit state and remove the destination; a legacy-unit migration hash,
metadata, backup, or restoration check fails; or lower-tier capacity is
`L < V + max(ceil(0.10 × V), 512 MiB)`. The local executor returns before NBD
disconnect and daemon stop after a failed `swapoff`; a live operator must not
infer that this source evidence authorizes teardown.

## Traceability

`PRD.md` RF-NBD-1..13 → `SPEC.md` ITEM-1..8 → this explicit partial record.
