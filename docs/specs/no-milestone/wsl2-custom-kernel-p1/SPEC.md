# SPEC — wsl2-custom-kernel-p1

> **Current status:** **PARTIAL / LIVE PROMOTION BLOCKED.** Source/static and
> hermetic results cannot authorize installation, promotion, WSL shutdown, or
> host configuration changes.

## 2026-08-24 R6 suspended-custody remediation (binding)

This section supersedes the process, cleanup, module-load, installer-live, and
test-completion claims in the transaction/provenance section below. The R6
source candidate remains **PARTIAL / LIVE PROMOTION BLOCKED**. One complete
PowerShell 5.1 run is GREEN; the independently selected final mode reruns and
remaining shell gates are pending.

1. A Windows child is created with `CreateProcessW(CREATE_SUSPENDED)`, assigned
   to a newly configured `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job before its
   primary thread is resumed, and identified by retained process and creation
   handles. Every native result is checked. Assignment failure terminates and
   reaps the never-resumed root by handle. Timeout or supervision failure uses
   checked `TerminateJobObject`, bounded handle waits, concurrent capped pipe
   drains, and closes retained handles only after custody is proved. Numeric
   PID termination and `taskkill` are prohibited.
2. Direct fixture execution in each PowerShell entry point is re-executed
   under that suspended-Job supervisor after default refusal and authority
   classification but before filesystem, hashing, manifest, deployment, or
   transaction work. Exact live credentials do not grant execution authority:
   the installer returns `STAGING_CUSTODY=NO_GO`, and the wrapper and launcher
   return `LIVE_PROMOTION=NO_GO`, before any filesystem query or effect.
3. Recursive path cleanup is prohibited. Windows-owned fixture and staging
   trees are deleted only through retained handles plus volume/file identity,
   reparse and single-link checks, and handle-bound disposition. Shell cleanup
   holds the directory identity, permits only fixed owned leaves, checks exact
   `device:inode:links:size` identity with `links == 1`, and otherwise leaks
   safely. Rename/replacement sentinels must remain untouched.
4. Immediately after the canonical `Global\` mutex is acquired, exact
   original bytes or absence are captured before any fallible validation.
   Rollback is permitted only after capture completes and only for targets
   whose mutation was attempted. Early validation failure performs no restore
   write and preserves exact state.
5. Lexical path rejection precedes every file/path probe. Each segment rejects
   case-insensitive DOS device stems even with an extension, trailing dot or
   space, control or Win32-invalid characters, ADS, namespaces, UNC, traversal,
   8.3 aliases, and case collisions.
6. End-to-end deadlines include argument processing, path work, hashing,
   parsing, locking, deployment, and transaction work. Hashing is bounded and
   chunked with remaining-time checks; the outer Job cancels blocked I/O.
   `wsl-kernel.sh enable` is unconditionally inert and contains no module
   loader or privilege escalation. The shell supervisor starts before state
   resolution.
7. The four exact SPEC test identifiers are independently selectable and have
   exact exit/reason output. Their R6 evidence includes injected assignment,
   resume, termination, and creation-identity failures; inherited-pipe
   descendants; pre-child expiry and blocked hashing;
   early-validation rollback, concurrent locking, replacement sentinels,
   hostile paths, exact sealer identity fields, and a positive installer-gate
   recognition that reaches the custody refusal without copying anything.
8. No R6 source/static result authorizes live installation or promotion. The
   unresolved Windows handle-execution guarantee and cryptographic
   module-to-VHDX provenance are independent blockers; later live canary work
   remains separately attended and out of scope.

## 2026-08-24 transaction/provenance remediation (binding)

This section supersedes every lower historical `latest`, kernel-only arm,
stale-config, natural-reboot, and `uname`-only instruction.

1. `seal-kernel-pair.sh` creates a never-overwritten directory identified by
   the kernel and modules hashes. Its strict manifest binds fixed basenames,
   byte sizes, SHA-256 values, release, module name/vermagic, minimum WSL
   runtime, `modules-layout.manifest`, and `qemu-pass.stamp`. Publication uses
   an invocation-unique directory and exact no-replace target semantics;
   ownership, modes, hashes, and single-link identity are read back. Because
   module-to-VHDX containment has no cryptographic attestation, the sealer
   prints `REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED`; its output is not a
   promotion credential.
2. The only admitted layout is currently `legacy_flat_v1` with zero release
   directories and zero nested release directories. WSL 2.7.12 predates merged
   unified-layout support in `microsoft/WSL#41267`; 6.18.40.1 unified artifacts
   and double nesting are refused. No future unified layout is admitted until
   an exact reviewed released WSL runtime is added to the allowlist.
