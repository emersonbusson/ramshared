# WSL2 freeze-class QEMU campaign — 2026-07-16

## Verdict

| Gate | Status | Evidence |
| --- | --- | --- |
| Disposable isolated surface | **PASS** | Transient diskless QEMU under an external timeout; the daily WSL2 swap and cascade were not changed. |
| Legitimate NBD lifecycle, two runs | **PASS** | Both runs activated NBD swap, completed swapoff with zero NBD entries left, and terminated the daemon afterward. |
| Forced ublk-daemon loss containment, two runs | **PASS for QEMU containment** | Both victims exited through contained SIGBUS while PID 1 and the bystander remained responsive. |
| Cascade refusal/legitimate policy tests, two runs | **PASS** | 42 tests passed twice with zero failures. |
| In-guest `BINARY_MATCH` | **BLOCKED** | The harness copied the freshly built daemon, but did not capture `readlink /proc/$pid/exe` or an in-guest binary hash. |
| Universal “WSL2 freezes are fixed” claim | **BLOCKED** | QEMU used a WSL kernel but is not the real WSL2 utility VM and did not exercise functional GPU-PV/dxg or host reclaim. |

## Fixed inputs

- Guest kernel in all recorded runs: `6.6.123.2-microsoft-standard-WSL2+`.
- QEMU acceleration: TCG.
- Built `target/debug/ramsharedd` SHA-256:
  `f03c1f1eb9e44843234f71246b77a5b6c12635f7f35bfc25d91b500075f257dd`.
- `cargo build -p ramshared-wsl2d -p ramshared-agent`: exit `0`.

## Before → action → after: legitimate NBD path

Command for each repetition:

```text
./scripts/kernel/qemu-broker-drill.sh
```

| Observation | Run 1 | Run 2 |
| --- | ---: | ---: |
| Process exit | 0 | 0 |
| Wall time | 19.37 s | 16.47 s |
| NBD swaps observed | 1 | 1 |
| Swap active | `ok` | `ok` |
| Telemetry | `ok` | `ok` |
| NBD swaps left after swapoff | 0 | 0 |
| Swapoff | `ok` | `ok` |
| Daemon terminated after swapoff | `ok` | `ok` |

This is the paired legitimate path required by Kahneman #13. It demonstrates
swapoff-first cleanup in the isolated NBD guest. Repetition from a fresh transient
guest produced the same unique terminal state: zero NBD swap entries.

## Before → action → after: forced ublk-daemon loss

Command for each repetition:

```text
./scripts/kernel/qemu-ublk-crash-e1b.sh
```

| Observation | Run 1 | Run 2 |
| --- | ---: | ---: |
| Process exit | 0 | 0 |
| Wall time | 18.69 s | 21.93 s |
| Swap used before daemon loss | 57,344 KiB | 57,088 KiB |
| Device disappearance latency | 130 ms | 5,050 ms |
| Victim exit | 42 (caught SIGBUS) | 42 (caught SIGBUS) |
| Bystander heartbeat | 41 → 45 | 41 → 69 |
| PID 1 alive | yes | yes |
| Kernel panic | 0 | 0 |
| `Attempted to kill init` | 0 | 0 |
| `hung_task` | 0 | 0 |
| `blocked for more than` | 0 | 0 |
| Swap read errors | 5 | 5 |
| Ghost swap after forced loss | 512 KiB used | 256 KiB used |
| Verdict | `CONTAINED-SIGBUS` | `CONTAINED-SIGBUS` |

The forced-loss action deliberately bypasses the product invariant inside the
disposable guest. The resulting `(deleted)` swap with `used_kB > 0` is evidence
that forced daemon termination remains unsafe. The valid product behavior is to
refuse that action and preserve swapoff-first ordering; this result must not be
reported as safe cleanup or as elimination of the failure mode.

## Refusal and legitimate policy pair

Command repeated twice:

```text
cargo test -p ramshared-cli cascade::tests -- --nocapture
```

Both repetitions returned exit `0`: `42 passed`, `0 failed`, `22 filtered out`.
The executed set included active/ghost daemon-kill refusal, ghost-with-pages
swapoff refusal, dirty orphan refusal, clean daemon-kill allowance, zero-used
orphan recovery, healthy-path recognition, and explicit ublk refusal on WSL.

## Safety boundary and remaining proof

No reboot, download, commit, merge, host swap mutation, or live-host pressure was
performed. Each QEMU guest was transient and diskless and powered off after its
bounded run.

## Harness update — 2026-07-17

The current `qemu-broker-drill.sh` and `qemu-ublk-daemon.sh` harnesses now verify
in-guest SHA-256 against the host binary copied into the initramfs.

Fresh single-run observations:

- `qemu-broker-drill.sh`: `KTEST-DAEMON-BINARY-MATCH=ok`,
  `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`,
  `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`,
  `KTEST-DAEMON-TERMINATED=ok`.
- `qemu-ublk-daemon.sh`: `KTEST-BINARY-MATCH=ok`, `KTEST-SERVED=ok`,
  `KTEST-TERMINATED=ok`, `KTEST-DEVICE-REMOVED=ok`.

This closes the harness-level binary-match gap for those current drills. It
does not promote the universal WSL2 freeze claim, which still requires a
disposable surface with functional GPU-PV/dxg and host reclaim.

This campaign proves repeatable NBD swapoff-first lifecycle behavior and containment
of the ublk forced-loss class in this QEMU environment. It does **not** prove the
absence of freezes in the real WSL2 utility VM. Promotion requires a disposable
surface that reproduces WSL2 integration with functional GPU-PV/dxg and host reclaim,
plus in-guest `BINARY_MATCH`, without placing the daily host at risk. Until then, the
universal WSL2-freeze claim remains **BLOCKED**.
