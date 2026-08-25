---
slug: wsl2-custom-kernel-p1
title: "Custom WSL2 kernel P1 — official-tree base + ublk + zram writeback"
milestone: —
issues:
  - "microsoft/WSL#41054"
---

# PRD — Custom WSL2 kernel P1

> **Document stage:** SSDV3 Step 1 (`PRD`). This is the canonical requirements
> document for the slug, not proof of delivery.
> **Design decision:** **GO to SPEC** after the 2026-07-10 UX audit. This verdict
> approves design continuation only.
> **Product qualification:** `PARTIAL / LIVE BLOCKED`. The document does not
> prove that a current kernel artifact, control command, package, installation,
> or enablement path is qualified. The stock-kernel NBD cascade is a separate
> design surface.
> **Language:** English (binding engineering).  
> **SSDV3:** Passo 1. SPEC next.

## 2026-08-24 transaction and provenance addendum (binding)

This addendum supersedes every live-install, arm, disarm, apply, or promotion
instruction below. The current source refuses live promotion even with the
exact attended credentials. Independently, it cannot prove Windows
handle-bound execution under its required suspended-Job custody, and it has no
cryptographic evidence that the validated module file is contained unchanged
in the modules VHDX. The sealer may publish a qualification bundle, but it
labels that bundle promotion ineligible. `apply`, `arm`, the installer, the
promotion wrapper, and the promotion launcher therefore stop before install,
log, receipt, config, WSL lifecycle, or module effects. Overall status remains
`PARTIAL / LIVE BLOCKED`.

- **RF-K40:** `PreflightOnly` is wholly hermetic. It requires a non-blank,
  confined runtime fixture and refuses before deployment, manifest, or
  `wsl.exe` access when that fixture is absent.
- **RF-K41:** every fixture input and every fixture output, PID, config,
  receipt, and log path is confined beneath one canonical direct child of the
  Windows temporary directory. The root name equals a 128-bit nonce and a
  single-link marker binds the same nonce. UNC, device, ADS, short-name,
  traversal, per-segment DOS device names even with extensions, trailing
  dot/space, control/invalid characters, case-collision, hard-link, and
  reparse-ancestor forms fail closed before any filesystem probe.
- **RF-K42:** fixture mode and live authority are mutually exclusive. This
  includes transaction, failure-injection, lock timeout/hold/evidence, runtime,
  canary, config, process, installer, and preflight inputs.
