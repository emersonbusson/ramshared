# SPEC — WSL #41054 config-only contribution boundary

> SSDV3 Step 2. Implements [`PRD.md`](PRD.md). This SPEC prepares local,
> public-safe evidence only. It does not build, boot, patch, post, push, or
> submit an external kernel/WSL surface.

## Closed scope

### In now

- Exact source-pinned #41054 configuration evidence.
- Canonical x86 and arm64 path/value matrix.
- Config-only request, no-external-PR route, and product/N3 separation.
- Public-safe English issue draft and human handoff contract.
- Named source, build, package, capability, refusal, and revalidation tests.

### Out now

- Any external issue/comment/PR/patch/email/branch/tag or Microsoft internal
  communication.
- Kernel source changes, dxgkrnl/FORTIFY work, uAPI, driver, host memory tier,
  N3 implementation, swap, GPU pressure, WSL shutdown, reboot, or custom-kernel
  boot.
- RamShared code/CI/configuration, NBD product readiness, release docs,
  `IMPL.md`, validation records, or `MEMORY.md`.

### Source pin

All current assertions in this revision are against
`microsoft/WSL2-Linux-Kernel` commit
`14794180686c2fb6307fbe359c359bec765249f3`:

```text
arch/x86/configs/config-wsl
arch/arm64/configs/config-wsl-arm64
```

The official build command may use `KCONFIG_CONFIG=Microsoft/config-wsl` (or
the arm64 counterpart); the architecture paths above remain the canonical
source facts. Any source/branch/tag/config dependency change returns the lane
to `NEEDS_REVALIDATION`.

## Traceability

| PRD | SPEC item |
| --- | --- |
| RF-U-1..4 | ITEM-1 — source pin and architecture matrix |
| RF-U-5..8 | ITEM-2 — config-only route and separation refusals |
| RF-U-9, NFR-U-2..6 | ITEM-3 — revalidation and maintenance |
| RF-U-10, NFR-U-1/3/4 | ITEM-4 — public-safe evidence and human handoff |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-U-1 | The only public target in this revision is #41054. | Keeps the packet narrow and avoids unrelated contribution narratives. |
| DT-U-2 | The target revision is the exact full SHA `14794180686c2fb6307fbe359c359bec765249f3`. | Short hashes and rolling branches are insufficient evidence. |
| DT-U-3 | `arch/x86/configs/config-wsl` and `arch/arm64/configs/config-wsl-arm64` are canonical facts. | Avoids copying an architecture value from a generic or generated config. |
| DT-U-4 | x86 requests exactly ublk=m and zram writeback=y. | Matches the two missing x86 symbols at the pinned source. |
| DT-U-5 | arm64 is independent: ublk is a candidate; writeback is already y. | Prevents a redundant or inaccurate arm64 request. |
| DT-U-6 | The route is config-only and no community kernel PR is prepared. | The WSL README routes feature requests to WSL and code to normal upstream. |
| DT-U-7 | Capability does not promote NBD/ublk product status. | Kernel symbols and product lifecycle proof are different contracts. |
| DT-U-8 | N3 is a separate host-authority RFC. | Native VRAM ownership cannot be created by this config request. |
| DT-U-9 | A source mismatch refuses before any build or packet update. | Prevents stale evidence and accidental target drift. |
| DT-U-10 | Human handoff is the terminal state of this pack. | No external network write is authorized. |

## Atomicity/rollback

| Layer | This SPEC | Rollback trigger |
| --- | --- | --- |
| Source identity | Read-only target SHA/path/value capture. | Any mismatch → `NEEDS_REVALIDATION`; discard the packet. |
| Config delta | Two x86 symbol values only; arm64 recorded separately. | Unexpected symbol/dependency/path change → refuse. |
| Build evidence | Later target-tree build in an isolated worktree. | Build/package/check failure → `PARTIAL`/`REFUSED`; no retry theater. |
| Capability | Later approved custom-kernel capability only, without swap/pressure. | Capability failure → retain NBD product; no adoption claim. |
| External route | No write in this SPEC; local packet only. | Any attempted issue/PR/push → hard stop and discard outbound action. |
| Product policy | NBD/N3 owners unchanged. | Any transport or native-memory change → scope refusal. |

No module unload, host restart, or external rollback is hidden in this pack.
If a future human-approved custom-kernel boot fails, use that kernel’s approved
recovery path and mark the row `PARTIAL`; this SPEC does not authorize the boot.

## Kahneman map

