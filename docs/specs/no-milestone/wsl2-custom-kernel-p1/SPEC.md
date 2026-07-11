# SPEC — wsl2-custom-kernel-p1

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

---

## Files

| Path | Action |
| --- | --- |
| `scripts/kernel/wsl-kernel.sh` | **create** — primary in-WSL CLI |
| `scripts/kernel/wsl-kernel-lib.sh` | **create** — shared probes, exit codes, paths |
| `scripts/kernel/build-wsl-kernel.sh` | **extend** — default KTAG `linux-msft-wsl-6.18.y`; write release + stamp dir on R: |
| `scripts/kernel/qemu-validate.sh` | **reuse** — call site only; write PASS stamp |
| `scripts/kernel/boot-kernel-safe.ps1` | **extend** — defaults KernelPath → `R:\WSL\kernels\bzImage-ramshared-latest`; ExpectedVersion from stamp; TimeoutSec **60** (PRD allow ≤120) |
| `scripts/kernel/boot-kernel-logged.ps1` | **reuse** as apply wrapper log |
| `R:\WSL\kernels\` | **runtime** — bzImage artifacts (not in git) |
| `R:\WSL\RamShared-Kernel-build\` | **runtime** — logs, stamps, intent patch |
| `docs/runbooks/FASE-B-KERNEL.md` | **update** — point to this SPEC; CLI first |
| `docs/labs/WSL-KERNEL-LAB.md` | **update** — build vs enable |
| `validation.md` | **append** — gates with numbers |
| `docs/specs/.../IMPL.md` | Passo 3 |
| `docs/specs/.../AUDIT-2.5.md` | before first apply |

**Not in repo (host paths):**

| Path | Role |
| --- | --- |
| `R:\WSL\kernels\bzImage-ramshared-latest` | Active custom image |
| `R:\WSL\RamShared-Kernel-build\release.txt` | `REL=…` `KTAG=…` `HEAD=…` |
| `R:\WSL\RamShared-Kernel-build\qemu-pass.stamp` | ITEM-3 stamp for apply gate |
| `%UserProfile%\.wslconfig` | `kernel=` arm target |
| `%UserProfile%\.wslconfig.ramshared.bak` | last pre-arm backup (SPEC freeze name) |
| `C:\wsl\wslconfig-original.txt` | clean stock config (no `kernel=`) for revert — keep compatibility with existing launcher |

---

## Constants (freeze)

| Name | Value |
| --- | --- |
| `KTAG_DEFAULT` | `linux-msft-wsl-6.18.y` |
| `KERNEL_WIN` | `R:\WSL\kernels\bzImage-ramshared-latest` |
| `KERNEL_WSL` | `/mnt/r/WSL/kernels/bzImage-ramshared-latest` |
| `BUILD_DIR_WIN` | `R:\WSL\RamShared-Kernel-build` |
| `BUILD_DIR_WSL` | `/mnt/r/WSL/RamShared-Kernel-build` |
| `MIN_BZIMAGE_BYTES` | `1048576` (1 MiB) |
| `ENABLE_TIMEOUT_SEC` | `30` |
| `INTEROP_FAIL_SEC` | `5` |
| `APPLY_TIMEOUT_SEC` | `60` (align `boot-kernel-safe.ps1`; PRD max 120) |
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

- **Preferred build host:** WSL distro `RamShared-Kernel` (lab on R:).  
- Product default distro remains `Ubuntu-24.04` (`wsl --set-default` must not flip to lab).  
- Build tree default: `$HOME/src/WSL2-Linux-Kernel` inside lab (may grow; lab VHD 40G — monitor `df`).

### 1.2 Algorithm (`build-wsl-kernel.sh` or lab `build-kernel-lab.sh` — same semantics)

1. `KTAG=${KTAG:-linux-msft-wsl-6.18.y}`  
2. Clone if missing: `git clone --depth 1 --branch "$KTAG" https://github.com/microsoft/WSL2-Linux-Kernel.git "$KSRC"`  
3. `cp Microsoft/config-wsl .config`  
4. Apply ITEM-2 deltas via `./scripts/config`  
5. `make olddefconfig`  
6. **Verify** each CONFIG_DELTAS with grep; fail if not sticky  
7. `make -j${JOBS:-2}` then `sudo make modules_install`  
8. `REL=$(make -s kernelrelease)`  
9. `cp arch/x86/boot/bzImage` →  
   - `$BUILD_DIR_WSL/../kernels/bzImage-$REL`  
   - `$KERNEL_WSL` (latest symlink-or-copy)  
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

- Config not sticky → exit 1, no latest overwrite if previous latest exists (prefer write versioned name first, then `cp` to latest only on full success).

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
| `P_BZ` | `[[ -f $KERNEL_WSL && $(stat -c%s) -gt MIN_BZIMAGE_BYTES ]]` |
| `P_REL` | `grep REL= release.txt` if present |
| `P_UNAME` | `uname -r` |
| `P_CUSTOM_RUNNING` | `P_REL` non-empty AND (`uname -r` equals REL **or** uname contains distinct custom marker written at build — freeze: **prefer exact REL match**; if MS-style name equals stock line, SPEC uses `REL` from release.txt only) |
| `P_CFG_ARMED` | `.wslconfig` contains `kernel=` pointing at `KERNEL_WIN` (normalize slashes) |
| `P_UBLK` | `lsmod | grep -q '^ublk_drv'` OR `/sys/module/ublk_drv` exists OR `modprobe -n ublk_drv` succeeds |
| `P_STAMP` | qemu-pass.stamp valid (ITEM-3) |