- **RF-K43:** an exclusive `Global\` mutex keyed by canonical target identity is acquired
  before transaction mutation. Under the held lock, the implementation
  revalidates the exact manifest, artifact hashes and filesystem identities,
  target config, and lock identity immediately before a `READY` result.
- **RF-K44:** exact original bytes or absence are captured immediately after
  the canonical lock and before fallible validation. Receipt-root creation,
  snapshots, config/receipt writes, and every persistent transaction mutation
  remain in rollback custody. Deterministic failure at each boundary restores
  only attempted targets; early validation failure performs no restore write.
- **RF-K45:** process execution uses `CreateProcessW(CREATE_SUSPENDED)`, assigns
  the never-resumed child to a configured kill-on-close Job, and retains native
  handles and creation identity. It has one end-to-end deadline, checked Job
  termination, concurrent stdout/stderr drain, and a strict output-byte cap.
  Descendants that inherit pipes or ignore graceful termination cannot create
  an unbounded wait. Numeric PID termination is prohibited.
- **RF-K46:** artifact installation copies from a locked, single-link source
  handle into invocation-owned staging and revalidates identity plus hash.
  The design does not claim immutable Windows handle execution where the
  platform launcher cannot prove it; current live promotion remains refused.
- **RF-K47:** `wsl-kernel.sh enable` is unconditionally inert and contains no
  module loader or privilege-elevation path. Its command-wide deadline begins
  before state resolution.
- **RF-K48:** shell parsers reject duplicate manifest/approval flags, unknown
  arguments, and missing or blank values before effects.
- **RF-K49:** atomic replacement retries only native transient sharing errors,
  preserves the original backup until replacement readback commits, and
  restores exact bytes or absence after injected failure.
- **RF-K50:** sealer publication is race-safe and no-replace, seals ownership,
  permissions, link count, hashes, and target-directory semantics, and reports
  `REFUSED_MODULE_VHDX_PROVENANCE_UNVERIFIED` until cryptographic containment
  evidence exists.
- **RF-K51:** the four SPEC-named tests are independently selectable executable
  mappings with exact exit/reason output. Every child PowerShell process is
  created under suspended-Job custody with an internal watchdog and the suite
  has a separate outer watchdog. Tests are hermetic and never call live WSL
  lifecycle APIs.
- **RF-K52:** assignment failure terminates and reaps the never-resumed root by
  handle. Timeout and supervision failure use checked `TerminateJobObject` and
  bounded handle waits; retained handles close only after exact custody proof.
- **RF-K53:** recursive cleanup is forbidden. Windows deletion is retained-
  handle and file-identity bound; shell cleanup permits only exact known leaves
  under a held directory identity. Ownership uncertainty leaks safely and
  replacement sentinels remain untouched.
- **RF-K54:** direct exact live credentials recognize the gate but grant no
  live effect: the installer returns `STAGING_CUSTODY=NO_GO`, and wrapper plus
  launcher return `LIVE_PROMOTION=NO_GO`, before filesystem or child use.
- **RF-K55:** hashing and parsing are bounded within the same monotonic
  transaction deadline. Hashing is chunked with size and remaining-time checks,
  and the outer Job cancels blocked I/O.

These requirements authorize only source edits and hermetic verification. R6
functional evidence remains pending until a contention-free PowerShell 5.1
window; limited source/static checks are not completion evidence.

## 2026-08-23 safety addendum (binding historical basis)

The original activation UX is superseded for live promotion by the observed
upstream-open WSL 6.18 DXG FORTIFY warning and repeated custom-distro init
timeouts. The signature exists on 6.18.26.1, bundled 6.18.33.2-2, and the local
6.18.35.2 artifact. This rejects RamShared attribution but does not establish
safety.

- **RF-K21:** a custom kernel may be retained only after a bounded canary on
  one exact distro proves exact version, systemd `running`, DXG/Xwayland,
  lightweight GPU metadata access, module metadata, and readable fresh logs.
- **RF-K22:** any fresh FORTIFY/init-timeout/unclean-journal/p9/fatal signal is
  a rollback trigger.
- **RF-K23:** DXG query failures require a same-host bundled-kernel baseline;
  candidate count must not exceed it. A bundled FORTIFY warning keeps both
  paths unqualified rather than waiving the gate.
- **RF-K24:** natural-reboot arm without auto-revert is unsafe-lab-only and
  refused by default.
- **RF-K25:** RamShared must not auto-apply issue-level or otherwise unaccepted
  upstream kernel patches.
- **RF-K26:** production `apply` accepts one immutable kernel/modules pair. The
  pair includes the exact kernel, modules VHDX, strict layout inventory, QEMU
  receipt, release, module vermagic, minimum WSL runtime, sizes, and SHA-256
  values under one hash-bound manifest. Mutable `latest` paths are forbidden.
- **RF-K27:** before shutdown, the WSL-side command atomically installs the
  reviewed wrapper, launcher, and complete pair into one versioned Windows
  bundle. The post-shutdown chain must not depend on a WSL UNC or repository
  path.
- **RF-K28:** the installed wrapper and launcher must parse and execute under
  Windows PowerShell 5.1. Every external command exit and timeout is checked,
  and timeout containment terminates the full host process tree.
- **RF-K29:** WSL 2.7.12 must reject unified/versioned modules artifacts,
  including 6.18.40.1 and double nesting. Unified layout remains closed until
  a reviewed released runtime containing `microsoft/WSL#41267` is allowlisted.
- **RF-K30:** candidate arm and rollback update `kernel=` and `kernelModules=`
  as one atomic pair. Each transaction takes a fresh hash-verified snapshot;
  no historical `wslconfig-original.txt` is an authority.