3. `wsl-kernel.sh apply --manifest <absolute-path>
   --i-know-this-stops-all-wsl` rejects duplicate/blank/missing/unknown
   arguments, validates the pair, and currently emits
   `MODULE_VHDX_PROVENANCE=REFUSED` before PowerShell, installation, logging,
   config, or WSL lifecycle use. The retained forwarding source is unreachable
   until this SPEC gains a reviewed cryptographic containment contract.
4. A hermetically installed `boot-kernel-logged.ps1` accepts only the deployment manifest
   beside itself, validates an exact property set and every file hash, and runs
   the exact bundled launcher through
   `System32\WindowsPowerShell\v1.0\powershell.exe`. The chain never falls back
   to `C:\wsl\boot-kernel-safe.ps1`. Live invocation with exact credentials
   still refuses on direct suspended-custody/handle-execution uncertainty
   before deployment probing, log creation, or child creation. Attended shell
   `apply` separately refuses earlier on unverified module-to-VHDX provenance.
5. Transaction-fixture config writes use a same-directory temporary file, atomic replace,
   byte-length and hash readback, and exact key cardinality. Candidate arm
   writes exactly one `kernel=` and one `kernelModules=`; disarm/rollback removes
   both. The transaction records a fresh snapshot and never reads a historical
   clean-config file. Live config writes are unreachable while provenance is
   unverified.
6. Every `wsl.exe` call has checked exit status. Shutdown is followed by an
   exact stopped-distro gate before either boot. The baseline and candidate use
   distinct boot IDs; any failure attempts a bundled rollback. Rollback is
   proved only when its boot ID is fresh against the baseline and any observed
   candidate and its kernel/distro/driver/DXG identities equal the valid
   bundled baseline.
7. A strict canary binds host Windows/WSL/WSLg/kernel versions, exact distro,
   boot ID, GPU driver, one `/dev/dxg`, current-boot dmesg hash/counters,
   systemd state, module release/vermagic/layout, and a bounded `xdpyinfo`
   transaction. The whole guest command and each external probe are bounded;
   host timeout containment uses a suspended process assigned to a kill-on-close
   Job before resume, retained handles, and checked tree termination.
8. `systemd=running` requires zero failed units. `degraded` requires an explicit
   exact WSL-only unit set for that transaction; getty is never implicitly
   waived. Hard dmesg signals remain zero and candidate DXG query errors cannot
   exceed the same-host bundled baseline.
9. `READY` is derived only from a strict `READY` receipt plus the exact current
   boot ID, `.wslconfig` hash/pair paths, immutable artifact hashes/layout,
   running release, module tree, and vermagic. Any mismatch is fail-closed.
10. Hermetic tests execute the actual installed wrapper-to-launcher chain under
    Windows PowerShell 5.1 and cover pair rollback, external-command failure,
    strict parser refusal, WSLg/getty semantics, runtime/layout mismatch,
    6.18.40.1 unified refusal, deployment hash mismatch, exact baseline return,
    transaction failure boundaries, concurrent lock refusal, descendant-held
    pipe timeout, parser duplicates, race-safe pair sealing, and fixture path
    confinement. They do not launch WSL or alter host configuration.
11. `Install-BootKernelLaunchers.ps1` defaults to PLAN/refusal and recognizes
    `-Run` plus exact case-sensitive token
    `INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE`, then returns
    `STAGING_CUSTODY=NO_GO` before its first filesystem query or effect. Only a
    confined non-live fixture may exercise installation mechanics.
12. `boot-kernel-logged.ps1` and `boot-kernel-safe.ps1` each default to
    PLAN/refusal and independently require `-Run` plus exact case-sensitive
    token `PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL`. The wrapper creates no log
    root/file until its gate is valid; the launcher gates before receipt,
    `.wslconfig`, or `wsl.exe` lifecycle effects.
13. Missing, whitespace-only, malformed, wrong, wrong-case, or token-only
    approval refuses. Exact-token-only approval also refuses because `-Run` is
    independently mandatory.
14. Every fixture-bearing parameter is a non-live capability:
    `EvaluateCanaryFixture`, `BaselineCanaryFixture`,
    `EvaluateRollbackFixture`, `EvaluateRuntimeFixture`, `DryRunConfig`,
    `EvaluateExternalFailureFixture`, `EvaluateExternalTimeoutFixture`,
    `EvaluateTransactionFixture`, `InjectFailureBoundary`,
    `InjectAssignFailureFixture`, `InjectResumeFailureFixture`,
    `InjectTerminateFailureFixture`, `InjectRootCreationMismatchFixture`,
    `EvaluateDeadlineFixtureSec`, `EvaluateSlowStartupFixture`,
    `EvaluateSlowHashFixture`,
    `TransactionLockTimeoutSec`, `HoldTransactionLockMilliseconds`,
    `TransactionLockEvidenceFixture`, `FixtureRoot`, and `FixtureNonce`.
    Either entry point rejects any explicitly bound `Run` or
    `ConfirmationToken` before manifest/deployment validation, logging,
    receipt/config mutation, or WSL access. Blank, malformed, wrong,
    token-only, and duplicate forms do not change this boundary. Baseline,
    rollback, and runtime are companion inputs; without a terminal fixture or
    preflight mode they refuse before launcher-manifest or WSL validation
    instead of falling through. The retired gate-only fixture always refuses;
    exact live credentials are tested without granting fixture authority.
