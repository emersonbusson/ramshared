# IMPL — wsl2-custom-kernel-p1

> **Passo 3 SSDV3.** Implements [`SPEC.md`](SPEC.md).  
> **Date:** 2026-07-10  
> **Status:** **HISTORICAL CAPABILITY ONLY / CURRENT LIVE PROMOTION NO-GO** — build,
> qemu, modules.vhdx, live kernel, ublk capability, cascade NBD smoke, and CLI
> were green for the recorded 2026-07-10 lab state. This does **not** close
> current kernel, DXG, systemd, or ublk transport readiness. Product cascade transport policy is
> closed in `cascade-transport-policy` as NBD Day-1; ublk remains deferred to
> `custom-kernel-ublk-product-transport`.

## 2026-08-24 R6 suspended-custody remediation

Current implementation status is **PARTIAL / LIVE PROMOTION NO-GO**. The R5
functional GREEN below is historical and invalidated by the independent R6
findings. One complete PowerShell 5.1 functional run and the focused custody,
confinement, and deadline probes are GREEN; independently selected final modes
and remaining shell gates are still pending.

| R6 finding | Candidate implementation | Current evidence |
| --- | --- | --- |
| KFA-R5-01 / 02 | PS5-compatible `CreateProcessW(CREATE_SUSPENDED)` helper configures a kill-on-close Job, assigns before resume, retains process/thread/Job and creation identity, drains capped pipes, checks native returns, terminates/reaps by handle, and has injected assignment/resume/termination/creation-mismatch seams; `taskkill` and numeric-PID termination are absent | Focused custody 5/5 and one full PS5 run GREEN; final selected-mode reruns pending |
| KFA-R5-03 | Windows deletion uses retained handles, volume/file IDs, reparse and link checks, and handle disposition; shell cleanup permits fixed leaves under a held identity and leaks on uncertainty; recursive cleanup is absent | Bash syntax and R6 static shell GREEN; replacement matrix pending full rerun |
| KFA-R5-04 | exact originals are captured immediately after the canonical lock and before fallible validation; capture/mutation flags prevent early-validation restore writes | 24-case present/absent failure-boundary matrix GREEN in one full run; selected-mode reruns pending |
| KFA-R5-05 / 07 | direct fixture work is re-executed under suspended-Job custody; chunked bounded hashing and command-wide monotonic deadlines include state resolution; `enable` is unconditionally inert with no module loader | Pre-child/no-child and blocked-hash/empty-Job focused proofs GREEN; Bash syntax and R6 static shell GREEN |
| KFA-R5-06 | lexical rejection precedes file/path probing and covers DOS device stems with extensions, trailing dot/space, controls/invalid characters, ADS, namespace/UNC, traversal, 8.3, reparse, and case collision | Hostile-path matrix GREEN in one full run; selected-mode reruns pending |
| KFA-R5-08 | all four exact SPEC identifiers are independently selectable; installer exact-gate fixture reaches `STAGING_CUSTODY=NO_GO` without copy/effect | Source mapping complete; selectable functional rerun pending |
| KFA-R5-09 | transaction and installer use canonical `Global\` mutexes and revalidate identity before READY; direct live paths remain custody/provenance blocked | Concurrent winner/refusal matrix GREEN in one full run; selected-mode reruns pending |
| KFA-R5-10 | sealer parses exactly `device:inode:links:size`, requires four numeric fields and `links == 1`, and rejects the inode/link-count adversary | Bash syntax and R6 static shell GREEN; full publication rerun pending |

Exact direct live credentials now recognize their gate but confer no mutation
authority. The installer returns `STAGING_CUSTODY=NO_GO`; the wrapper and
launcher return `LIVE_PROMOTION=NO_GO`; attended shell `apply` separately
returns its provenance refusal. These boundaries precede filesystem probing,
copy, logging, receipts, configuration, child execution, and WSL lifecycle use.
No live execution fallback exists.

The suspended helper deliberately does not claim immutable Windows
handle-bound execution by the platform launcher. That unresolved guarantee and
cryptographic module-to-VHDX provenance are independent live blockers. No
source/static result closes either blocker.

## 2026-08-24 transaction/provenance remediation (R5 evidence; superseded by R6)

Current implementation status is **PARTIAL / LIVE PROMOTION NO-GO**. This
section supersedes the 2026-08-23 source checkpoint below.

The shell sealer can publish a race-safe, no-replace qualification bundle, but
it now emits
`RAMSHARED_PROMOTION_ELIGIBILITY=REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED`.
The present manifest binds a module's metadata and the VHDX hash separately;
it does not cryptographically prove that those exact module bytes are inside
that exact VHDX. Accordingly, attended `apply` validates its pair and then
returns `LIVE_PROMOTION=NO_GO` before PowerShell, install, log, config, or WSL
lifecycle use. `arm` and `disarm` are also mutation-inert. The promotion
wrapper and launcher now independently return the later R6 suspended-custody
refusal before deployment probing, logging, receipt/config writes, child
execution, or `wsl.exe`.

The remediation maps KFA-01 through KFA-12 as follows:

| Finding | Implementation |
| --- | --- |
| KFA-01 | `PreflightOnly` requires a confined, non-blank runtime fixture and has no `wsl.exe` fallback |
| KFA-02 / KFA-12 | nonce-bound canonical `%TEMP%` root; every fixture input/output confined; UNC/device/ADS/short-name/traversal/case/hard-link/reparse forms refused |
| KFA-03 | canonical transaction context and mutex precede mutation; receipt root, snapshot, config, receipts, and cleanup stay in rollback custody |
| KFA-04 | 300-second end-to-end deadline, per-child bounds, Windows job termination, concurrent drains, 1 MiB cap, and bounded post-kill waits |
| KFA-05 | canonical named mutex with bounded/abandoned-owner-safe acquisition; manifest, hashes, filesystem identities, config, and lock identity revalidated before READY |
| KFA-06 | installer holds a single-link source handle while copying into invocation-owned staging and verifies identity/hash/length; no immutable handle-execution claim is made |
| KFA-07 | superseded by R6: `enable` is unconditionally inert and contains no module loader |
| KFA-08 | duplicate manifest/approval, unknown, missing, and blank arguments refuse before effects |
| KFA-09 | only native sharing violations 32/33 retry before replacement; backup retained through replacement readback; exact restoration on failure |
| KFA-10 | race-safe no-replace sealer publication, sealed modes/identity, explicit promotion-ineligible provenance result |
| KFA-11 | four exact SPEC test markers plus per-child internal watchdogs and a direct-suite outer watchdog |

The R5 PowerShell suite included present/absent state at every injected mutation
boundary, atomic pre/post-replace failure, post-check mutation, concurrent
mutex refusal, hard-link/reparse/outside-root probes, descendant-held pipe
timeout, exact sentinel readback, and legitimate hermetic paths. The executable
shell suite included duplicate parser probes, inert enable behavior, concurrent
sealer publication, exact
permissions/link counts, apply provenance refusal, and the apply-only gate
mapping. These are E2E-only PowerShell/shell checks; Rust coverage is N/A.
The final bounded R5 runs returned exit 0 for all four named SPEC
mappings, the Windows PowerShell 5.1 parser and fixture suite, direct
executable shell invocation, shell syntax, candidate public hygiene, and the
scoped diff check. The PowerShell matrix preserved at least the prior 102
distinct refusal scenarios. R6 invalidates that completion claim until its
expanded functional suite passes under PowerShell 5.1 and the direct shell
suite is rerun.

No source/static result authorizes a live install, promotion, kernel switch,
WSL lifecycle call, module load, or host configuration change. A reviewed
Windows handle-execution custody, a reviewed cryptographic module-to-VHDX
containment design, and a later separately attended environment-bound campaign
remain prerequisites.

## 2026-08-23 immutable-pair source checkpoint (superseded)

The current dirty candidate replaces the mutable historical activation path
with a sealed pair and a versioned host-installed launcher bundle. The normal
apply chain validates and installs the reviewed wrapper, launcher, kernel,
modules VHDX, layout inventory, and QEMU receipt before shutdown; after that
point it has no dependency on the repository UNC. Both scripts parse and run
under Windows PowerShell 5.1.

The launcher derives a fresh bundled configuration from the current
`.wslconfig`, snapshots it, atomically writes/removes both pair keys, checks
every `wsl.exe` result and stopped-state gate, executes bundled/candidate A/B
boots, and writes strict host/canary receipts. WSLg is exercised with a bounded
`xdpyinfo` request; degraded systemd is accepted only for an explicitly named
exact WSL-only service set. `READY` additionally requires the current boot,
configuration, release, hashes, layout, module tree, and vermagic to match.
Failure rollback is not labeled restored unless a fresh third boot returns to
the exact bundled-baseline kernel, distro, driver, and DXG identities.

WSL 2.7.12 does not admit unified/versioned modules artifacts. The source
explicitly rejects 6.18.40.1 unified layout and double nesting; no download,
build, or use of that artifact occurred. The lower 2026-07-10 `latest` paths
and GREEN rows are retained only as historical capability evidence and are not
current instructions.

Lightweight evidence currently includes Windows PowerShell 5.1 execution of
the hermetic installed wrapper-to-launcher chain, shell parser/static fixtures,
and direct rustfmt parser/format checks. Rust build/check/test/Clippy evidence
is intentionally pending explicit authorization. No live WSL, kernel, module,
service, device, swap, GPU, Docker, VM, or host configuration action occurred.

## 2026-08-23 direct-entrypoint inertness (superseded evidence record)

The three direct PowerShell entry points are now source-default PLAN/refusal.
The installer requires `-Run` with exact token
`INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE`; the logging wrapper and launcher
each independently require `-Run` with exact token
`PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL`. All comparisons are case-sensitive.
The installer gate precedes install-root creation, copy, and rename. The
wrapper gate precedes log-root/file creation and forwards the promotion gate.
The launcher gate precedes receipt/config writes and every WSL lifecycle call.

Only the shell `apply` branch forwards these parameters, and only after its
existing exact `--i-know-this-stops-all-wsl` proof. Both promotion scripts now
classify all seven fixture-bearing parameters by bound-parameter presence:
canary, baseline, rollback, runtime, dry config, external failure, and external
timeout. Any bound `Run` or `ConfirmationToken` refuses before validation or
mutation. Baseline, rollback, and runtime companion inputs without a terminal
fixture refuse before launcher-manifest or WSL access; the wrapper remains
log-free while verifying its immutable deployment before forwarding that
refusal. The expanded classifier therefore cannot fall through into live
promotion. The later remediation retires the gate-only fixture because fixture
capability and live authority must never coexist.

Hermetic TDD evidence for this source slice is GREEN:

- RED: the PowerShell suite exited 1 on the missing installer token; the shell
  suite exited 1 on missing forwarding.
- RED follow-up: the expanded fixture-authority matrix exited 1 at
  `standalone-baseline`, proving that exact live credentials could previously
  advance that parameter to manifest validation.
- GREEN: Windows PowerShell 5.1 parses all three scripts and the complete
  `Test-BootKernelSafeStatic.ps1` suite exits 0. It retains 22 direct-entry
  refusals and expands fixture-authority coverage from two cases to 80: all
  seven parameters standalone, every unordered pair, all-parameter and
  gate-fixture conflicts for launcher and wrapper; six credential-shape cases
  per entry point; three companion-only cases per entry point; and two
  representative legitimate-combination/live-authority conflicts. The suite
  therefore executes 102 distinct refusal scenarios. Exact-gate, temporary
  immutable install, every legitimate fixture capability, preflight, and
  installed wrapper-to-launcher paths pass. Sentinel SHA-256 and recursive
  path-state readback prove that refused paths do not change manifest,
  deployment, log, config, receipt, or temporary roots.
- GREEN: direct executable `scripts/kernel/test-wsl-kernel-static.sh` exits 0,
  including the apply-only token-forwarding assertion and legitimate sealed
  pair/config/receipt parser fixtures.
- Coverage is `N/A - E2E-only PowerShell/shell`; no Rust business-logic file is
  in this slice.

This historical checkpoint closed only source/static R4 inertness. The current
status remains **PARTIAL**. Suspended-custody/Windows handle execution and
cryptographic module-to-VHDX provenance are independent live blockers; any
separately approved A/B evidence window comes only after both are reviewed.

## 2026-08-23 superseding safety review

The current `6.18.35.2-microsoft-standard-WSL2+` boot emitted a DXG
`CONFIG_FORTIFY_SOURCE` field-spanning-write warning from Xwayland, and repeated
the isolated build distro starts failed to bring `/sbin/init` up within 10 s.
The exact DXG signature is also reported on Microsoft 6.18.26.1 and bundled
6.18.33.2-2, so it is classified as an upstream-open regression confounder,
not as a RamShared-introduced regression. Bundled reproduction is not a safety
waiver. See the
[incident finding](../../../reliability/incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md).

The source launcher now requires an exact-distro, bounded systemd/DXG canary
and a same-host bundled query-error baseline before it can retain a candidate.
The unaccepted issue patch in `microsoft/WSL#41093` is prohibited. No live A/B
canary has run, so current promotion remains NO-GO.