- **RF-K31:** the bundled baseline and candidate receipt bind boot ID, exact
  distro, Windows/WSL/WSLg/kernel/GPU-driver versions, DXG cardinality, and
  current-boot dmesg evidence. WSLg must complete a bounded display
  transaction rather than only expose a process name.
- **RF-K32:** `READY` requires the current configuration, boot ID, release,
  manifest, kernel hash, modules hash, layout, module tree, and vermagic to
  match the exact successful candidate receipt. Uncertainty is `BROKEN`.
- **RF-K33:** a typical WSL getty failure is not silently ignored. A degraded
  baseline/candidate is accepted only when its exact failed-unit set equals an
  explicit per-transaction WSL-only exception.
- **RF-K34:** a failed transaction may report bundled rollback only when a new
  boot differs from both the baseline and any observed candidate boot while
  its kernel, distro, driver, and DXG identities equal the valid bundled
  baseline. Pair-key removal alone is not rollback proof.
- **RF-K35:** each direct PowerShell installer/promotion entry point is inert by
  default. Live authority requires both an explicit `-Run` switch and the exact
  case-sensitive token naming that entry point's effect. Missing, blank,
  malformed, wrong, or wrong-case tokens refuse before any install root, log,
  receipt, host config, or WSL lifecycle mutation.
- **RF-K36:** the logging wrapper independently validates the promotion gate,
  forwards it to the bundled launcher, and creates no log directory or file
  until the gate is valid. The launcher independently revalidates the gate
  before any receipt/config write or `wsl.exe` lifecycle call.
- **RF-K37:** only `wsl-kernel.sh apply`, after its existing exact attended
  stop-all-WSL proof, may forward the installer and promotion gates. No other
  shell command may forward either token.
- **RF-K38:** hermetic gate, canary, config, runtime, and preflight fixtures
  remain non-live. They must not accept or carry live authority, invoke WSL, or
  mutate a host target; refusal tests preserve sentinel hashes and path-state
  readback and pair every refusal class with a legitimate hermetic PASS.
- **RF-K39:** every fixture-bearing parameter (`EvaluateCanaryFixture`,
  `BaselineCanaryFixture`, `EvaluateRollbackFixture`,
  `EvaluateRuntimeFixture`, `DryRunConfig`,
  `EvaluateExternalFailureFixture`, and `EvaluateExternalTimeoutFixture`)
  selects a non-live capability in both launcher and wrapper. Any explicitly
  bound `Run` or `ConfirmationToken` parameter, including blank, malformed,
  wrong, token-only, or duplicated forms, refuses before manifest/deployment
  validation, log/receipt/config mutation, or WSL access. Baseline, rollback,
  and runtime companion inputs cannot stand alone or fall through to the live
  path. Transaction fixture inputs are governed by RF-K40–RF-K51 above.

These are source requirements, not authorization to run the live A/B campaign.
See the [2026-08-23 finding](../../../reliability/incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md).

---

## 0. UX audit (user constraint — binding)

The requirements above supersede every lower historical reference to a
`bzImage-ramshared-latest` path, a kernel-only arm, a stale clean-config file,
or `uname`-only readiness. Those references describe the 2026-07-10 capability
record and are not an executable current contract.

### 0.1 What you asked

1. Enable/activate **from a command inside WSL2**.  
2. **Must not freeze / hang** anything.  
3. The system should **already know how to activate** — in the common case it should **do almost nothing**.

### 0.2 Audit findings (fact vs wish)

| Wish | Physics / WSL reality | Class |
| --- | --- | --- |
| “Turn on ublk features from inside WSL” | **No current enable action.** R6 returns an unconditional `NO_GO`; a later reviewed design is required. | Blocked |
| “Switch kernel binary without restarting WSL” | **Impossible**: kernel is selected by Windows `.wslconfig` **before** the VM starts. Change only applies after **`wsl --shutdown`** (kills **all** distros) | Confirmed platform limit |
| “One command that never disrupts” | Possible for **status + feature activate**. **Not** possible for **first-time kernel switch** without an explicit restart step | Confirmed |
| “Shouldn’t do anything” | Correct **default**: detect state → if already good, **exit 0, no-op** | Required UX |