**Reading `.wslconfig` from WSL (order):**

1. `$WSL_CONFIG` env override  
2. `/mnt/c/Users/$WIN_USER/.wslconfig` where `WIN_USER` from `cmd.exe /c echo %USERNAME%` with **timeout INTEROP_FAIL_SEC**  
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
| NEED_MODULE | `sudo modprobe ublk_drv` (and deps if needed); re-probe | 0 if ok else 2 |
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
| #15 | modprobe fail (ENOENT) → no retry loop |

---

## ITEM-5 — CLI `arm` / `disarm`

**RF-K18, RF-K20, RF-K16**

### 5.1 `arm`

1. Require `P_BZ` else exit 2.  
2. Resolve Windows path `KERNEL_WIN`.  
3. Backup current `.wslconfig` → `.wslconfig.ramshared.bak` (overwrite one slot).  
4. Ensure clean original exists for apply-revert: if `C:\wsl\wslconfig-original.txt` missing and current has no `kernel=`, copy current there (interop).  
5. Atomic write: temp file in same dir → replace (or PowerShell helper with Set-CfgRetry from boot-kernel-safe).  
6. Set under `[wsl2]`: `kernel=R:\\WSL\\kernels\\bzImage-ramshared-latest` (double backslash style as existing PS).  
7. Idempotent: second arm same path → exit 0.  
8. **No** `wsl --shutdown`.  
9. Print: `ARMED next WSL start will use custom kernel (NEED_REBOOT until then)`. Exit 0.

Implementation preference: call small PowerShell snippet via interop with **INTEROP_FAIL_SEC**; on fail exit 3 + paste:

```powershell
# example printed, exact text frozen in IMPL
```

### 5.2 `disarm`

1. Remove all `kernel=` lines from `.wslconfig` (same as Disarm-Config).  
2. No shutdown.  
3. Exit 0 if already disarmed.

### Kahneman

| # | Rule |
| --- | --- |
| #17 | arm 2× same path = one kernel= line |
| #2 | Missing bzImage → refuse arm |

---

## ITEM-6 — CLI `apply` (disruptive)

**RF-K6, RF-K15, RF-K19, NFR-K4**

### 6.1 Invocation

```bash
bash scripts/kernel/wsl-kernel.sh apply --i-know-this-stops-all-wsl
```

Without flag → exit 5, print help. No side effects.

### 6.2 Preconditions

1. `P_BZ`  
2. Valid `qemu-pass.stamp` (ITEM-3) matching current bzImage sha  
3. Interop OK or fail 3  

### 6.3 Steps

1. Print warning: all WSL distros will stop.  
2. Invoke elevated/non-elevated as needed:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File <repo>/scripts/kernel/boot-kernel-logged.ps1 `
  -KernelPath 'R:\WSL\kernels\bzImage-ramshared-latest' `
  -ExpectedVersion '<REL from release.txt>' `
  -TimeoutSec 60 `
  -CheckModules 'ublk_drv'
```

(Path to ps1 via `/mnt/c/...` or repo-on-Windows — SPEC: resolve `REPO_WIN` as `wslpath -w` of git root.)

3. On success: exit 0; state should become READY or NEED_MODULE.  
4. On failure: boot-kernel-safe must disarm; CLI exit 4.

### 6.4 Agent policy

Automation/agents **must not** run `apply` without explicit human flag in the same turn. Document in script header.

### Kahneman

| # | Rule |
| --- | --- |
| #2 | Timeout 60s → revert |
| #16 | Revert path independent of custom kernel health |
| #13 | Boot success + modprobe warn ≠ auto-revert (match existing PS comment) |

---

## ITEM-7 — Product defaults + cascade non-regression

**RF-K7, RF-K9**

1. Never `wsl --set-default RamShared-Kernel` in any script in this SPEC.  
2. Docs: Day-1 `ramshared up` remains NBD on stock kernel.  
3. `wsl-kernel enable` is **not** a substitute for `ramshared up`.  
4. FASE-B-KERNEL.md: first paragraph links this SPEC; “enable = no-op when ready”.

---

## ITEM-8 — Module proof

**RF-K8**

On READY or after successful apply:

```bash
sudo modprobe ublk_drv
ls /dev/ublk-control   # or documented ublk control node for this kernel version
# zram writeback: document check from kernel docs (e.g. zram sysfs writeback path exists)
```

Record in IMPL/validation.md: kernelrelease, modprobe exit, path existence — no thrash.

---

## ITEM-9 — Host safety + retry

**NFR-K1, NFR-K10**

| Operation | Retry? |
| --- | --- |
| `.wslconfig` share violation | yes, ≤6 × 800ms (existing PS) |
| modprobe ENOENT | **no** |
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
| V9 | apply with stamp+flag — human only; auto-revert drill once | yes |
| V10 | default distro still Ubuntu-24.04 | yes |

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
- Changing default distro to lab  
- Cascade ublk as required (later SPEC)  
- HMM / VRAM-as-RAM  
- Stopping gha  
- win11-drill  

---

## Explicit handoff

| Next | When |
| --- | --- |
| Implement ITEM-4 status/enable | Now (safe) |
| AUDIT-2.5 | After ITEM-3/6 designed/impl ready, **before** first real apply |
| IMPL.md | When V1–V8 green; V9 human-gated |

**SPEC status:** ready for implementation of non-apply items; apply gated by AUDIT-2.5 go.
