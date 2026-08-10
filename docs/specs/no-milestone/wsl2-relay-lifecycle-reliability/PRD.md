---
slug: wsl2-relay-lifecycle-reliability
title: "WSL2 Relay lifecycle reliability"
milestone: —
issues:
  - "microsoft/WSL#41242"
  - "microsoft/WSL#41286"
---

# PRD — WSL2 Relay lifecycle reliability

> **Status:** SSDV3 Step 1. This slice adds a RamShared operator safety surface
> for a confirmed WSL lifecycle defect. It does not replace or fork WSL.

## 1. Problem

Long-lived WSL2 sessions can retain processes whose command line is `/init`
and whose process name is exactly `Relay`. A stranded Relay repeatedly logs a
`UtilAcceptVsock` long-accept diagnostic and may retain host/guest transport
resources after its client session is gone.

The defect is upstream, not a RamShared memory-tier failure. Microsoft merged
the bounded-accept fix in `microsoft/WSL#41252`, closing
`microsoft/WSL#41242`, but the installed release may predate that merge. Until
a released WSL build containing the fix is deployed and observed cleanly,
RamShared needs a narrow diagnostic and attended recovery tool.

## 2. Product decision

Provide one safety script with two explicit modes:

- `--check`: read-only classification and sanitized JSON output;
- `--reap`: attended cleanup of only the exact candidates returned by the
  classifier, with identity revalidation before every signal.

There is no automatic or timer-driven kill mode. A timer may run `--check`
only. The tool must never call `wsl.exe --terminate`, `wsl.exe --shutdown`,
restart Docker, reboot Windows, change WSL configuration, or touch RamShared
swap tiers.

## 3. Exact candidate contract

A process is a cleanup candidate only when all conditions are true:

1. `/proc/<pid>/comm` is exactly `Relay`;
2. `/proc/<pid>/cmdline` is exactly `/init` with its terminating NUL;
3. parent PID is 1 or 2;
4. the process has no children;
5. `/run/WSL/<pid>_interop` does not exist;
6. age derived from `/proc/uptime`, `/proc/<pid>/stat` start ticks, and
   `CLK_TCK` is at least 600 seconds; and
7. the process start-tick identity remains unchanged during the operation.

Any unreadable field, malformed value, unexpected parent, live child, interop
socket, young process, PID reuse, or candidate count above the configured cap
is a refusal. The classifier never infers safety from process name alone.

## 4. Recovery contract

`--reap` performs one bounded pass:

1. capture the exact candidate PID and start-tick set;
2. revalidate every candidate;
3. send `SIGTERM` to the exact set;
4. wait at most 10 seconds without an unbounded loop;
5. revalidate each survivor, including start ticks;
6. send `SIGKILL` only to still-identical survivors;
7. verify zero classified candidates and preserve a sanitized result.

The operation stops on cardinality drift, identity drift, unreadable state, or
any validation ambiguity. It must not broaden a PID list after the initial
snapshot.

## 5. Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-1 | Detect the exact stranded-Relay signature without mutation. | `--check` reports candidates and exits non-zero when attention is required. |
| RF-2 | Refuse live, ambiguous, young, or reused processes. | Every negative classifier case is covered by a named manufactured test. |
| RF-3 | Bound attended cleanup and signal only the sealed candidate set. | TERM and KILL phases revalidate start-tick identity and complete within their deadlines. |
| RF-4 | Keep output public-safe. | Output contains counts, PIDs, ages, reason codes, and verdicts; never command contents beyond the fixed `/init` classification or environment data. |
| RF-5 | Prove the operator surface before and after cleanup. | Live evidence includes baseline, action, zero candidates, interop probe, and a quiet diagnostic window. |
| RF-6 | Track upstream adoption without claiming product ownership. | A released WSL tag containing the merged fix changes the local state to observation-only, not DONE by ancestry alone. |

## 6. Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-1 | Bash strict mode; no Python, Node, jq, or undocumented runtime dependency. |
| NFR-2 | `--check` requires no privilege; `--reap` fails closed unless signals are permitted. |
| NFR-3 | Maximum candidate count defaults to 128 and accepts only a bounded positive override. |
| NFR-4 | No physical reboot, WSL shutdown, workload pressure, swap mutation, GPU workload, external write, or automatic cleanup. |
| NFR-5 | Shell business logic is `N/A — E2E-only` for Rust slice coverage; manufactured fixtures and live before/action/after are mandatory. |
| NFR-6 | Missing live WSL evidence leaves the slice `partial`, never DONE. |

## 7. Observability

The JSON record contains schema version, UTC timestamp, mode, inspected count,
candidate count, candidate PIDs and ages, TERM survivors, KILL survivors,
refusal reason, action duration, and final verdict. It contains no usernames,
private paths, environment variables, socket addresses, command payloads,
kernel addresses, or raw journal lines.

## 8. Upstream boundary

- The local tool is a temporary reliability guard, not a WSL implementation.
- The Microsoft merge is the source fix; RamShared does not prepare a duplicate
  patch.
- Release ancestry is necessary but not sufficient for closure. After updating
  to a released build containing the fix, observe at least seven days of the
  normal operator workload with zero classified Relays and zero matching
  diagnostic increments.
- A new-session transport wedge remains a separate recovery case; this tool
  must not claim that removing stranded Relays repairs every hvsocket failure.

## 9. Rollback trigger

Disable `--reap` and retain read-only `--check` if any candidate changes
identity during cleanup, any non-candidate receives a signal, interop fails
after cleanup, the WSL instance becomes unavailable, or the cleanup exceeds 20
seconds. Preserve the sanitized record and require manual diagnosis.

## 10. Documents and files

| Path | Action |
| --- | --- |
| `scripts/safety/wsl-relay-health.sh` | create — read-only check and explicit attended cleanup |
| `scripts/safety/test-wsl-relay-health.sh` | create — manufactured proc-tree and signal seam tests |
| `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/SPEC.md` | create — executable contract |
| `docs/specs/no-milestone/wsl2-relay-lifecycle-reliability/IMPL.md` | create after validation |
| `validation.md` | append after legitimate and refusal E2E |
| `docs/INDEX.md` | regenerate |

## 11. Out of scope

- A WSL fork, duplicate upstream patch, automatic service restart, or automatic
  process kill.
- `wsl --terminate`, `wsl --shutdown`, Windows reboot, Docker restart, or
  Modern Standby policy.
- RamShared daemon, cascade, swap, ublk, GPU memory, Windows driver, or product
  performance changes.
- Claims that this lifecycle defect caused every historical WSL or RamShared
  hang.