| # | Question | Required evidence | Abort |
| --- | --- | --- | --- |
| #2 | Does a config symbol prove product transport or native memory? | `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL`, `N3_SCOPE_REFUSAL` | Any product/N3 claim from capability. |
| #3 | Is the public source and architecture path exact? | `UPSTREAM_SOURCE_SHA_REVALIDATION` | Missing/full-SHA/path mismatch. |
| #9 | Are config/build results numeric and reproducible? | `UPSTREAM_CONFIG_EVIDENCE_NUMBERS` | Text-only “works” claim. |
| #13 | Are valid and refusal boundaries paired? | `X86_CONFIG_PAIR`, `ARM64_INDEPENDENT_PAIR`, `NO_EXTERNAL_PR_REFUSAL` | Fail-open source/route state. |
| #15 | Is a failure transient? | `UPSTREAM_DETERMINISTIC_FAILURE_NO_RETRY` | Blind retry of mismatch or build failure. |
| #16 | Can custom-kernel safety stop independently? | `UPSTREAM_BOOT_ROLLBACK_BOUNDARY` | Any unsafe host/boot action. |
| #17 | Does evidence replay have one effect? | `UPSTREAM_EVIDENCE_REPLAY_IDEMPOTENCY` | Duplicate packet or external action. |
| #18 | Which owner decides? | `UPSTREAM_OWNER_ROUTE`, `N3_HOST_OWNER_BOUNDARY` | RamShared guest decides Microsoft/N3 ownership. |

## Security checklist

- [x] No external write, secret, credential, private host path, username, IP,
  raw log, dump, account ID, or internal route is included.
- [x] Full SHA and canonical file paths are captured before interpreting values.
- [x] Config parser/build inputs are bounded and source-pinned.
- [x] No generic defconfig, guessed maintainer, mailing list, or target branch
  is substituted for the official target source.
- [x] Capability tests do not create swap, run pressure, or alter product
  transport; missing approval is `PARTIAL`/`REFUSED`.
- [x] No kernel/module/host action is authorized by a documentation packet.
- [x] N3 refusal preserves host authority; #41054 cannot smuggle a native tier.
- [ ] DMA/MMIO/IRQ/uAPI/lifetime: N/A for this config-only documentation
  slice; a kernel code change requires a new target-tree SPEC.

## Files create/modify/delete

The following paths are future campaign anchors. None is changed by this docs
revision except the three documents in this folder.

| Path | Action | Contract / test owner |
| --- | --- | --- |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/PRD.md` | Modify | Source-pinned decision. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/SPEC.md` | Modify | This executable matrix. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/CONTRIBUTION-CHECKLIST.md` | Modify | Human packet, issue drafts, milestone mapping. |
| `scripts/kernel/` in the exact external target tree | Future read-only/build context | Official target-tree commands only; no RamShared code. |
| `Microsoft/config-wsl*` in the exact external target tree | Read-only target input | Build entry point resolves to canonical architecture files. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/IMPL.md` | Create later | Only after approved campaign and evidence. |
| `validation.md` | Append later | Only for authorized live evidence; not this task. |

No file in the RamShared code, CI/workflow, release, validation, or MEMORY
surfaces is created, modified, or deleted by this SPEC.

## Observability

| Signal | Required record | Pass condition |
| --- | --- | --- |
| Target | repository, branch/tag, full SHA, date | Exact pinned source. |
| Paths | canonical x86/arm64 file and resolved build entry | Both architecture rows resolve. |
| Values | ublk/writeback state per architecture | x86 pair is exact; arm64 writeback is already y. |
| Build | target-tree command class, toolchain, exit, warning/error summary | Official gate result is recorded or partial. |
| Capability | module/control-node observation without workload | Capability only, never product ready. |
| Route | issue draft state and external action | `HUMAN_REVIEW_ONLY`; no network write. |
| Revalidation | trigger and reason | Branch/SHA/Kconfig/30-day drift → `NEEDS_REVALIDATION`. |

## Living docs

| Document | Action |
| --- | --- |
| `wsl2-nbd-product-readiness/` | NBD product owner; no status imported from this lane. |
| `microsoft-native-vram-memory-tier/` | N3 RFC owner; no config adoption imported. |
| `docs/labs/WSL2-UPSTREAM-CONFIG-PR.md` | Historical research; do not rewrite it as an external PR plan. |
| `docs/INDEX.md` | Regenerate as a generated index after pack updates. |
| `IMPL.md`, `validation.md`, release docs | N/A in this docs-only revision. |
| `AGENTS.md`, `CLAUDE.md`, `.claude/rules/*` | N/A — no convention change. |

## Implementation order

1. **ITEM-1:** resolve the exact target SHA and canonical architecture files.
2. **ITEM-2:** record the x86 pair, arm64 independent state, and config-only refusal boundaries.
3. **ITEM-3:** run later official source/build/package/capability gates on an approved isolated surface.
4. **ITEM-4:** render the English human-review draft and milestone mapping; do not post.
5. **ITEM-5:** revalidate at every trigger and hand off to a human decision.

## Required tests matrix