### 0.3 Verdict of audit on previous PRD draft

| Gap in draft | Severity | Fix in this revision |
| --- | --- | --- |
| Activation described only as Windows `boot-kernel-*.ps1` | HIGH | Primary UX = **CLI inside WSL** (`ramshared-kernel` or `scripts/kernel/wsl-kernel.sh`) |
| Implied “arm = always restart WSL now” | HIGH | Default **never** calls `wsl --shutdown`. Restart is **opt-in** (`--apply-reboot` / explicit subcommand) |
| No idempotent “already active” path | HIGH | **RF-K13**: status/enable are **no-op** when ready |
| Risk of freeze under thrash / long silent ops | HIGH | **NFR-K8**: no thrash; bounded timeouts; print state; no background kill of user sessions without opt-in |
| Halo: “enable” means rewrite whole product | MED | Split **kernel binary arm** vs **feature activate** (modules) |

**Design conclusion:** continue to SPEC for the custom-kernel option, with the
proposed activation flow inverted as follows. This is not product
qualification:

```text
Default path (from WSL):  detect → ensure modules if possible → exit 0 (often zero work)
Opt-in path:              arm .wslconfig for *next* boot (no shutdown)
Opt-in path (disruptive): apply reboot / wsl --shutdown (explicit only) + auto-revert on fail
```

---

## 1. Summary

### What we build