15. Only the unreachable post-provenance portion of `wsl-kernel.sh apply`,
    after parsing the exact `--i-know-this-stops-all-wsl` proof, contains the
    two PowerShell gate forwarders. No other command forwards them. Current
    execution refuses before reaching either forwarder.
16. `PreflightOnly` requires a non-blank `EvaluateRuntimeFixture`; absence or
    blank input returns `PREFLIGHT_RUNTIME_FIXTURE=REFUSED` before deployment,
    manifest, or `wsl.exe` access.
17. Every fixture path is a canonical descendant of
    `%TEMP%\ramshared-kernel-static-<128-bit-nonce>`. The root already exists,
    its exact leaf contains the supplied nonce, and a single-link marker binds
    the same nonce. All ancestors are checked for reparse points and exact
    case. UNC, namespace, per-segment DOS device names (including names with
    extensions), ADS, trailing dot/space, control/invalid characters, 8.3
    short-name, traversal, outside-root, hard-link, and case-collision inputs
    refuse before any file/path probe.
18. One `Global\` named mutex derived from the canonical config identity is acquired
    with a 1–10 second bound before transaction mutation. Abandoned ownership
    is safe to acquire; timeout returns `TRANSACTION_LOCK=REFUSED`. Manifest,
    artifact hashes and filesystem identities, config hash, and lock identity
    are revalidated under the held mutex immediately before `READY`.
19. Exact original bytes or absence are captured immediately after the lock
    and before fallible validation. Receipt-root creation, snapshot, bundled config, current receipt,
    transaction receipt, candidate config, and READY receipt are rollback
    boundaries. The executable matrix injects failure after each boundary and
    after atomic temp flush/replacement; exact original bytes or absence and
    unrelated sentinels must be restored only for an attempted mutation.
    Failure before mutation performs no compensating write.
20. Atomic replacement retries at most two times and only for native sharing
    violations 32 or 33 before replacement. The original backup remains until
    replacement length/hash readback succeeds. All other failures restore and
    propagate without retry.
21. Every redirected child is created suspended and assigned to a configured
    kill-on-close Job before resume. It drains stdout and stderr concurrently
    with a 1 MiB combined cap. Per-child and 300-second end-to-end deadlines
    are monotonic. Checked Job termination and every handle wait are bounded; a
    descendant holding pipes or ignoring graceful termination must not hang
    either suite. Numeric PID termination is forbidden.
22. The installer acquires a canonical `Global\` publication mutex before install-root
    creation. Each source is canonical, single-link, and non-reparse; it is
    copied while an exclusive source handle is held, with identity/hash/length
    revalidation into invocation-owned staging. Publication is no-overwrite;
    cleanup is handle/identity-bound, never recursive, and leaks safely when
    ownership cannot be proved. Live staging remains `NO_GO`.
23. The source makes no claim that pre-hash plus path execution provides an
    immutable Windows execution handle. That unresolved handle-use guarantee,
    plus module-to-VHDX provenance, keeps live promotion `NO_GO`.
24. `enable` is unconditionally inert, returns an exact `NO_GO`, and contains
    no module loader or privilege-elevation path. The command-wide deadline
    begins before state resolution.
25. Environment-selected roots and every ancestor are canonicalized and
    constrained. Shell manifest/approval parsers reject duplicates, unknown
    arguments, and missing/blank values before effects.
26. The named executable mappings are exactly
    `direct_entrypoints_refuse_before_mutation`,
    `legitimate_gate_and_fixture_paths_pass_without_live_effects`,
    `apply_is_the_only_gate_forwarder`, and
    `fixture_parameters_cannot_carry_live_authority`. Each mapping is
    independently selectable with exact exit/reason output. Each child
    PowerShell call uses suspended-Job custody and an internal 45-second
    watchdog; the direct suite requires a separate outer watchdog. Final R6
    functional evidence is pending a contention-free PowerShell 5.1 rerun.

## 2026-08-23 promotion correction (binding)

This section supersedes any later text that treats exact `uname`, a qemu stamp,
module best-effort, or a natural reboot as sufficient live promotion evidence.

1. `boot-kernel-safe.ps1` targets one explicit distro and uses one outer
   deadline of 10–120 seconds.
2. Confirmation occurs only after exact release equality, systemd `running`,
   character-device `/dev/dxg`, running Xwayland, a bounded non-allocating
   NVIDIA metadata query, module metadata, and readable current-boot warnings.
3. The hard rollback counts for DXG field-spanning warning, init timeout,
   unclean journal, p9 cancellation, and kernel fatal signatures are all zero.
4. `query_adapter_info`/`is_feature_enabled` errors are compared with a
   non-negative attended same-host bundled baseline. Missing or malformed
   baseline/evidence fails closed.
5. The canary never loads a module, activates RamShared, touches swap/block
   devices, or runs pressure. Failure disarms before any candidate can be
   retained.
6. `arm` without bounded auto-revert is refused by default and may exist only
   behind an explicit unsafe-lab token.
7. The issue-only patch proposed in `microsoft/WSL#41093` is not an accepted
   dependency and must not be applied by build or promotion automation.