---

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| V1 historical bzImage + release.txt | **HISTORICAL ONLY** | 17,330,688 B; REL=`6.18.35.2-microsoft-standard-WSL2+`; HEAD=`1bd4ed3d4`; not a current promotion input |
| V2 config stickiness | **GREEN** | `CONFIG_BLK_DEV_UBLK=m` `CONFIG_ZRAM_WRITEBACK=y` `CONFIG_IO_URING=y` |
| V3 qemu-validate | **GREEN** | `QEMU-VALIDATE: PASS` KTEST-UNAME matches REL; stamp sha `d278b032…` |
| V4 status CLI | **GREEN** | prints STATE= |
| V5 enable no-op rules | **GREEN** | enable never restarts WSL; NEED_BUILD/NEED_* exit 2 |
| V6 enable &lt;30s | **GREEN** | smoke &lt;1s on stock path |
| V7 arm without bzImage | **GREEN** (logic) | refuse if missing |
| V8 apply without flag | **GREEN** | exit 5 |
| V9 apply live | **HISTORICAL ONLY / CURRENT NO-GO** | 2026-07-10 log predates the DXG/systemd finding; fresh A/B is mandatory |
| V10 default distro | **GREEN** | configured product distro remains unchanged |
| V11 direct PowerShell inertness | **GREEN (source/hermetic)** | 22 direct-entry refusals plus exact-gate and installed-chain PASS; no live authority |
| V12 shell forwarding boundary | **GREEN (source/hermetic)** | executable static suite proves only attended `apply` forwards both gates |
| V14 fixture authority | **GREEN (source/hermetic)** | 80 fixture-authority refusals plus legitimate forwarding/PASS paths; companion-only inputs cannot fall through |