1. **Kernel binary** from official [`microsoft/WSL2-Linux-Kernel`](https://github.com/microsoft/WSL2-Linux-Kernel) **`linux-msft-wsl-6.18.y`**, base **`Microsoft/config-wsl`**, minimum deltas:

| Symbol | Target | Role |
| --- | --- | --- |
| `CONFIG_BLK_DEV_UBLK` | **`=m`** | ublk module |
| `CONFIG_ZRAM_WRITEBACK` | **`=y`** | zram writeback |
| `CONFIG_IO_URING` / `ZRAM` / `SWAP` / `NBD` | stock | verify only |

2. **In-WSL control surface** that:
   - **knows** whether we are already on the right kernel + modules;
   - **activates** features without reboot when possible;
   - **does nothing** when already good;
   - **never freezes** the host (no thrash, no surprise `wsl --shutdown`).

No new kernel C code. Config hygiene matches MS-style `config:` commits. Community PR merge to MS is **not** a gate ([microsoft/WSL#41054](https://github.com/microsoft/WSL/issues/41054) advocacy only).

### What “native” means

| Claim | Truth |
| --- | --- |
| Our WSL2 kernel with ublk/writeback | Yes, when custom bzImage is booted |
| Stock MS for everyone | No — issue only |
| VRAM as RAM / HMM | No — out of scope |
| Day-1 VRAM path | Still userspace cascade (NBD until later SPEC) |

### One-sentence goal

**Build the official-tree custom kernel, then expose a WSL-side command that mostly no-ops when ready, enables modules when needed, and only restarts WSL when the user explicitly asks.**

---

## 2. Technical context

### 2.1 Environment (2026-07-10) — Confirmed

| Fact | Class |
| --- | --- |
| Isolated build distro on `<approved-build-volume>`; configured product distro unchanged | Confirmed |
| Tree `linux-msft-wsl-6.18.y` @ `1bd4ed3d4`; configs UBLK/WRITEBACK verified | Confirmed |
| Build in progress; bzImage not yet at first PRD write | Confirmed |
| Artifacts under `<isolated-build-root>`; kernels under `<approved-kernel-root>` | Confirmed |
| Existing `boot-kernel-safe.ps1` **does** `wsl --shutdown` on full arm | Confirmed in codebase |
| Interop `wsl.exe` / PowerShell from WSL is flaky on custom kernels (binfmt) — SPEC must handle | Confirmed in environment |
| gha-ubuntu stays Running (user) | Confirmed |

### 2.2 Codebase / docs — Confirmed

| Fact | Class |
| --- | --- |
| `build-wsl-kernel.sh`, `qemu-validate.sh`, `boot-kernel-safe.ps1` | Confirmed |
| FASE-B runbook; cascade Day-1 = NBD | Confirmed |
| ADR-0007: kernel C | Confirmed |

### 2.3 Platform law (must not fight)

| Law | Implication |
| --- | --- |
| Kernel chosen at WSL VM start | “Switch kernel” ≠ “toggle a sysctl” |
| `wsl --shutdown` kills all distros | Must be **explicit**, never default of `enable` |
| Modules can load at runtime | “Activate ublk” **should** be cheap on custom kernel |

### 2.4 Inference

| Item | Class |
| --- | --- |
| CLI name: `scripts/kernel/wsl-kernel.sh` with subcommands `status|enable|arm|disarm|apply` | Proposal (SPEC freezes name) |
| If interop dead: CLI prints exact Windows one-liner; does not hang retrying forever | Proposal |

---

## 3. Recommended option

### 3.1 Design decision — continue custom-kernel work

| Option | Verdict |
| --- | --- |
| Build official-tree + min config | **GO** |
| Default activate = full WSL restart every time | **Reject** (freezes/kills sessions) |
| Historical default activation proposal | **SUPERSEDED / NO-GO** — `enable` is unconditionally inert |
| Kernel switch only with explicit apply | **GO** |
| Wait only on MS stock | **Reject** as sole path |

### 3.2 Proposed activation model

```text
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel status     (always safe, read-only)             │
│    → running kernel? custom path in .wslconfig? ublk?       │
│    → exit 0 + READY | NEED_ARM | NEED_REBOOT | NEED_BUILD   │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel enable     (DEFAULT product verb — from WSL)    │
│    if READY:           do nothing meaningful; exit 0        │
│    enable is inert: report NO_GO; do not load a module      │
│    if stock kernel:    print “arm then apply”; exit 2       │
│                        **no** shutdown, **no** thrash       │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel arm        (write .wslconfig kernel= only)      │
│    → backup; set kernel= to <approved-kernel-root>          │
│    → does NOT shutdown                                      │
│    → next natural WSL restart picks it up                   │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel apply      (EXPLICIT disruptive)                │
│    → requires --i-know-this-stops-all-wsl                   │
│    → boot-kernel-safe path: shutdown + verify + auto-revert │
│    → never the default of enable                            │
└─────────────────────────────────────────────────────────────┘
```

**“Shouldn’t do anything”** = `enable` when already READY is a **documented no-op success**.

### 3.2.1 State machine (binding — closes audit G1)

| State | Meaning | Typical probes (SPEC freezes exact commands) |
| --- | --- | --- |
| **NEED_BUILD** | No usable `bzImage` artifact on R: | file missing or size 0 |
| **NEED_ARM** | Artifact exists; `.wslconfig` has no `kernel=` to our path | artifact OK; config not pointing to it |
| **NEED_REBOOT** | `.wslconfig` already points to our path; **running** kernel is still stock/other | config armed; `uname -r` does not match expected custom release |
| **NEED_MODULE** | Historical state: running custom kernel lacks `ublk_drv` | No automatic remediation; `enable` returns `NO_GO` |
| **READY** | Running custom kernel; ublk usable (loaded or loadable in &lt;30s) | enable no-op path |
| **BROKEN** | Armed path missing file, or apply/revert failed loud | refuse enable; print disarm/repair |

`status` may report these historical states read-only. R6 `enable` never fixes
them: it returns exact `NO_GO` without a module-loader or shutdown path.

### 3.2.2 Arm vs apply gates (closes audit G2/G3)

| Command | Requires | Does not require |
| --- | --- | --- |
| **arm** | bzImage exists on R: (size &gt; 1 MiB); path writable `.wslconfig` | qemu-validate (user may arm for next natural reboot after they validated) |
| **apply** | **qemu-validate PASS recorded** for that artifact + explicit flag | — |
| **enable** | nothing destructive | never qemu, never shutdown |

**apply timeout (numerical):** boot verification must complete within **120s** (or the existing `boot-kernel-safe.ps1` timeout if lower — SPEC freezes one number). On timeout → restore clean `.wslconfig` and start stock path.

### 3.2.3 Where the CLI runs (closes audit G5)

| Action | Distro |
| --- | --- |
| Build | `<isolated-build-distro>` |
| Daily `status` / `enable` | configured product distro on the shared WSL2 kernel |
| `arm` / `apply` / `disarm` | Invoked from WSL but effects are **host-wide** (all distros share one kernel) |

### 3.3 Official quality bar (MS-style)

Same as before: `Microsoft/config-wsl` base, minimal delta, English `config:` commits, smoke evidence, no mega-config PR.

### 3.4 Where work happens

| Work | Env | Must not |
| --- | --- | --- |
| Build | `<isolated-build-distro>` on `<approved-build-volume>` | Fill the system volume |
| QEMU validate | lab | Skip before first arm |
| Daily enable | **inside product WSL** | Surprise shutdown |
| apply | user-confirmed | Agent auto-apply without flag |

---

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| **RF-K1** | Build from official MS tree 6.18.y line (pin in SPEC) |
| **RF-K2** | Base `Microsoft/config-wsl` + only listed Kconfig deltas; verify after olddefconfig |
| **RF-K3** | Produce bzImage + modules_install (or MS modules VHDX if required) |
| **RF-K4** | Artifacts on **R:** only |
| **RF-K5** | qemu-validate PASS before first **apply** |
| **RF-K6** | **apply** uses backup + auto-revert on boot failure (wrap/extend boot-kernel-safe) |
| **RF-K7** | Configured product distro remains unchanged |
| **RF-K8** | On custom kernel: prove ublk load + writeback capability (numbers in IMPL) |
| **RF-K9** | Stock NBD cascade remains Day-1 without custom kernel |
| **RF-K10** | Cascade ublk preference = later SPEC (optional after RF-K8) |
| **RF-K11** | MS merge not a gate; #41054 non-blocking |
| **RF-K12** | No new kernel uAPI |
| **RF-K13** | **In-WSL CLI** with at least: `status`, `enable`, `arm`, `disarm`, `apply` (names freezable in SPEC) |
| **RF-K14** | **`enable` is the default human path**: if already READY → **no-op exit 0**; if custom kernel and module missing → load module only; never shutdown |
| **RF-K15** | **`apply` is opt-in only** (explicit flag/subcommand); prints that all WSL sessions will stop |
| **RF-K16** | CLI must be **idempotent**: second `enable`/`arm` without change = success no-op |
| **RF-K17** | If Windows interop unavailable: CLI fails **fast** with copy-paste Windows command; no multi-minute hang |
| **RF-K18** | **arm** refuses if bzImage missing or size ≤ 1 MiB (no dead `kernel=` path) |
| **RF-K19** | **apply** refuses unless qemu-validate PASS is recorded for that artifact (file/log stamp SPEC freezes) |
| **RF-K20** | `.wslconfig` updates are **atomic** (write temp + replace) + single backup path; concurrent arm: last writer wins but never half-truncated file |

---

## 5. Non-functional requirements

| ID | Requirement |
| --- | --- |
| **NFR-K1** | No swap/ublk thrash on live host as “validation” |
| **NFR-K2** | Build `JOBS` capped under RAM pressure |
| **NFR-K3** | Disk policy R:/E: not C: for heavy artifacts |
| **NFR-K4** | apply fail → restore stock `.wslconfig` within **120s** boot-watch (or safer lower bound from existing script) |
| **NFR-K5** | Product cascade regression → document stock path |
| **NFR-K6** | Repro: HEAD, configs, kernelrelease, date in log |
| **NFR-K7** | English for kernel commits / binding docs |
| **NFR-K8** | **No freeze UX**: bounded timeouts on interop; no silent loops; `enable` completes in **&lt; 30s** when no reboot; no memory pressure test in enable path; interop fail-fast **&lt; 5s** |
| **NFR-K9** | **Session safety**: default commands must not kill other distros or Hyper-V VMs |
| **NFR-K10** | Retry only on proven transient I/O (e.g. sharing violation on `.wslconfig`); never retry “module not found” or bad path blindly (Kahneman #15) |
| **NFR-K11** | Attended tokens are literal, unique, case-sensitive parameters; environment variables, truthy strings, partial matches, duplicates, and fixture combinations grant no live authority |

---

## 6. Flows

### 6.0 Happy path (user intent — preferred)

After a separately qualified build and reboot onto the custom kernel, the
preferred `enable` verb is a no-op when already ready or performs one bounded
`ublk_drv` module-load attempt. Its expected terminal state is `READY`; it does
not restart WSL.

### 6.1 First-time install (rare, explicit)

This is a non-executable argument contract, not a current activation
instruction. The read-only `status` verb reports `NEED_BUILD`, `NEED_ARM`,
`NEED_REBOOT`, or `READY`. Unsafe-lab `arm` may write the pair only under its
separate exact proof. Disruptive `apply` requires an absolute immutable
manifest plus `--i-know-this-stops-all-wsl`, then owns shutdown, verification,
and fail-closed rollback. A later `enable` is a no-op or one bounded module-load
attempt; live promotion remains blocked by the qualification gates below.

### 6.2 Build (lab)

```text
clone MS tree → config-wsl + deltas → olddefconfig verify
→ make -j$JOBS → modules_install → `<approved-kernel-root>`
```

### 6.3 Offline validate

```text
qemu-validate PASS required before first apply
```

### 6.4 Product cascade

```text
stock or custom: ramshared up (NBD Day-1)
custom + later SPEC: prefer ublk if present
```

---

## 7. Data model / artifacts

| Artifact | Path |
| --- | --- |
| Tree | lab `~/src/WSL2-Linux-Kernel` |
| Build | `<isolated-build-root>` |
| Sealed pair | `<approved-output-root>\<pair-id>\{kernel.bzImage,modules.vhdx,modules-layout.manifest,qemu-pass.stamp,kernel-pair.manifest}` |
| CLI | `scripts/kernel/wsl-kernel.sh` (SPEC may rename) |
| Windows helper | existing `boot-kernel-safe.ps1` only for **apply** path |
| State probe | `uname -r`, `/proc/config.gz` or modules, `.wslconfig` via interop if up |

---

## 8. API / interfaces

| Interface | Change |
| --- | --- |
| **WSL CLI `wsl-kernel`** | **Yes — primary product control** |
| `.wslconfig` `kernel=` | Yes, only via `arm` / `apply` / `disarm` |
| Runtime module loading | **Not authorized**; `enable` is inert |
| RamShared daemon uAPI | No in this PRD |
| Auto `wsl --shutdown` on `enable` | **Forbidden** |

Exit codes (proposal for SPEC):

| Code | Meaning |
| --- | --- |
| 0 | READY / success no-op / enable ok |
| 2 | Need arm/reboot/build (action required, not crash) |
| 3 | Interop/Windows helper failed (fail fast) |
| 4 | apply failed and reverted (or revert failed — loud error) |

---

## 9. Dependencies and risks

| Risk | Mitigation |
| --- | --- |
| User thinks enable reboots WSL | Docs + CLI text; enable never reboots |
| apply freezes host | Only shutdown WSL VM; no thrash; auto-revert; NFR-K8 |
| Interop broken | Fail fast + print Windows command (RF-K17) |
| Boot brick | QEMU first; apply auto-revert |
| Scope creep HMM | Out of scope |

**Kahneman:** #2 no arm without QEMU; #13 enable ≠ product cascade rewrite; #16 default safe no-op; #17 arm/enable idempotent.

---

## 10. Implementation strategy

| Step | Output |
| --- | --- |
| 1 | This PRD (design decision + UX audit) |
| 2 | SPEC: freeze CLI name, probes, arm without shutdown, apply flags |
| 2.5 | AUDIT-2.5 before first **apply** |
| 3 | Finish bzImage build |
| 4 | qemu-validate |
| 5 | Implement CLI; wire enable no-op path first |
| 6 | IMPL evidence |
| 7 | Optional: cascade ublk SPEC later |

---

## 11. Documents to update (with IMPL)

| Doc | Why |
| --- | --- |
| FASE-B-KERNEL.md | Subordinate to this PRD; CLI first |
| WSL-KERNEL-LAB.md | Lab vs product enable |
| cascade docs | enable does not replace ramshared up |
| validation.md | status/enable/apply results |
| INDEX.md | regenerate |

---

## 12. Out of scope

- MS GitHub kernel PR merge as gate  
- Implicit WSL shutdown on enable  
- Stopping the CI VM
- Starting or modifying a Windows lab VM
- HMM / VRAM-as-RAM  
- Mega config rewrites  
- Thrash tests on live host  
- Replacing NBD Day-1 before separate SPEC  

---

## 13. Acceptance criteria

| # | Criterion |
| --- | --- |
| A1 | SPEC approved (+ 2.5 go if apply used) |
| A2 | bzImage + verified configs |
| A3 | qemu-validate PASS |
| A4 | Minimal intent patch (2 symbols) |
| A5 | CLI `status` / `enable` work **from inside WSL** |
| A6 | `enable` on READY = **no-op**, exit 0, **&lt; 30s**, no shutdown |
| A7 | `enable` never freezes host (no thrash, no surprise kill) |
| A8 | `apply` only with explicit flag; auto-revert on fail |
| A9 | Stock cascade path still documented |
| A10 | IMPL maps RF-K* → evidence |
| A11 | State machine §3.2.1 implemented by `status` |
| A12 | arm refuses missing bzImage; apply refuses without qemu stamp |
| A13 | all three direct PowerShell entry points refuse before mutation unless both exact live gates are present |
| A14 | executable hermetic suites prove default/missing/blank/malformed/wrong/wrong-case/duplicate refusal, every fixture parameter standalone and pairwise under supplied live authority, companion-only refusal, and legitimate fixture/preflight PASS with sentinel readback |

### Kahneman PRD audit

Full map: [`AUDIT-PRD.md`](AUDIT-PRD.md). **Design verdict: GO to SPEC only**
after G1–G4 closed in this PRD revision. It is not an implementation or product
qualification verdict.

---

## 14. Validation

| Gate | Method |
| --- | --- |
| Config / build | log + release.txt |
| Offline boot | qemu-validate |
| UX no-op | run enable twice on READY; second is no-op |
| UX no freeze | enable does not call wsl --shutdown (grep/script test) |
| apply | only manual; auto-revert drill once |
| Module | No runtime loader; separately reviewed future qualification required |
| Direct PowerShell inertness | Windows PowerShell 5.1 `Test-BootKernelSafeStatic.ps1`; sentinel hashes and path-state readback |
| Shell forwarding boundary | direct executable `scripts/kernel/test-wsl-kernel-static.sh`; tokens occur only inside attended `apply` |

**Rollback triggers**

- QEMU fail → no arm/apply.  
- apply fail → restore clean `.wslconfig`.  
- enable hang &gt; 30s → treat as bug (NFR-K8).

---

## Traceability

| Prior | Relation |
| --- | --- |
| wsl2-native-vram-tier | Phase map; this owns P1 delivery + UX |
| cascade-swap | Day-1 NBD remains |
| boot-kernel-safe.ps1 | **apply** backend only, not default enable |
| WSL#41054 | Advocacy only |
| This audit §0 | Binding UX law |

---

## Design decision recorded

**GO to SPEC** for an official-tree custom-kernel design and an in-WSL command
whose default behavior is detection plus a no-op when ready. The proposed
kernel switch remains explicit and auto-reverting. Delivery, installability,
safe activation, and current runtime behavior remain `UNQUALIFIED` until their
own implementation and live gates pass.

**Next:** `SPEC.md` (Passo 2) freezes CLI contract, probes, paths, qemu stamp format, then **AUDIT-2.5** before first `apply`.