8. A hermetic fixture evaluator proves PASS and each rollback class without
   launching WSL. The live A/B remains a separate environment-bound gate.

Traceability: PRD RF-K21–RF-K25; incident
[`2026-08-23-wsl2-dxg-fortify-systemd-no-go`](../../../reliability/incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md).

> **Passo 2 SSDV3.** Implements [`PRD.md`](PRD.md) in this folder.  
> **Zero creativity** outside this document. New decision → revise SPEC in-place first.  
> **AUDIT-2.5** required before first live `apply` (not before implementing `status`/`enable` no-op paths).

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-K1..K4, NFR-K2,K3,K6 | ITEM-1 Build artifact pipeline |
| RF-K2, RF-K11, A4 | ITEM-2 Config deltas + intent patch |
| RF-K5, RF-K19, A3 | ITEM-3 qemu-validate + PASS stamp |
| RF-K13..K17, RF-K14, NFR-K8,K9, A5–A7,A11 | ITEM-4 CLI `wsl-kernel` (status/enable) |
| RF-K18, RF-K20, RF-K16, §3.2.1 NEED_ARM | ITEM-5 CLI `arm` / `disarm` |
| RF-K6, RF-K15, RF-K19, NFR-K4, A8,A12 | ITEM-6 CLI `apply` (disruptive) |
| RF-K7, RF-K9, A9 | ITEM-7 Product defaults + cascade non-regression docs |
| RF-K8, A6 | ITEM-8 Module proof on custom kernel |
| NFR-K1, NFR-K10 | ITEM-9 Host safety + retry rules |
| PRD §11 | ITEM-10 Docs / validation.md / INDEX |
| RF-K26–RF-K33 | 2026-08-23 immutable-pair remediation |
| RF-K35–RF-K39, NFR-K11, A13–A14 | Direct-entrypoint inertness and fixture-authority evidence |
| RF-K40–RF-K51 | Transaction custody, deadlines, canonical fixture roots, parser/path hardening, race-safe publication, and provenance NO-GO |

---

## Files

| Path | Action |
| --- | --- |
| `scripts/kernel/wsl-kernel.sh` | **create** — primary in-WSL CLI |
| `scripts/kernel/wsl-kernel-lib.sh` | **create** — shared probes, exit codes, paths |
| `scripts/kernel/build-wsl-kernel.sh` | **extend** — default KTAG `linux-msft-wsl-6.18.y`; write release + stamp dir on R: |
| `scripts/kernel/qemu-validate.sh` | **reuse** — call site only; write PASS stamp |
| `scripts/kernel/seal-kernel-pair.sh` | **create** — seal exact kernel/modules/layout/QEMU pair |
| `scripts/kernel/Install-BootKernelLaunchers.ps1` | **create** — atomic versioned host-bundle installer |
| `scripts/kernel/boot-kernel-safe.ps1` | **replace** — strict A/B canary and atomic pair rollback |
| `scripts/kernel/boot-kernel-logged.ps1` | **replace** — verify deployment and invoke the bundled launcher under PowerShell 5.1 |
| `scripts/kernel/Test-BootKernelSafeStatic.ps1` | **extend** — PS5.1 parser plus sentinel-backed direct-gate refusal/PASS fixtures |
| `scripts/kernel/test-wsl-kernel-static.sh` | **extend** — executable hermetic shell and apply-only forwarding boundary |
| `<approved-kernel-root>` | **runtime** — bzImage artifacts (not in git) |
| `<isolated-build-root>` | **runtime** — logs, stamps, intent patch |
| `docs/runbooks/FASE-B-KERNEL.md` | **update** — point to this SPEC; CLI first |
| `docs/labs/WSL-KERNEL-LAB.md` | **update** — build vs enable |
| `validation.md` | **append** — gates with numbers |
| `docs/specs/.../IMPL.md` | Passo 3 |
| `docs/specs/.../AUDIT-2.5.md` | before first apply |

**Not in repo (host paths):**