---

## RF → evidence

| RF | Status | Evidence |
| --- | --- | --- |
| RF-K1 | GREEN | tree 6.18.y @ 1bd4ed3d4 |
| RF-K2 | GREEN | configs above |
| RF-K3 | GREEN | bzImage + .ko copies (ublk/zram/zsmalloc) under build dir |
| RF-K4 | GREEN | all under `<approved-build-volume>` |
| RF-K5 | GREEN | qemu PASS + stamp |
| RF-K6 | SOURCE READY / LIVE OPEN | boot-kernel-safe confirms only after full systemd/DXG canary; live A/B not run |
| RF-K7 | GREEN | no set-default lab |
| RF-K8 | **HISTORICAL GREEN only** | recorded 2026-07-10 module/control-device observation; R6 grants no runtime loader authority |
| RF-K9 | GREEN | cascade docs unchanged; NBD path |
| RF-K13–17 | GREEN | `scripts/kernel/wsl-kernel.sh` |
| RF-K18–20 | GREEN | arm gates + atomic write |

---

## Files delivered

| Path | Role |
| --- | --- |
| `scripts/kernel/wsl-kernel.sh` | CLI |
| `scripts/kernel/wsl-kernel-lib.sh` | probes / state machine |
| `scripts/kernel/build-wsl-kernel.sh` | default KTAG 6.18.y, JOBS=2 |
| `<approved-output-root>/<pair-id>/kernel.bzImage` | immutable manifest-bound candidate artifact |
| `<historical-output-root>/release.txt` | Historical REL/HEAD record |
| `<historical-output-root>/qemu-pass.stamp` | Historical offline gate record |
| `<historical-output-root>/{ublk_drv,zram,zsmalloc}.ko` | Historical module artifacts |
| `docs/specs/.../AUDIT-2.5.md` | go human apply |
| `docs/specs/.../IMPL.md` | this file |

