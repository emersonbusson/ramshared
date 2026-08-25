# AUDIT-2.5 — wsl2-custom-kernel-p1

> **Date:** 2026-07-10  
> **Object:** [`SPEC.md`](SPEC.md) before first live `apply`  
> **Historical verdict (2026-07-10):** GO for the recorded capability exercise.
> **Superseding verdict (2026-08-23): CURRENT LIVE PROMOTION NO-GO.** The
> observed DXG FORTIFY warning and repeated distro init timeout require a fresh
> same-host bundled/custom A/B canary. `uname` and qemu stamps are insufficient.

## 2026-08-24 KFA-R5-01..KFA-R5-10 independent disposition

The prior R5 hermetic GREEN is invalidated. R6 found an independent
start-before-Job race, unsafe numeric-PID termination, cleanup replacement
windows, rollback capture ordering, incomplete end-to-end deadline custody,
lexical path gaps, a reachable module-loader path, non-independent test modes,
non-global lock identity, and an ambiguous sealer identity parser.

The source candidate now creates each Windows child suspended, configures and
assigns a kill-on-close Job before resume, retains native handles and creation
identity, drains bounded pipes concurrently, and checks assignment,
termination, waits, and handle closure. Assignment or resume failure terminates
and reaps the never-resumed root by handle. `taskkill`, numeric-PID termination,
and recursive cleanup are prohibited. Windows cleanup is retained-handle and
file-ID bound; shell cleanup operates only on fixed single-link leaves under a
held identity and leaks safely when ownership changes.