| Path | Role |
| --- | --- |
| `<approved-output-root>\<pair-id>\` | Immutable source pair selected by `--manifest` |
| `<launcher-install-root>\<bundle-id>\` | Immutable installed wrapper/launcher/artifact bundle |
| `<launcher-install-root>\receipts\` | Transaction logs outside immutable bundles |
| `%UserProfile%\.wslconfig` | `kernel=` arm target |
| `%UserProfile%\.wslconfig.ramshared.snapshot.<UTC>` | Fresh transaction snapshot with receipt-bound hash |

---

## Constants (freeze)

| Name | Value |
| --- | --- |
| `KTAG_DEFAULT` | `linux-msft-wsl-6.18.y` |
| `KERNEL_PAIR_MANIFEST` | Explicit absolute path passed to `--manifest` |
| `BUILD_DIR_WIN` | `<isolated-build-root-windows>` |
| `BUILD_DIR_WSL` | `<isolated-build-root-wsl>` |
| `MIN_BZIMAGE_BYTES` | `1048576` (1 MiB) |
| `ENABLE_TIMEOUT_SEC` | `30` |
| `INTEROP_FAIL_SEC` | `5` |
| `APPLY_TIMEOUT_SEC` | `60` (align `boot-kernel-safe.ps1`; PRD max 120) |
| `INSTALL_CONFIRMATION_TOKEN` | `INSTALL_IMMUTABLE_KERNEL_LAUNCHER_BUNDLE` (exact case) |
| `PROMOTION_CONFIRMATION_TOKEN` | `PROMOTE_WSL_KERNEL_AND_STOP_ALL_WSL` (exact case) |
| `CONFIG_DELTAS` | `CONFIG_BLK_DEV_UBLK=m` `CONFIG_ZRAM_WRITEBACK=y` |
| `VERIFY_ALREADY` | `CONFIG_IO_URING=y` `CONFIG_ZRAM=m` `CONFIG_SWAP=y` `CONFIG_BLK_DEV_NBD=m` (warn if missing, do not force-fight stock) |

### Exit codes (`wsl-kernel.sh`)

| Code | Meaning |
| --- | --- |
| `0` | READY / success / no-op success |
| `2` | Action required (NEED_BUILD / NEED_ARM / NEED_REBOOT / user must apply) — not a crash |
| `3` | Interop / Windows helper / path error (fail fast) |
| `4` | apply failed (reverted or revert failed — message must say which) |
| `5` | Internal misuse (bad argv) |

---

## ITEM-1 — Build artifact pipeline

**RF-K1..K4**

### 1.1 Environment

- **Preferred build host:** `<isolated-build-distro>` on an approved build volume.
- The configured product distro remains unchanged; build scripts must not change the default.
- Build tree default: `<isolated-build-root>/WSL2-Linux-Kernel` (monitor its bounded volume with `df`).

### 1.2 Algorithm (`build-wsl-kernel.sh` or lab `build-kernel-lab.sh` — same semantics)

1. `KTAG=${KTAG:-linux-msft-wsl-6.18.y}`  
2. Clone if missing: `git clone --depth 1 --branch "$KTAG" https://github.com/microsoft/WSL2-Linux-Kernel.git "$KSRC"`  
3. `cp Microsoft/config-wsl .config`  
4. Apply ITEM-2 deltas via `./scripts/config`  
5. `make olddefconfig`  
6. **Verify** each CONFIG_DELTAS with grep; fail if not sticky  
7. `make -j${JOBS:-2}` then `sudo make modules_install`  
8. `REL=$(make -s kernelrelease)`  
9. Keep `arch/x86/boot/bzImage`, the modules VHDX, module metadata, and the
   layout inventory as versioned build inputs; do not publish a mutable alias.
10. Write `$BUILD_DIR_WSL/release.txt`:

```text
REL=<kernelrelease>
KTAG=<tag>
HEAD=<short sha>
DATE=<ISO8601>
JOBS=<n>
```

11. Append human line to `$BUILD_DIR_WSL/kernel-build.log`  
12. **Do not** write `qemu-pass.stamp` here (only ITEM-3 does).

### 1.3 Abort

- Config not sticky → exit 1 and do not seal or publish a pair.

### Kahneman (ITEM-1)

| # | Question | Evidence | Abort |
| --- | --- | --- | --- |
| #3 | Release string recorded? | `release.txt` | No REL → fail |
| #1 | Build on contested RAM? | JOBS=2 default | — |

---

## ITEM-2 — Config deltas + intent patch

**RF-K2, RF-K11**

### 2.1 Apply only

```bash
./scripts/config --file .config --module CONFIG_BLK_DEV_UBLK
./scripts/config --file .config --enable CONFIG_ZRAM_WRITEBACK
# optional ensure (if unset, enable; if already y/m, leave):
./scripts/config --file .config --enable CONFIG_IO_URING   # may already be y
```

Do **not** paste full `olddefconfig` toolchain noise into “intent patch”.

### 2.2 Intent patch file (for humans / MS-style review)

Path: `$BUILD_DIR_WSL/0001-config-ublk-zram-writeback.patch`  
Content: only the two symbol intent lines (as in PRD). Regenerated each successful config stage.

