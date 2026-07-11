---
slug: wsl2-custom-kernel-p1
title: "Custom WSL2 kernel P1 — official-tree base + ublk + zram writeback (definitive)"
milestone: —
issues:
  - "microsoft/WSL#41054"
---

# PRD — Custom WSL2 kernel (P1 definitive)

> **Status:** **GO** (revised after UX audit 2026-07-10).  
> **Authoritative** delivery PRD for custom WSL2 kernel P1.  
> **Day-1 cascade** (`wsl2-cascade-swap` / boot) still shippable on **stock** kernel via NBD.  
> **Language:** English (binding engineering).  
> **SSDV3:** Passo 1. SPEC next.

---

## 0. UX audit (user constraint — binding)

### 0.1 What you asked

1. Enable/activate **from a command inside WSL2**.  
2. **Must not freeze / hang** anything.  
3. The system should **already know how to activate** — in the common case it should **do almost nothing**.

### 0.2 Audit findings (fact vs wish)

| Wish | Physics / WSL reality | Class |
| --- | --- | --- |
| “Turn on ublk features from inside WSL” | If **already** running our custom kernel: `modprobe ublk_drv` (or already loaded) — **seconds, no reboot** | Achievable |
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

**Conclusion:** the PRD still **GO** for building the custom kernel, but **product-facing activation is inverted**:

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
| Lab `RamShared-Kernel` on `R:\WSL\…`; product default `Ubuntu-24.04` | Confirmed |
| Tree `linux-msft-wsl-6.18.y` @ `1bd4ed3d4`; configs UBLK/WRITEBACK verified | Confirmed |
| Build in progress; bzImage not yet at first PRD write | Confirmed |
| Artifacts `R:\WSL\RamShared-Kernel-build\`, kernels `R:\WSL\kernels\` | Confirmed |
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

### 3.1 GO custom kernel + **lazy activation UX**

| Option | Verdict |
| --- | --- |
| Build official-tree + min config | **GO** |
| Default activate = full WSL restart every time | **Reject** (freezes/kills sessions) |
| Default activate = status + modprobe, no-op if ready | **GO** |
| Kernel switch only with explicit apply | **GO** |
| Wait only on MS stock | **Reject** as sole path |

### 3.2 Activation model (definitive)

```text
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel status     (always safe, read-only)             │
│    → running kernel? custom path in .wslconfig? ublk?       │
│    → exit 0 + READY | NEED_ARM | NEED_REBOOT | NEED_BUILD   │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel enable     (DEFAULT product verb — from WSL)    │
│    if READY:           do nothing meaningful; exit 0        │
│    if custom kernel + ublk missing: modprobe ublk_drv       │
│    if stock kernel:    print “arm then apply”; exit 2       │
│                        **no** shutdown, **no** thrash       │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│  wsl-kernel arm        (write .wslconfig kernel= only)      │
│    → backup; set kernel= to R:\WSL\kernels\…                │
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
| **NEED_MODULE** | Running custom kernel; `ublk_drv` not loaded / not present | custom uname; modprobe needed or module missing |
| **READY** | Running custom kernel; ublk usable (loaded or loadable in &lt;30s) | enable no-op path |
| **BROKEN** | Armed path missing file, or apply/revert failed loud | refuse enable; print disarm/repair |

`status` prints one of these. `enable` only auto-fixes **NEED_MODULE** (modprobe). All other non-READY states → exit 2 with next step text (no shutdown).

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
| Build | `RamShared-Kernel` (lab) |
| Daily `status` / `enable` | **Product** `Ubuntu-24.04` (or any distro on the shared WSL2 kernel) |
| `arm` / `apply` / `disarm` | Invoked from WSL but effects are **host-wide** (all distros share one kernel) |

### 3.3 Official quality bar (MS-style)

Same as before: `Microsoft/config-wsl` base, minimal delta, English `config:` commits, smoke evidence, no mega-config PR.

### 3.4 Where work happens

| Work | Env | Must not |
| --- | --- | --- |
| Build | `RamShared-Kernel` on R: | Fill C: |
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
| **RF-K7** | Default distro remains Ubuntu-24.04 |
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

---

## 6. Flows

### 6.0 Happy path (user intent — preferred)

```text
# after build exists and user already rebooted WSL once onto custom kernel
$ wsl-kernel enable
READY: custom kernel + ublk  (or: loaded ublk_drv)
# often: zero side effects
```

### 6.1 First-time install (rare, explicit)

```text
$ wsl-kernel status          # NEED_BUILD | NEED_ARM | NEED_REBOOT | READY
$ wsl-kernel arm             # writes .wslconfig only
# user closes terminals later OR:
$ wsl-kernel apply --i-know-this-stops-all-wsl
  → shutdown + verify + auto-revert on fail
$ wsl-kernel enable          # no-op or modprobe
```

### 6.2 Build (lab)

```text
clone MS tree → config-wsl + deltas → olddefconfig verify
→ make -j$JOBS → modules_install → R:\WSL\kernels\
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
| Build | `R:\WSL\RamShared-Kernel-build\` |
| bzImage | `R:\WSL\kernels\bzImage-ramshared-latest` |
| CLI | `scripts/kernel/wsl-kernel.sh` (SPEC may rename) |
| Windows helper | existing `boot-kernel-safe.ps1` only for **apply** path |
| State probe | `uname -r`, `/proc/config.gz` or modules, `.wslconfig` via interop if up |

---

## 8. API / interfaces

| Interface | Change |
| --- | --- |
| **WSL CLI `wsl-kernel`** | **Yes — primary product control** |
| `.wslconfig` `kernel=` | Yes, only via `arm` / `apply` / `disarm` |
| `modprobe ublk_drv` | Via `enable` when needed |
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
| 1 | This PRD (GO + UX audit) |
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
- Stopping gha  
- win11-drill  
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

### Kahneman PRD audit

Full map: [`AUDIT-PRD.md`](AUDIT-PRD.md). **Verdict: GO to SPEC** after G1–G4 closed in this PRD revision.

---

## 14. Validation

| Gate | Method |
| --- | --- |
| Config / build | log + release.txt |
| Offline boot | qemu-validate |
| UX no-op | run enable twice on READY; second is no-op |
| UX no freeze | enable does not call wsl --shutdown (grep/script test) |
| apply | only manual; auto-revert drill once |
| Module | modprobe ublk_drv on custom kernel |

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

## Explicit GO (revised)

**GO** to build the custom kernel on the official MS tree **and** ship an **in-WSL command** whose default action is **detect + enable features with no-op when ready**, **without freezing or restarting WSL**. Kernel binary switch remains possible but **explicit, rare, and auto-reverting**.

**Next:** `SPEC.md` (Passo 2) freezes CLI contract, probes, paths, qemu stamp format, then **AUDIT-2.5** before first `apply`.