Exact originals are captured after the canonical `Global\` lock and before
fallible validation. Restore writes are limited to attempted mutations after
capture completes. Lexical validation rejects DOS device stems even with
extensions, trailing dot/space, control/invalid characters, namespaces, UNC,
ADS, traversal, 8.3, reparse, and case-collision inputs before path probing.
Hashing, parsing, locking, filesystem work, and transaction work share one
monotonic deadline. A pre-child expiry proves `NO_CHILD_CREATED`; a blocked
hash seam proves checked Job termination, `JOB_EMPTY`, and retained root
creation identity. `enable` is unconditionally inert and has no module-loader
or privilege-elevation path.

The four exact SPEC test modes are independently selectable. Expanded tests
cover assignment/resume/termination seams, root-creation mismatch,
inherited-pipe descendants, slow startup and blocked hashing,
capture-before-validation rollback, lock concurrency, replacement sentinels,
hostile paths, exact four-field sealer identity, and installer gate recognition
without copy/effect. The full R6 PowerShell 5.1 suite has passed once; the
independently selected final mode reruns and shell gates remain pending, so this
audit does not yet claim final R6 completion.

Current decision remains **OVERALL PARTIAL; LIVE-NO-GO**. Direct attended
installer, wrapper, and launcher invocation stops on suspended-custody/
handle-execution uncertainty before filesystem effects; attended shell `apply`
stops separately on unverified module-to-VHDX provenance. These are independent
blockers, and neither source/static evidence nor a hermetic pass grants live
authority.

## 2026-08-24 KFA-01..KFA-12 disposition (R5 evidence; superseded by R6)

The transaction review found twelve independent source gaps. The remediation
is source/hermetic only and does not authorize live promotion:

- preflight has no live runtime fallback and requires a confined runtime
  fixture before deployment or manifest access;
- every fixture path is nonce-bound below one canonical fresh Windows temp
  root, and fixture capability is mutually exclusive with any bound live
  authority;
- a canonical named mutex is held before transaction mutation; receipt-root
  creation, snapshots, configs, receipts, and cleanup remain in exact rollback
  custody across every injected boundary;
- manifest, artifact hashes and filesystem identities, target config, and lock
  identity are revalidated under the lock before READY;
- redirected children have concurrent capped drains, end-to-end deadlines,
  process-tree termination, and bounded post-termination waits;
- installer staging copies from held single-link source handles and verifies
  identity/hash/length, without claiming an immutable Windows execution handle;
- shell parsing refuses duplicate/unknown/missing/blank forms; R6 later makes
  module loading unconditionally inert and removes the loader entirely;
- atomic writes retry only classified sharing violations, retain the original
  backup through replacement readback, and restore exact bytes or absence;
- sealer publication is race-safe/no-replace with sealed permissions and link
  identity, but explicitly reports promotion ineligible because cryptographic
  module-to-VHDX containment is unverified;
- the four exact SPEC test identifiers are executable markers with internal
  child watchdogs and an outer direct-suite watchdog.

The then-final R5 hermetic evidence was GREEN: the Windows PowerShell 5.1
suite returned all three PowerShell SPEC markers and its suite PASS; direct
execution of the shell suite returned the fourth marker and suite PASS; shell
syntax, candidate public hygiene, and the scoped diff check also passed.
These results exercised only confined fixture state and did not constitute
live promotion evidence. The R6 findings invalidate them as current completion
evidence.

Therefore `apply`, `arm`, the live wrapper, and the live launcher now refuse
before installer/log/receipt/config/WSL effects even when attended credentials
are exact. `disarm` is also mutation-inert pending the reviewed transaction
path. The direct installer is no longer a live copy entry point: exact
credentials reach `STAGING_CUSTODY=NO_GO` before filesystem probing or copy.
Its confined fixture mechanics are not promotion authority.

## 2026-08-23 remediation disposition (superseded evidence record)

The independent pre-commit failures are corrected in source but do not
authorize live promotion:

- the then-proposed apply path installed and hash-bound the reviewed
  wrapper/launcher and exact kernel/modules/layout/QEMU bundle before WSL
  shutdown; the current path refuses before installation on provenance;
- Windows PowerShell 5.1 parses and executes the real installed
  wrapper-to-launcher fixture chain;
- mutable `latest` and stale June clean-config authorities are removed from the
  executable path;
- arm, disarm, and rollback treat `kernel=` plus `kernelModules=` atomically;
- rollback requires a third fresh boot with the exact bundled-baseline kernel
  and host-visible identities, not only pair-key removal;
- WSL 2.7.12, 6.18.40.1 unified layout, and double nesting fail closed;
- baseline/candidate receipts bind boot, host/runtime/driver/distro/dmesg/DXG
  evidence and an actual bounded WSLg display transaction;
- `READY` revalidates the exact current boot, pair, configuration, hashes,
  layout, release, module tree, and vermagic;
- a getty failure requires an explicit exact WSL-only exception and is never
  ignored by default.
- all three direct PowerShell entry points refuse before mutation unless both
  `-Run` and their exact case-sensitive effect token are present;
- the wrapper creates no log before its gate, and only the attended shell
  `apply` branch forwards installer/promotion gates;
- 22 direct-entry refusal cases preserve sentinel hashes and path-state
  readback: the 21-entry direct-gate matrix and one implicit-log-root case.
  Exact-gate and fixture paths PASS without WSL or host effects.
- the KERNEL-FIXTURE-AUTHORITY-001 follow-up adds 80 executable refusals over
  every fixture parameter standalone, all unordered pairs, combined/conflict
  shapes, credential variants, and companion-only inputs across launcher and
  wrapper, including the two earlier representative fixture cases. The total
  suite has 102 distinct refusal scenarios. Supplied-live-authority refusal
  precedes manifest/deployment access and every mutation; companion-only
  refusal precedes launcher-manifest/WSL access and creates no wrapper log.
  Legitimate fixture forwarding still passes without live authority.

Current decision: **OVERALL PARTIAL; LIVE-NO-GO.** Source/hermetic GREEN must
remain separate from promotion authorization. Cryptographic module-to-VHDX
containment is the first blocker; a separately attended A/B window comes only
after that design is reviewed.

---

## Findings by severity

### CRITICAL

| ID | Finding | Disposition |
| --- | --- | --- |
| C1 | Current 6.18.35.2 boot emits a DXG field-spanning-write warning in the Xwayland GPU wait path | Upstream-open/confounded because 6.18.26.1 and bundled 6.18.33.2-2 reproduce it; nevertheless block promotion until the fresh candidate has zero warning |
| C2 | Isolated build-distro attempts fail `/sbin/init` within 10 s and leave unclean journal/p9 evidence | Root cause unresolved; require systemd `running` and zero fresh init/unclean/p9 signals |

### HIGH

| ID | Finding | Disposition |
| --- | --- | --- |
| H1 | `apply` kills every WSL session | Accept with flag `--i-know-this-stops-all-wsl` + auto-revert (boot-kernel-safe) |
| H2 | Module insmod failed in qemu busybox initramfs | **Accepted historical residual only** — qemu-validate documents best-effort modules. Runtime module loading is not authorized; `enable` is unconditionally inert. A stamp alone is never promotion authority. |
| H3 | `uname -r` of custom equals stock-style string `…microsoft-standard-WSL2+` | Detection uses **exact REL from release.txt** after boot; before reboot NEED_REBOOT when armed. Risk of false READY if stock REL ever equals custom — low; document in IMPL. |
| H4 | Existing launcher retained a kernel after `uname` even when post-boot module or control-plane checks failed | Corrected: exact-distro systemd/DXG/log/module-metadata verdict owns confirmation; uncertainty disarms |
| H5 | Issue #41093 proposes an unaccepted DXG patch | Do not carry or apply it automatically; accepted upstream release plus live canary remains required |
| H6 | Direct installer/wrapper/launcher invocations could mutate install/log/receipt/config state without an exact attended gate | Corrected before each mutation frontier; paired refusal/PASS fixtures execute under Windows PowerShell 5.1 |
| H7 | Runtime, baseline, and rollback fixture parameters were omitted from the non-live classifier and could advance with exact live credentials | Corrected in wrapper and launcher using all seven bound fixture parameters; companion-only inputs terminate before launcher-manifest/WSL access while immutable wrapper deployment verification stays log-free |
| H8 | Fixture/preflight paths, transaction mutation, process trees, and publication had independent authority, rollback, deadline, TOCTOU, and parser gaps (KFA-01..KFA-12) | R5 remediation was independently incomplete; R6 adds suspended-before-resume custody, handle-bound cleanup, capture-before-validation, stricter paths/deadlines, Global locks, exact identity parsing, and separate live NO-GO boundaries. Final selected-mode reruns are pending. |

### MEDIUM

| ID | Finding | Disposition |
| --- | --- | --- |
| M1 | Interop flaky (binfmt) | CLI fail-fast plus validated config/user inputs; arm is currently mutation-inert |
| M2 | `modules_install` completion was not independently established in the isolated build environment | Historical module copy is not promotion authority; cryptographic containment remains required |

### LOW

| ID | Finding | Disposition |
| --- | --- | --- |
| L1 | enable on stock with ublk=loaded (host already has something) | State machine still NEED_BUILD until custom bzImage + match |

---

## Kahneman map (apply path)

| # | Check | Status |
| --- | --- | --- |
| #2 | Rollback: any canary failure → disarm | Present in boot-kernel-safe; timeout is only one trigger |
| #13 | Boot PASS ≠ systemd/DXG/module PASS; refusal ≠ safety proof alone | Full canary remains mandatory; 22 direct-entry and 80 fixture-authority refusals pair with legitimate fixture/install/chain PASS paths |
| #16 | Direct entry points default safe | PLAN/refusal and fixture-authority refusal precede manifest, deployment, install, log, receipt, config, and lifecycle mutation frontiers |
| #17 | arm/enable idempotent | arm is mutation-inert; enable is unconditionally inert and performs no module load |
| #15 | No blind retry/module-loader loop | Implemented by removing the loader path from `enable` |

---

## Open questions

1. Capture a fresh same-host bundled-kernel baseline without RamShared or
   pressure activation.
2. Run the candidate only through the bounded auto-reverting canary under
   separate attended approval.
3. Keep ublk and product transport qualification separate even if the kernel
   canary passes.

---

## go / no-go

| Path | Decision |
| --- | --- |
| status | **go** (read-only) |
| disarm / enable / arm / natural candidate reboot | **no-go**; mutation/enablement remains blocked |
| qemu-pass stamp as sole apply gate | **no-go** |
| Agent or human runs `apply` now | **no-go**; separate attended approval and bundled baseline are missing |

**Blockers fixed in source:** fail-closed canary and unsafe natural-arm refusal.
**Environment-bound blocker:** fresh bundled/custom A/B remains open.
**Historical stamp:** `<historical-output-root>/qemu-pass.stamp` (2026-07-10).
**Live (2026-07-10 evening):** kernel + `kernelModules` VHDX + `ublk_drv` + cascade NBD smoke GREEN. See `IMPL.md` + `validation.md`.