### 2.3 Not a gate

Opening GitHub PR on `WSL2-Linux-Kernel` is **out of SPEC success criteria**.

---

## ITEM-3 — qemu-validate + PASS stamp

**RF-K5, RF-K19**

### 3.1 Command

```bash
sudo bash scripts/kernel/qemu-validate.sh \
  "$KERNEL_WSL" \
  "$(grep '^REL=' "$BUILD_DIR_WSL/release.txt" | cut -d= -f2-)" \
  "$KSRC/drivers/block/ublk_drv.ko" \
  "$KSRC/mm/zsmalloc.ko" \
  "$KSRC/drivers/block/zram/zram.ko"
```

(Adjust `.ko` paths if out-of-tree build layout differs — must exist or validate documents skip with fail.)

### 3.2 PASS stamp

On qemu-validate exit 0, write:

`$BUILD_DIR_WSL/qemu-pass.stamp`

```text
REL=<same as release.txt>
KERNEL_SHA256=<sha256 of bzImage>
HEAD=<git short>
DATE=<ISO8601>
VALIDATE=qemu-validate.sh
```

### 3.3 Gate for apply

`apply` must:

1. Read stamp exists  
2. `sha256sum` of current `$KERNEL_WSL` equals `KERNEL_SHA256`  
3. Else refuse exit 2 with “re-run qemu-validate”

### Kahneman

| # | Rule |
| --- | --- |
| #2 | No stamp → no apply |
| #13 | Stamp alone insufficient without sha match |

---

## ITEM-4 — CLI `status` / `enable` (primary UX)

**RF-K13, RF-K14, RF-K16, RF-K17, NFR-K8, NFR-K9**

### 4.1 Entry point

```bash
# from repo root or PATH install later
bash scripts/kernel/wsl-kernel.sh <subcommand> [flags]
```

Default when no args: `status`.

### 4.2 Probes (library)

| Probe | Source |
| --- | --- |
| `P_BZ` | manifest-bound `kernel.bzImage` exists, exceeds `MIN_BZIMAGE_BYTES`, and matches size/hash |
| `P_REL` | `grep REL= release.txt` if present |
| `P_UNAME` | `uname -r` |
| `P_CUSTOM_RUNNING` | `P_REL` non-empty AND (`uname -r` equals REL **or** uname contains distinct custom marker written at build — freeze: **prefer exact REL match**; if MS-style name equals stock line, SPEC uses `REL` from release.txt only) |
| `P_CFG_ARMED` | `.wslconfig` contains exactly one `kernel=` and one `kernelModules=` pointing at the receipt-bound installed pair |
| `P_UBLK` | read-only evidence: `lsmod` reports `ublk_drv` or `/sys/module/ublk_drv` exists |
| `P_STAMP` | qemu-pass.stamp valid (ITEM-3) |

**Reading `.wslconfig` from WSL (order):**

1. `$WSL_CONFIG` env override
2. `/mnt/c/Users/<validated-windows-user>/.wslconfig`, with identity resolution bounded by **INTEROP_FAIL_SEC**
3. If interop fails: treat `P_CFG_ARMED` as **unknown** → status prints UNKNOWN_CFG; enable does not guess

### 4.3 State resolution (§3.2.1 PRD)

```
if ! P_BZ:           NEED_BUILD
elif ! P_CFG_ARMED:  NEED_ARM
elif ! P_CUSTOM_RUNNING: NEED_REBOOT
elif ! P_UBLK:       NEED_MODULE
else:                READY
```

If `P_CFG_ARMED` and !`P_BZ`: **BROKEN** (dead path).

### 4.4 `status`

- Print one line: `STATE=<…>` plus short human lines (uname, kernel path, ublk).  
- Exit `0` if READY else `2` (or `3` if interop required and failed hard for arm detection — prefer still print NEED_* with UNKNOWN_CFG).  
- **Read-only.** No writes. No modprobe. No shutdown.

### 4.5 `enable`

Within **ENABLE_TIMEOUT_SEC**:

| State | Action | Exit |
| --- | --- | --- |
| READY | no-op print READY | 0 |
| NEED_MODULE | perform one bounded `ublk_drv` load through the platform module loader (including dependencies), then re-probe | 0 if ok else 2 |
| NEED_BUILD / NEED_ARM / NEED_REBOOT / BROKEN | print next step only | 2 |
| any | **forbidden:** `wsl --shutdown`, thrash, long retry | — |

**Static check (CI/local):** `grep -E 'wsl --shutdown|wsl.exe.*--shutdown' scripts/kernel/wsl-kernel.sh` must not match enable code path (or entire file except apply calling PS1).

### 4.6 Idempotency

`enable` twice on READY → two exit 0, no log spam required; second may print READY again.

### Kahneman