These are contractual names for a future target-tree campaign. They have not
run in this documentation-only revision.

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover / pass condition |
| --- | --- | --- | --- | --- |
| Target repository | `UPSTREAM_SOURCE_SHA_REVALIDATION` | source/static | #3 | Full SHA equals `14794180686c2fb6307fbe359c359bec765249f3`. |
| Target repository | `UPSTREAM_CANONICAL_ARCH_PATHS` | source/static | #3/#13 | Both exact arch config files exist and are the reviewed inputs. |
| `arch/x86/configs/config-wsl` | `X86_CONFIG_PAIR` | config/static | #13 | x86 current values are not set; candidate delta is exactly ublk=m + writeback=y. |
| `arch/arm64/configs/config-wsl-arm64` | `ARM64_INDEPENDENT_PAIR` | config/static | #13 | ublk is candidate; writeback is already y; no duplicate request. |
| Target Kconfig | `UPSTREAM_CONFIG_OLDDEFCONFIG_X86` | build | #3 | Both x86 deltas survive official `olddefconfig`. |
| Target Kconfig | `UPSTREAM_CONFIG_OLDDEFCONFIG_ARM64` | build | #3 | Arm64 is independently evaluated; no copied x86 result. |
| Target build | `UPSTREAM_CONFIG_BUILD_W1` | build | #3 | Official target-tree build result is recorded. |
| Target build | `UPSTREAM_CONFIG_SPARSE_C1` | sparse | #13 | No new relevant sparse diagnostic, or explicit partial/refusal. |
| Target package | `UPSTREAM_CONFIG_MODULE_PACKAGE` | package | #16 | Expected ublk module/package state is recorded without swap. |
| Approved custom kernel | `UPSTREAM_CONFIG_UBLK_CAPABILITY_NO_PRODUCT` | drill/E2E | #2/#18 | Capability is observed but product remains NBD-only. |
| Approved custom kernel | `UPSTREAM_CONFIG_WRITEBACK_CAPABILITY` | drill/E2E | #2 | Writeback capability is recorded per architecture; no pressure workload. |
| Human packet | `UPSTREAM_CONFIG_PUBLIC_REDACTION` | static/manual | #13 | No private/secret/raw host material. |
| Human packet | `NO_EXTERNAL_KERNEL_PR_REFUSAL` | refusal | #13/#18 | No branch, patch, PR, push, or auto-post action. |
| Human packet | `UPSTREAM_OWNER_ROUTE` | static/manual | #18 | #41054 stays WSL feature request; code route is normal upstream, not community PR. |
| N3 boundary | `N3_SCOPE_REFUSAL` | refusal | #2/#18 | N3 host RFC remains a separate pack. |
| Evidence process | `UPSTREAM_EVIDENCE_REPLAY_IDEMPOTENCY` | process | #17 | Replaying a packet creates no external or host effect. |
| Maintenance | `UPSTREAM_REVALIDATION_TRIGGER` | process | #3/#15 | SHA/path/branch/Kconfig/build/30-day drift yields `NEEDS_REVALIDATION`. |

### Platform-correct future gates

When explicitly authorized on the exact external target tree, use the target
tree’s own commands. The following are examples of command classes, not an
authorization to run them in this task:

```bash
make KCONFIG_CONFIG=Microsoft/config-wsl olddefconfig
make KCONFIG_CONFIG=Microsoft/config-wsl W=1
make KCONFIG_CONFIG=Microsoft/config-wsl C=1
make INSTALL_MOD_PATH="$PWD/modules" modules_install
```

For arm64, resolve `Microsoft/config-wsl-arm64` and
`arch/arm64/configs/config-wsl-arm64` from the exact target tree. Do not use a
generic distro defconfig. Do not run `modprobe`, swap, pressure, or a boot as
part of this documentation task.

## Validation checklist

Future campaign only:

- [ ] Read the exact external target README and source at the pinned SHA.
- [ ] Verify both canonical architecture paths and all four symbol values.
- [ ] Run official build/sparse/package gates and record status per architecture.
- [ ] If custom-kernel capability is approved, use isolated before/action/after
  evidence with no swap or pressure; capability remains non-product.
- [ ] Pair x86/arm64 legitimate values with mismatch/refusal cases.
- [ ] Pair config capability with `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL`.
- [ ] Pair #41054 request with `NO_EXTERNAL_KERNEL_PR_REFUSAL`.
- [ ] Keep N3 separate with `N3_SCOPE_REFUSAL`.
- [ ] Keep missing environment evidence `PARTIAL`; do not update validation in
  this docs-only turn.

## Out of SPEC

- Any kernel code, dxgkrnl/FORTIFY patch, external kernel PR, GitHub action,
  issue/comment, mailing-list submission, branch push, or internal route;
- any NBD/ublk product implementation, native VRAM/N3 implementation, custom
  kernel promotion, swap/pressure/GPU workload, reboot, or WSL shutdown;
- any release, code, CI/workflow, `IMPL.md`, validation, or `MEMORY.md` edit.