---

## Small decisions

1. Used `include/config/kernel.release` instead of `make kernelrelease` while parallel make still ran.  
2. Copied .ko from tree before full modules_install finished (modules were already built as .ko).  
3. The historical session did not run `apply`. Its natural-reboot `arm` model is now superseded: production arm without bounded auto-revert is refused.
4. qemu module insmod FAIL in busybox accepted per existing qemu-validate policy.

---

## Validation numbers

| Metric | Value |
| --- | --- |
| bzImage size | 17 330 688 bytes |
| REL | 6.18.35.2-microsoft-standard-WSL2+ |
| HEAD | 1bd4ed3d4 |
| QEMU | PASS (KVM), uname match |
| KERNEL_SHA256 | d278b0327d4306a414e32ecec56c7d530541ed52756a68d8ecab356b49e25410 |
| enable path shutdown | none |

---

## Env-bound gaps

- The recorded GREEN state is historical and must be revalidated before use.
- Current live promotion is NO-GO on the observed DXG FORTIFY and systemd/init
  evidence, even though the DXG signature also exists in bundled kernels.
- Current product transport remains NBD Day-1.
- ublk product transport requires the dedicated
  `custom-kernel-ublk-product-transport` lifecycle campaign.

---

## Rollback trigger

- qemu fail → no stamp (satisfied).  
- apply canary timeout, malformed/unreadable evidence, version mismatch,
  systemd non-running, DXG/Xwayland/NVIDIA probe failure, any FORTIFY/init/
  unclean-journal/p9/fatal signature, query errors above bundled baseline, or
  missing module metadata → boot-kernel-safe disarms.
