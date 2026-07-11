# WSL2 official kernel — how contribution actually works (research 2026-07-10)

> **Corrected strategy.** Opening a community PR on `microsoft/WSL2-Linux-Kernel` for `config-wsl` is **not** the path MS uses. Evidence below.  
> **gha-ubuntu / product Ubuntu:** leave running (do not stop for this work).

## TL;DR

| Question | Answer |
| --- | --- |
| Does MS accept random PRs on `WSL2-Linux-Kernel`? | **No** (policy since ~2021; still true in 2026 practice) |
| Where to ask for `UBLK` / `ZRAM_WRITEBACK` in stock WSL? | **[microsoft/WSL](https://github.com/microsoft/WSL)** issues (feature request) |
| Where to contribute real kernel **code**? | **Upstream Linux** ([submitting-patches](https://www.kernel.org/doc/html/latest/process/submitting-patches.html)) |
| What works **today** for RamShared? | **Custom WSL kernel** from this tree + our configs (lab build) |
| SSDV3 for “merge into MS kernel”? | **No dedicated SPEC** yet; related PRDs exist (P1 only) |

---

## 1. SSDV3 in *this* repo (RamShared) — status

| Spec folder | PRD | SPEC | IMPL | Role vs “official WSL kernel function” |
| --- | --- | --- | --- | --- |
| `wsl2-cascade-swap` | yes | **yes** | (via product) | Day-1 cascade userspace — **not** MS PR |
| `wsl2-cascade-boot` | yes | yes | yes | Autostart cascade |
| `wsl2-native-vram-tier` | yes | **no** | no | Decision PRD: P0 product / P1 custom configs / P2–P3 mm |
| `kernel-vram-as-memory` | yes | no | no | Research “VRAM as memory” |
| `kernel-native-language` | yes | no (AUDIT only) | no | C vs Rust for kernel surface |
| `mainline-vram-tiering` | yes | no | no | True upstream mm (far) |
| `docs/labs/WSL2-UPSTREAM-CONFIG-PR.md` | this file | — | — | **Process research** (not a feature SPEC) |

**Gap closed for delivery:** authoritative SSDV3 PRD for **custom kernel P1** is  
[`docs/specs/no-milestone/wsl2-custom-kernel-p1/PRD.md`](../specs/no-milestone/wsl2-custom-kernel-p1/PRD.md)  
(MS stock merge is still **not** an acceptance gate; custom kernel is.)

---

## 2. Official repo policy (primary sources)

### README (`microsoft/WSL2-Linux-Kernel`)

- **Bugs / features:** report on **[microsoft/WSL](https://github.com/microsoft/WSL/issues/new/choose)** — *“It is not possible to report issues on the WSL2-Linux-Kernel project.”*
- **Kernel code contributions:** *“we encourage you to [submit the change **upstream**](https://www.kernel.org/doc/html/latest/process/submitting-patches.html)”* — i.e. **kernel.org**, not this GitHub as a community merge target.
- **Build:** `make KCONFIG_CONFIG=Microsoft/config-wsl` (+ modules VHDX scripts under `Microsoft/scripts/`).

### Explicit “we don’t take PRs” (maintainers)

From closed community config PR [#249](https://github.com/microsoft/WSL2-Linux-Kernel/pull/249) (erofs configs), **@tyhicks** (2022-08-04):

> “Unfortunately, **we don't take PRs through this repo**.”  
> “Please see the Feature Requests section of the README.”  
> “Subscribe to [this issue](https://github.com/microsoft/WSL/issues/7257). We'll discuss the request **internally**.”

From closed optimized-config PR [#245](https://github.com/microsoft/WSL2-Linux-Kernel/pull/245), **@jiayali-ms** (2021-06-22):

> “The WSL team has discussed and decided that the **WSL 2 Linux kernel GitHub repo is not taking pull requests**.”  
> Repo intent: *“provide the additional infrastructure necessary to **build and release** the kernel component of WSL 2”* — not replace [microsoft/WSL](https://github.com/microsoft/WSL) as the feedback channel.  
> Config discussion → **WSL GitHub**.  
> General kernel work → **upstream Linux**.

### What *does* get merged on that repo (2025–2026 practice)

Observed on the PR list (~4 open, ~46 closed):

| Pattern | Authors | Example |
| --- | --- | --- |
| Release train | Collaborators (e.g. `chessturo`) | “WSL Kernel 6.6.123.2”, “Release 6.18.20.3” |
| Config size/debug trim | **Members** (`effective-light` / `*@linux.microsoft.com`) | [#259](https://github.com/microsoft/WSL2-Linux-Kernel/pull/259) AMD64 `.config` optimizations — atomic commits `config: disable X` / `config: enable Y`, `Signed-off-by: …@linux.microsoft.com`, “compile and smoke tested” |
| Hyper-V / virt patches | Members (e.g. `benhillis`) | Drafts PCI/hv, virtio_fs |
| Community “enable CONFIG_*” | External | Historically **closed** (#245, #248, #249, socketcan #242, …) |

**Conclusion:** treat `WSL2-Linux-Kernel` as a **source drop + MS internal integration**, not as a community PR forge for config wishlist.

---

## 3. Criteria language / “how they write” (when MS *does* change config)

From open/internal-style config PR #259:

- **Commit subject:** `config: <verb> CONFIG_<SYMBOL>`  
  Examples: `config: disable CONFIG_DRM`, `config: enable CONFIG_PSI_DEFAULT_DISABLED`
- **Body:** short; often just `Signed-off-by: Name <email@linux.microsoft.com>`
- **PR description:** e.g. “compile and smoke tested.”
- **Granularity:** **one logical Kconfig change per commit** (not a giant “optimize everything” blob — that pattern failed in #245 for external + lack of data)
- **Language:** **English** only for commits/PR text  
- **Code language of the tree:** standard **Linux kernel C** (GPLv2, `COPYING`); config files are Kconfig text  
- **What they optimize for (inferred):** smaller attack surface / image (disable DRM, debug, VFIO, …), not “enable every hobby feature”

### What MS said they need for config asks (from #245 close)

- **Enough data** that the change is warranted (telemetry / validated scenarios — vague externally)  
- Prefer **narrow** asks tied to **concrete WSL user scenarios** filed on **microsoft/WSL**  
- Team discusses **internally** after the issue; community PR is not the merge vehicle  

### What a *good* feature request on microsoft/WSL should look like

```text
Title: [Kernel] Enable CONFIG_BLK_DEV_UBLK=m and CONFIG_ZRAM_WRITEBACK=y in config-wsl

Body (English):
- Problem: stock WSL kernel cannot load ublk / zram writeback; rebuild required
- Why general (not one product): advanced multi-tier swap / userspace block backends
- Proposed config only (no out-of-tree module in the ask)
- Risk: UBLK=m (module), ZRAM_WRITEBACK opt-in at runtime
- Evidence: boot test, modprobe ublk_drv, zram writeback attach, no HCS timeout
- Willingness to use custom kernel meanwhile; asking for stock enablement
```

Link any existing issues (search first: ublk, zram writeback, custom kernel config).

**Timeline expectation:** weeks → **months → never**. No SLA. Stock enablement is MS product decision; custom kernel is the reliable engineering path.

---

## 4. How the *code* in that repo is shaped (for us)

```text
microsoft/WSL2-Linux-Kernel
├── arch/x86/...              # normal Linux tree
├── Microsoft/
│   ├── config-wsl            # often symlink → arch/x86/configs/config-wsl
│   ├── config-wsl-arm64
│   └── scripts/              # e.g. gen_modules_vhdx.sh
├── MSFT-Merge/               # MS merge metadata (downstream process)
└── README.md                 # build + “don’t file issues here”
```

- Branches: `linux-msft-wsl-6.6.y` (LTS packaging line), `linux-msft-wsl-6.18.y` (newer rolling line)  
- Versioning: `linux-msft-wsl-<upstream>.<msft rev>`  
- **Always** start from `Microsoft/config-wsl` (Hyper-V / vsock / WSL boot options); generic distro defconfig → boot hang (`HCS_E_CONNECTION_TIMEOUT` class failures)  
- Our lab deltas that matter for P1:

```diff
-# CONFIG_ZRAM_WRITEBACK is not set
+CONFIG_ZRAM_WRITEBACK=y

-# CONFIG_BLK_DEV_UBLK is not set
+CONFIG_BLK_DEV_UBLK=m
```

(`IO_URING`, `ZRAM=m`, `SWAP`, `NBD` already on in stock.)

---

## 5. Correct multi-track strategy (RamShared)

```text
Track A — Product function (now)
  Custom WSL kernel from MS tree + configs above
  → qemu-validate → boot-kernel-safe → ramshared cascade on ublk/writeback
  → “function works” for us without MS merge

Track B — Official stock enablement (advocacy)
  Feature request on github.com/microsoft/WSL (English, narrow, evidence)
  Optional: discuss; do NOT expect merge via WSL2-Linux-Kernel PR

Track C — True kernel novelty (years)
  Upstream Linux (kernel.org) for new mm / device-memory — SSDV3 mainline PRDs
  MS may later rebase WSL onto kernels that already have the feature
```

**Do not** spend SSDV3 IMPL budget on “open PR to WSL2-Linux-Kernel and wait for green”.

---

## 6. If we still write SSDV3 for this

Recommended slug (only if we formalize P1 delivery):

`docs/specs/no-milestone/wsl2-config-ublk-zram-writeback/`

| Artifact | Content |
| --- | --- |
| PRD | P1: deliver custom kernel + validation; **optional** WSL issue (non-blocking) |
| SPEC | Build script, symbols, qemu-validate, boot-kernel-safe, rollback, Kahneman |
| IMPL | Numbers from lab build on `RamShared-Kernel` |

**Out of scope of that SPEC:** merge into MS stock, win11-drill, gha shutdown.

---

## 7. Lab pointers (this machine)

| Item | Path |
| --- | --- |
| Lab distro | `RamShared-Kernel` on `R:\WSL\…` |
| Build log | `R:\WSL\RamShared-Kernel-build\kernel-build.log` |
| Intent patch (local only) | `R:\WSL\RamShared-Kernel-build\0001-config-ublk-zram-writeback.patch` |
| Related decision PRD | `docs/specs/no-milestone/wsl2-native-vram-tier/PRD.md` |

---

## Sources (web)

- [WSL2-Linux-Kernel README](https://github.com/microsoft/WSL2-Linux-Kernel) — bugs/features → WSL; code → upstream  
- [PR #249 comment (tyhicks)](https://github.com/microsoft/WSL2-Linux-Kernel/pull/249) — “we don't take PRs through this repo”  
- [PR #245 close (jiayali-ms)](https://github.com/microsoft/WSL2-Linux-Kernel/pull/245) — repo not taking PRs; use microsoft/WSL  
- [PR list](https://github.com/microsoft/WSL2-Linux-Kernel/pulls) — open work dominated by Members/Collaborators  
- [PR #259](https://github.com/microsoft/WSL2-Linux-Kernel/pull/259) — MS style `config:` commits + smoke test  
- [kernel.org submitting-patches](https://www.kernel.org/doc/html/latest/process/submitting-patches.html)  