| # | Rule |
| --- | --- |
| #16 | Default safe: enable never kills sessions |
| #17 | 2× enable = 1× effect |
| #15 | `enable` → exact `NO_GO`, no module-loader or retry loop |

---

## ITEM-5 — CLI `arm` / `disarm`

**RF-K18, RF-K20, RF-K16**

### 5.1 `arm`

1. Require a valid sealed pair manifest; otherwise exit 2.
2. Unsafe-lab `arm` additionally requires both artifacts on one mounted Windows
   drive and the explicit `RAMSHARED_UNSAFE_LAB_ARM` token.
3. Take a new timestamped snapshot of the current `.wslconfig` and verify its
   hash; never reuse or overwrite one backup slot.
4. Atomic write: temporary file in the same directory, replace, and hash
   readback.
5. Set exactly one `kernel=` and one `kernelModules=` under the one `[wsl2]`
   section. Duplicate sections refuse.
6. Idempotent: a second arm of the same pair is one pair of keys.
7. **No** `wsl --shutdown`.
8. Print that natural-reboot arm remains unsafe-lab-only and LIVE-NO-GO.

Implementation preference: call small PowerShell snippet via interop with **INTEROP_FAIL_SEC**; on fail exit 3 + paste:

```powershell
# example printed, exact text frozen in IMPL
```

### 5.2 `disarm`

1. Take a fresh timestamped, hash-verified snapshot.
2. Atomically remove all `kernel=` and `kernelModules=` lines.
3. Prove both key counts are zero after readback.
4. Do not shut down WSL; exit 0 if already disarmed.

### Kahneman

| # | Rule |
| --- | --- |
| #17 | arm 2× same path = one kernel= line |
| #2 | Missing bzImage → refuse arm |

---

## ITEM-6 — CLI `apply` (disruptive)

**RF-K6, RF-K15, RF-K19, NFR-K4**

### 6.1 Argument contract

This is a non-executable binding contract, not a current activation
instruction. The shell entry point accepts the `apply` verb, one
`--manifest <absolute-immutable-pair-path>` argument, and the exact attended
`--i-know-this-stops-all-wsl` proof in the same invocation.

Without flag → exit 5, print help. No side effects.

### 6.2 Preconditions

1. Strict immutable pair manifest and all fixed artifacts pass size/hash,
   release/layout/vermagic/runtime-minimum/QEMU admission.
2. Layout is admitted for the installed WSL runtime; unified or nested layout
   currently refuses.
3. Windows PowerShell 5.1 and interop are available before shutdown.

### 6.3 Steps

1. Print warning: all WSL distros will stop.  
2. Install the exact reviewed bundle before shutdown, then invoke the installed
   wrapper with only its adjacent deployment manifest and expected hash. The
   shell supplies `-Run` plus `INSTALL_CONFIRMATION_TOKEN` to the installer and
   `-Run` plus `PROMOTION_CONFIRMATION_TOKEN` to the wrapper. The wrapper
   validates the promotion token, forwards both parameters, and the launcher
   validates them again. These are argument bindings inside the already
   attended shell transaction, not separately runnable documentation. The
   repository path is not referenced after bundle installation.

3. On success: exit 0; state should become READY or NEED_MODULE.  
4. On failure: boot-kernel-safe must disarm; CLI exit 4.

### 6.4 Agent policy

Automation/agents **must not** run `apply` without explicit human flag in the same turn. Document in script header.

### Kahneman

| # | Rule |
| --- | --- |
| #2 | Timeout 60s → revert |
| #16 | Revert path independent of custom kernel health |
| #13 | Boot success + module-metadata warning ≠ auto-revert |

---

## ITEM-7 — Product defaults + cascade non-regression

**RF-K7, RF-K9**

1. Never change the configured default distro in any script in this SPEC.
2. Docs: Day-1 `ramshared up` remains NBD on stock kernel.  
3. `wsl-kernel enable` is **not** a substitute for `ramshared up`.  
4. FASE-B-KERNEL.md: first paragraph links this SPEC; “enable = no-op when ready”.

---

## ITEM-8 — Module proof

**RF-K8**

In a separately approved live campaign, record the module-load exit code, the
expected ublk control-node observation, and the kernel-documented zram
writeback capability. This is an evidence requirement, not a current
activation instruction. Record kernel release and exact observations in
IMPL/validation.md without pressure.

---

## ITEM-9 — Host safety + retry

**NFR-K1, NFR-K10**

| Operation | Retry? |
| --- | --- |
| `.wslconfig` share violation | yes, ≤6 × 800ms (existing PS) |
| module-loader request | **no; `enable` is inert** |
| qemu fail | **no** auto re-apply |
| enable path | **no** wsl shutdown ever |

Forbidden in enable/status/arm/disarm:

- fio / stress-ng / swap thrash  
- starting Hyper-V VMs  
- `wsl --shutdown`

---

## ITEM-10 — Docs + index

On IMPL commit:

