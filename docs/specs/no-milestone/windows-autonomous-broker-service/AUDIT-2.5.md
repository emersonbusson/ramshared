# AUDIT-2.5 — windows-autonomous-broker-service

Audit date: 2026-07-24. Scope: adversarial SSDV3 Step 2.5 review of
[`SPEC.md`](SPEC.md) against its folder PRD, the current
`ramshared-broker`, `ramshared-wsl2d`, and `ramshared-winsvc` symbols,
Windows SCM/named-pipe privilege and cancellation boundaries, existing
StorPort teardown evidence, SSDV3, and Kahneman #2/#5/#9/#13/#15–#17.

This audit authorizes Step 3 implementation in ITEM order only. It does not
claim that the named crate, Windows APIs, tests, package, VM drills, or
physical cold-boot evidence already exist.

## Findings

| Sev | SPEC § | Issue | Required fix | Resolution |
| --- | --- | --- | --- | --- |
| CRITICAL | DT-3; pipe CREATE row; NFR-1 | The first SPEC selected blocking `ConnectNamedPipe`/read threads but did not provide a cancellation/rundown contract. SCM stop could wait forever or close a handle while an I/O operation still referenced its buffers/event. | Use overlapped accept/read/write, per-frame deadlines, `CancelIoEx`, completion observation, RAII ownership, and named blocked-accept/read stop drills. | Fixed in DT-3, security lifetime, `pipe.rs`, service tests, and the E2E matrix. |
| HIGH | Shared lease extraction; current `BrokerCore::on_lease_release` | The current Linux broker dispatches release by lease ID without binding the sending session to the holder. Preserving that shape would let another registered tenant release a foreign lease if it learned/guessed the ID. | Pass session ID/registered holder to release, enforce holder in the shared `LeaseBook`, close/refuse a foreign sender before mutation, and pair legitimate/foreign tests. | Fixed in DT-16, ITEM-2 records, Kahneman #13/#17, and named Linux tests. |
| HIGH | DT-11 | “Restart three times, then stop” did not match SCM failure-action behavior: Windows repeats the last action unless an explicit terminal `SC_ACTION_NONE` is present. SCM also cannot choose a recovery action from an arbitrary service exit-code class. | Configure restart/restart/restart/none explicitly for abnormal termination, keep non-crash failure actions disabled, and make deterministic errors exit normally with a service-specific error. | Fixed in DT-11 and `fourth_failure_remains_stopped` / `deterministic_failure_does_not_restart`. |
| HIGH | DT-8/DT-9/DT-15; package atomicity | The manifest hashed “config,” while the effective configs were described as mutable ProgramData files. A post-validation edit or torn broker/winsvc config pair could bypass the complete-manifest claim. The active-pointer rename was also overstated as crash-atomic. | Make both effective configs immutable version artifacts; include both hashes in the manifest; cross-validate tenant/capacity; independently re-hash before effects; use `ReplaceFileW` plus backup; fail closed and reconstruct only from stopped SCM paths matching one complete manifest. | Fixed in DT-8, DT-9, DT-15, package/config records, and rollback rules. |
| HIGH | DT-10; observability | Reading the latest JSONL row in `status` could present stale “Ready/Online” evidence as current health after a crash. | Separate `last_evidence` from current observations, query SCM/pipe/host live, cap the status protocol, and return `Unknown`/non-zero if any required current source fails. | Fixed in DT-10 and the status/evidence requirements. |
| MEDIUM | DT-5 | The original service-SID check did not close SID resolution type, deny-only groups, token lifetime, `RevertToSelf` on every exit, or equivalent authentication on the administrator status pipe. | Specify two-call owned SID resolution, required SID type/enabled state, RAII impersonation reversal, and paired legitimate/refusal cases for both endpoints. | Fixed in DT-5, security checklist, pipe signatures, and peer-matrix tests. |
| MEDIUM | Required tests matrix | “Existing suite” and “tests listed above” were not real test names and violated the Step 2 matrix rule. | Expand the affected rows to explicit repository-root paths and named tests. | Fixed in the Linux broker, package, and SCM/IPC matrix rows. |
| MEDIUM | DT-15 | The standalone Windows broker's logical capacity existed in a config type but its relation to LUN size/alignment and request equality was not closed. | Require exact broker/winsvc tenant and capacity equality, block-size alignment, exact lease request, and runtime config hash checks. | Fixed in DT-15 and named cross-config tests. |
| LOW | Consumer MODIFY row / ITEM-5 | One stale phrase still promised broker “instance correlation” inside the consumer although protocol v1 intentionally carries no instance field. | Keep instance identity external to status/evidence and use pipe EOF without reconnect as the runtime signal. | Fixed in the MODIFY record and ITEM-5 wording. |

## Re-audit checks

| Gate | Result |
| --- | --- |
| Scope remains local Windows broker packaging/supervision; no signing/pagefile/driver expansion | PASS |
| Exact current code anchors and contiguous ITEM-1…9 order | PASS |
| RF and code-bearing NFR traceability | PASS |
| Protocol v1 unchanged; `Register → Registered` is the same authoritative session | PASS |
| Windows peer boundary has legitimate + refusal evidence | PASS after revision |
| Pipe accept/read/write cancellation, timeout, rundown, and stop behavior | PASS after revision |
| Sender-bound lease authorization and duplicate/disconnect idempotency | PASS after revision |
| SCM failure actions match Windows repeat-last semantics | PASS after revision |
| Config/package TOCTOU and crash-recovery frontier | PASS after revision |
| Status cannot promote stale evidence to current health | PASS after revision |
| Rollback split covers userspace, Windows driver, and host-persistent state | PASS |
| Online/ambiguous states are forward-only through safe teardown | PASS |
| Critical Kahneman rows have commands, legitimate/refusal pairs, and observable aborts | PASS after revision |
| Test matrix uses real paths/names and cover/E2E rationales | PASS after revision |
| Platform gates are Windows/Rust specific; no foreign cascade/checkpatch gate | PASS |
| Physical host remains bounded, supervised, pressure-free, and VM-first | PASS |
| Environment-bound signing/VM/physical gaps remain `PARTIAL` until executed | PASS |

## Open questions

No design question blocks Step 3.

Implementation must stop and revise the same SPEC if the pinned Windows SDK or
`windows-service` crate proves any of these assumptions false:

- the `RamSharedWinSvc` unrestricted service SID is present as an enabled token
  group under LocalSystem and is usable in the named-pipe DACL;
- `CancelIoEx` plus `GetOverlappedResult` can quiesce every pipe operation
  within the specified stop budget;
- a virtual service account can create the protected pipe and write only to
  the intended evidence/Event Log locations with the specified ACL;
- `ReplaceFileW`/SCM query behavior differs from the documented rollback
  frontier on the pinned Windows build.

These are executable ITEM-3/4 VM gates, not permission to weaken identity,
fall back to TCP, run the broker as LocalSystem without a revised privilege
decision, or claim evidence from a mock.

Production signing and Secure Boot remain separate release gates. Failure to
run the Windows VM or three physical cold boots keeps implementation
`PARTIAL`.

## Verdict

Initial review: **`no-go`**.

After the mandatory in-place SPEC corrections above: **`go`** for SSDV3 Step
3 implementation in ITEM order.

The verdict becomes `no-go` again if implementation introduces blocking
uncancellable pipe I/O, a daily TCP fallback, foreign-holder release,
automatic consumer restart/reconnect, mutable unmanifested effective config,
stale-evidence health inference, forced storage teardown, or any protocol/driver
ABI change not first closed by a revised SPEC and re-audit.