- User: `bash scripts/kernel/wsl-kernel.sh disarm` then restart WSL for stock.

---

## Current operator boundary

Do not run `arm`, `apply`, a natural reboot onto the candidate, module loading,
RamShared activation, or a pressure workload from this document. The only
remaining promotion procedure is a separately approved attended bundled/custom
A/B campaign through the auto-reverting canary. Product cascade remains NBD
Day-1 independently of this historical kernel artifact.

---

## Commit traceability

Non-trivial commits should cite: `RF-K1`…`RF-K20` as applicable; `SPEC ITEM-1`…`ITEM-6`.


## Cascade smoke (custom kernel)

| Step | Result |
| --- | --- |
| `ramshared check` | ready; UBLK=m; ublk=ready |
| `up --vram 512 --zram 512` | zram prio 200, nbd0 prio 100, disk sdc |
| `down` | swapoff-first; no managed ghosts |
| thrash | **not** run (host safety) |

## modules.vhdx

| Path | Size |
| --- | --- |
| `C:\wsl\modules-ramshared.vhdx` | ~2.8 GiB |
| `.wslconfig kernelModules=` | set |


## 2026-07-14 — NBD vs ublk product decision (issue #30)

**Context:** Daily host runs stock/inbox WSL kernel (`6.18.33.2-microsoft-standard-WSL2`); `/dev/ublk-control` **absent**. Product cascade policy already fails closed on ublk for WSL2 (freeze risk on teardown; transport=auto → nbd).

| Criterion | NBD (Day-1) | ublk (lab/custom kernel) |
| --- | --- | --- |
| Available now on daily WSL | **YES** | **NO** without custom kernel + modules VHDX |
| Host safety | Proven cascade path | Historical freeze class on WSL teardown |
| 15% latency win claim | Not re-measured this session | **Blocked** until custom kernel READY + non-daily lab |
| Product ship | **Ship NBD** | Optional Phase B if kernel earns keep |

**Acceptance re-scope (honest):**
1. ~~Compile custom kernel with CONFIG_BLK_DEV_UBLK~~ — capability existed on armed custom kernel earlier; **not** the running product kernel today.
2. Latency suite under pressure on daily host — **refuse** (benchmarks.md / thrash policy).
3. ublk ≥15% better than NBD — **OPEN** only in isolated lab (qemu already has ublk smoke PASS; not apples-to-apples swap latency).

**Recommendation:** keep product on **NBD**; close #30 as **wontfix on daily WSL / deferred to dedicated kernel lab** unless custom kernel is re-armed and measured off the daily host.
