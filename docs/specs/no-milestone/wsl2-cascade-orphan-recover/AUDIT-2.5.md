# AUDIT-2.5 — Cascade Fail-Closed Lifecycle

> Step 2.5 SSDV3 + privileged surface safety review.
> Revision: 2026-08-23.
> Scope: enumeration, swapoff, reset, disconnect, and backend stop.

## Decision

| Path | Verdict |
| --- | --- |
| Live enumeration of NBD/ublk/zram | **GO for detection only** |
| Mutation by name, shape, allowlist, or `used_kb == 0` | **NO-GO** |
| Mutation with exact, sealed, and revalidated lifecycle binding | **Source GO; live gates pending** |
| Swapoff with unreadable, malformed, or uncertain snapshot | **NO-GO** |
| Reset/disconnect/delete without fresh proof of absence | **NO-GO** |
| Standalone ublk on WSL2, with or without override variable | **Permanent NO-GO** |

The audit of 2026-07-10 was superseded. Zero usage does not imply inactivity:
a row present in `/proc/swaps` remains an active swap.

## Threat Model

| ID | Failure | Risk | Mandatory Control |
| --- | --- | --- | --- |
| A1 | Foreign device reuses an expected name | Critical | Exact binding + cardinality + kernel identity |
| A2 | Parser interprets uncertainty as absence | Critical | Strict parser returns `Result`; error preserves all |
| A3 | State changes between plan and action | Critical | Reauthorization immediately before each mutation |
| A4 | `swapoff` fails or yields ambiguous outcome | Critical | Fresh strict snapshot; absence not presumed |
| A5 | Backend dies with active swap, including zero usage | Critical | Swapoff-first under lifecycle owner responsibility |
| A6 | Auxiliary record diverges from binding | High | Divergence blocks entire operation |
| A7 | Duplicate or unexpected cardinality | High | Refuse all and execute zero commands |
| A8 | Evidence erased on partial failure | High | Binding, records, and backend remain recoverable |

## Minimum Authority for Mutation

An action requires, simultaneously:

1. Exact schema binding and sealed file;
2. Daemon boot ID, InvocationID, PID, and start identity;
3. Socket/export identity;
4. Origin PARTUUID, PTUUID, `dev_t`, swap UUID, and hashes;
5. Exact set and cardinality of devices;
6. Equality with stable auxiliary records;
7. Live enumeration free of foreign or ambiguous devices;
8. Fresh revalidation immediately before action;
9. Before reset/disconnect/delete, strict snapshot proving exact absence.

Failure of any single item invalidates the entire authority. There is no
partial recovery based solely on items that passed.

## Kahneman

| # | Application |
| --- | --- |
| #13 | Fixtures prove refusal and zero commands, not only happy path |
| #15 | First error terminates sequence; no retry hiding uncertainty |
| #16 | Safe default is preserving foreign, ambiguous, and active zero-use swap |
| #17 | State is removed only after complete terminal success |
| #18 | Controller owning the lifecycle also owns swapoff-first |

## Mandatory Hermetic Evidence

- Foreign, duplicate, ambiguous cardinality, and divergent record execute zero commands;
- Unreadable or malformed snapshot refuses before mutation;
- Active zero-use swap and swapoff failure preserve backend and evidence;
- Uncertain swapoff outcome requires fresh strict proof of absence;
- Valid order is swapoff, fresh absence, reset/disconnect/delete;
- Standalone ublk TERM/Ctrl-C preserves backend when swapoff is not proven;
- No test invokes real mkswap, swapoff, NBD, ublk, or zram.

## Residual Risk and Gate

Source review does not prove kernel behavior or real hot-unplug. The
live gate may only occur in a disposable, isolated environment with a
fixture device crafted for the trial and terminal detach evidence. The daily host
and production WSL2 remain out of scope. Until that gate, the status is
**Source GO / Live Activation NO-GO**.