1. Update FASE-B-KERNEL.md, WSL-KERNEL-LAB.md  
2. Append validation.md  
3. `node tools/generate-docs-index.mjs`  
4. IMPL.md RF→evidence table  

---

## Context matrix (scripts)

| Code | Context | May sleep | Notes |
| --- | --- | --- | --- |
| `wsl-kernel.sh` | process (WSL userspace) | yes | no kernel locks |
| `boot-kernel-safe.ps1` | Windows process | yes | may kill WSL VM |
| build | process | yes | JOBS capped |

No IRQ/softirq. No new kernel locks.

---

## Error table (CLI)

| Situation | Exit | User message gist |
| --- | --- | --- |
| READY enable | 0 | READY (no-op) |
| NEED_MODULE fixed | 0 | loaded ublk_drv |
| NEED_ARM | 2 | run: wsl-kernel arm |
| NEED_REBOOT | 2 | restart WSL or: apply --i-know… |
| NEED_BUILD | 2 | build first |
| BROKEN | 2 | disarm + repair artifact |
| No interop | 3 | paste Windows command |
| apply no flag | 5 | usage |
| apply fail | 4 | reverted / revert failed |

---

## Validation plan (SPEC-level)

| Gate | Command / check | Pass |
| --- | --- | --- |
| V1 | build → release.txt + bzImage &gt; 1MiB | yes |
| V2 | config grep UBLK=m WRITEBACK=y | yes |
| V3 | qemu-validate + stamp sha match | yes |
| V4 | `wsl-kernel status` prints STATE= | yes |
| V5 | enable on READY twice, both exit 0, no shutdown in strace/grep | yes |
| V6 | enable completes &lt; 30s (READY path) | yes |
| V7 | arm without bzImage exit 2 | yes |
| V8 | apply without flag exit 5 | yes |
| V9 | apply with stamp+flag refuses before PowerShell with unverified module-to-VHDX provenance | yes; live promotion remains NO-GO |
| V10 | configured product distro remains unchanged | yes |
| V11 | `Test-BootKernelSafeStatic.ps1` :: `direct_entrypoints_refuse_before_mutation` covers default/missing/blank/malformed/wrong/wrong-case/token-only cases with sentinel SHA-256 and path-state readback | yes; N/A - E2E-only PowerShell |
| V12 | `Test-BootKernelSafeStatic.ps1` :: `legitimate_gate_and_fixture_paths_pass_without_live_effects` covers exact-gate recognition, installed wrapper chain, canary, config, runtime, transaction, and preflight fixtures | yes; N/A - E2E-only PowerShell |
| V13 | `test-wsl-kernel-static.sh` :: `apply_is_the_only_gate_forwarder` runs by direct executable invocation and never calls PowerShell or WSL | yes; N/A - E2E-only shell |
| V14 | `Test-BootKernelSafeStatic.ps1` :: `fixture_parameters_cannot_carry_live_authority` covers every fixture-bearing parameter standalone/pairwise under supplied authority, credential variants, companion-only refusal, canonical-root/path attacks, sentinel readback, and legitimate installed-wrapper forwarding | yes; N/A - E2E-only PowerShell |

---

## Rollback triggers (numerical / observable)

| Trigger | Action |
| --- | --- |
| qemu-validate fail | do not write stamp; do not apply |
| apply: no WSL response in **60s** | disarm `.wslconfig`; restart stock |
| enable wall &gt; **30s** | bug; fix CLI (NFR-K8) |
| interop hang risk | kill after **5s**; exit 3 |
| custom boot but no ublk module ever | do not auto-disarm kernel (usable); mark NEED_MODULE / docs modules_install |

---

## Implementation order (mandatory)

1. ITEM-1/2 finish build + release.txt (may already be running)  
2. ITEM-4 library + `status` + `enable` (no-op path unit-testable on stock)  
3. ITEM-5 arm/disarm  
4. ITEM-3 qemu + stamp  
5. ITEM-6 apply wire-up  
6. **AUDIT-2.5.md** → go/no-go on apply  
7. Human apply once  
8. ITEM-8/7/10 IMPL + docs

---

## Out of SPEC (forbidden)

- MS GitHub PR as success gate  
- `enable` calling shutdown  
- Changing the configured default distro
- Cascade ublk as required (later SPEC)  
- HMM / VRAM-as-RAM  
- Stopping gha  
- `<approved-windows-lab>`

---

## Explicit handoff

| Next | When |
| --- | --- |
| Implement ITEM-4 status/enable | Now (safe) |
| AUDIT-2.5 | After ITEM-3/6 designed/impl ready, **before** first real apply |
| IMPL.md | When V1–V8 green; V9 human-gated |

**SPEC status:** **PARTIAL / LIVE BLOCKED**. Non-live implementation and
hermetic verification may proceed; apply execution remains provenance-blocked
regardless of older AUDIT-2.5 wording.
