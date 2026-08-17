# SPEC — WSL #41054 config-only contribution boundary

> SSDV3 Steps 2–3. Implements [`PRD.md`](PRD.md). This SPEC prepares and
> validates a public-safe config-only patch series, publishes reviewed evidence
> through the issue-first route, and refuses an unsolicited Microsoft PR.

## Closed scope

### In now

- Exact source-pinned #41054 configuration evidence.
- Canonical x86 and arm64 path/value matrix.
- Config-only request, no-external-PR route, and product/N3 separation.
- Public-safe English issue update and human handoff contract.
- Isolated external-tree config, build, package, Sparse, and QEMU capability
  evidence.
- A two-commit branch in the author's fork after all mandatory gates pass.
- Named source, build, package, capability, refusal, and revalidation tests.

### Out now

- Any unsolicited PR to `microsoft/WSL2-Linux-Kernel`, email, tag, or claim of
  Microsoft internal adoption.
- Kernel source changes, dxgkrnl/FORTIFY work, uAPI, driver, host memory tier,
  N3 implementation, swap, GPU pressure, WSL shutdown, reboot, or custom-kernel
  boot.
- RamShared product code/CI/configuration, NBD product readiness, release docs,
  or validation records.

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
| RF-U-11 | ITEM-5 — maintainer-routed patch/PR decision |
| RF-U-12 | ITEM-3 — generated-config dependency disclosure |

## Technical decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-U-1 | The only public target in this revision is #41054. | Keeps the packet narrow and avoids unrelated contribution narratives. |
| DT-U-2 | The target revision is the exact full SHA `14794180686c2fb6307fbe359c359bec765249f3`. | Short hashes and rolling branches are insufficient evidence. |
| DT-U-3 | `arch/x86/configs/config-wsl` and `arch/arm64/configs/config-wsl-arm64` are canonical facts. | Avoids copying an architecture value from a generic or generated config. |
| DT-U-4 | x86 requests exactly ublk=m and zram writeback=y. | Matches the two missing x86 symbols at the pinned source. |
| DT-U-5 | arm64 is independent: ublk is a candidate; writeback is already y. | Prevents a redundant or inaccurate arm64 request. |
| DT-U-6 | The route is config-only and issue-first; an unsolicited community kernel PR is refused. | The WSL README and maintainer comments route feature requests to WSL and integration to Microsoft. |
| DT-U-7 | Capability does not promote NBD/ublk product status. | Kernel symbols and product lifecycle proof are different contracts. |
| DT-U-8 | N3 is a separate host-authority RFC. | Native VRAM ownership cannot be created by this config request. |
| DT-U-9 | A source mismatch refuses before any build or packet update. | Prevents stale evidence and accidental target drift. |
| DT-U-10 | One reviewed #41054 update is the terminal outbound action before maintainer response. | Prevents comment churn and unsolicited PR submission. |
| DT-U-11 | Patch readiness and PR authorization are separate states. | Public history shows technically accepted work can be applied internally while the external PR is closed. |
| DT-U-12 | `CONFIG_BLKDEV_UBLK_LEGACY_OPCODES=y` is recorded as an upstream-derived default, not a requested source line. | Prevents a two-line source claim from hiding a generated compatibility/security value. |

## Atomicity/rollback

| Layer | This SPEC | Rollback trigger |
| --- | --- | --- |
| Source identity | Read-only target SHA/path/value capture. | Any mismatch → `NEEDS_REVALIDATION`; discard the packet. |
| Config delta | Two x86 symbol values only; arm64 recorded separately. | Unexpected symbol/dependency/path change → refuse. |
| Build evidence | Later target-tree build in an isolated worktree. | Build/package/check failure → `PARTIAL`/`REFUSED`; no retry theater. |
| Capability | Later approved custom-kernel capability only, without swap/pressure. | Capability failure → retain NBD product; no adoption claim. |
| External route | Reviewed fork branch plus one #41054 evidence update; Microsoft PR only on explicit maintainer request. | Any broader write or unsolicited PR → hard stop. |
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

- [x] No secret, credential, private host path, username, IP, raw log, dump,
  account ID, or internal route is included in public material.
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

The following paths are campaign anchors. External target-tree files live only
in the isolated candidate branch; RamShared changes remain in this folder and
the generated documentation inventory.

| Path | Action | Contract / test owner |
| --- | --- | --- |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/PRD.md` | Modify | Source-pinned decision. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/SPEC.md` | Modify | This executable matrix. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/CONTRIBUTION-CHECKLIST.md` | Modify | Human packet, issue drafts, milestone mapping. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/RESEARCH.md` | Create | Current PR/issue route evidence and maintained decision. |
| `scripts/kernel/` in the exact external target tree | Future read-only/build context | Official target-tree commands only; no RamShared code. |
| `Microsoft/config-wsl*` in the exact external target tree | Read-only target input | Build entry point resolves to canonical architecture files. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/IMPL.md` | Create | Exact campaign and public-action evidence. |
| `docs/specs/no-milestone/wsl2-upstream-native-contribution/EVIDENCE.json` | Create | Public-safe machine-readable source, patch, build, capability, and route receipts. |
| `scripts/kernel/test-wsl-upstream-config-contribution.sh` | Create | Source, architecture, scope, DCO, checkpatch, and apply refusal gates. |
| `scripts/kernel/qemu-wsl-config-capability.sh` | Create | Isolated QEMU boot, module, writeback I/O, and teardown gate. |
| `scripts/kernel/qemu-wsl-config-capability-init.sh` | Create | Minimal public-safe capability guest. |
| `validation.md` | Append later | Only for authorized live evidence; not this task. |

No file in the RamShared product code, CI/workflow, release, or validation
surfaces is created, modified, or deleted by this SPEC.

## Observability

| Signal | Required record | Pass condition |
| --- | --- | --- |
| Target | repository, branch/tag, full SHA, date | Exact pinned source. |
| Paths | canonical x86/arm64 file and resolved build entry | Both architecture rows resolve. |
| Values | ublk/writeback state per architecture | x86 pair is exact; arm64 writeback is already y. |
| Build | target-tree command class, toolchain, exit, warning/error summary | Official gate result is recorded or partial. |
| Capability | module/control-node observation without workload | Capability only, never product ready. |
| Route | issue update, fork branch, and PR gate | One reviewed issue update; no unsolicited Microsoft PR. |
| Revalidation | trigger and reason | Branch/SHA/Kconfig/30-day drift → `NEEDS_REVALIDATION`. |

## Living docs

| Document | Action |
| --- | --- |
| `wsl2-nbd-product-readiness/` | NBD product owner; no status imported from this lane. |
| `microsoft-native-vram-memory-tier/` | N3 RFC owner; no config adoption imported. |
| `docs/labs/WSL2-UPSTREAM-CONFIG-PR.md` | Historical research; do not rewrite it as an external PR plan. |
| `docs/INDEX.md` | Regenerate as a generated index after pack updates. |
| `IMPL.md` | Create with exact campaign evidence and residuals. |
| `validation.md`, release docs | N/A; this campaign is not product validation. |
| `AGENTS.md`, `CLAUDE.md`, `.claude/rules/*` | N/A — no convention change. |

## Implementation order

1. **ITEM-1:** resolve the exact target SHA and canonical architecture files.
2. **ITEM-2:** record the x86 pair, arm64 independent state, and config-only refusal boundaries.
3. **ITEM-3:** run later official source/build/package/capability gates on an approved isolated surface.
4. **ITEM-4:** render and post one reviewed English #41054 evidence update after all gates pass.
5. **ITEM-5:** revalidate at every trigger and ask for the maintainer-owned
   integration route; a PR remains refused until explicitly requested.

## Required tests matrix

These are the contractual names for the approved target-tree campaign. Exact
results are recorded in `IMPL.md`.

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover / pass condition |
| --- | --- | --- | --- | --- |
| Target repository | `UPSTREAM_SOURCE_SHA_REVALIDATION` | source/static | #3 | Full SHA equals `14794180686c2fb6307fbe359c359bec765249f3`. |
| Target repository | `UPSTREAM_CANONICAL_ARCH_PATHS` | source/static | #3/#13 | Both exact arch config files exist and are the reviewed inputs. |
| `scripts/kernel/test-wsl-upstream-config-contribution.sh` | `UPSTREAM_PATCH_SERIES_SCOPE` | source/static | #3/#13 | Exactly two commits change only the two canonical config files. |
| `scripts/kernel/test-wsl-upstream-config-contribution.sh` | `UPSTREAM_PATCH_DCO_AND_CHECKPATCH` | source/static | #13 | Canonical author/sign-off, strict checkpatch, and clean apply all pass. |
| `arch/x86/configs/config-wsl` | `X86_CONFIG_PAIR` | config/static | #13 | x86 current values are not set; candidate delta is exactly ublk=m + writeback=y. |
| `arch/arm64/configs/config-wsl-arm64` | `ARM64_INDEPENDENT_PAIR` | config/static | #13 | ublk is candidate; writeback is already y; no duplicate request. |
| Target Kconfig | `UPSTREAM_CONFIG_OLDDEFCONFIG_X86` | build | #3 | Both x86 deltas survive official `olddefconfig`. |
| Target Kconfig | `UPSTREAM_CONFIG_OLDDEFCONFIG_ARM64` | build | #3 | Arm64 is independently evaluated; no copied x86 result. |
| Target Kconfig | `UPSTREAM_CONFIG_DERIVED_DEFAULTS` | config/static | #2/#3 | Both generated configs disclose `CONFIG_BLKDEV_UBLK_LEGACY_OPCODES=y`. |
| Target build | `UPSTREAM_CONFIG_BUILD_W1` | build | #3 | Official target-tree build result is recorded. |
| Target build | `UPSTREAM_CONFIG_SPARSE_C1` | sparse | #13 | No new relevant sparse diagnostic, or explicit partial/refusal. |
| Target package | `UPSTREAM_CONFIG_MODULE_PACKAGE` | package | #16 | Expected ublk module/package state is recorded without swap. |
| Approved custom kernel | `UPSTREAM_CONFIG_UBLK_CAPABILITY_NO_PRODUCT` | drill/E2E | #2/#18 | Capability is observed but product remains NBD-only. |
| Approved custom kernel | `UPSTREAM_CONFIG_WRITEBACK_CAPABILITY` | drill/E2E | #2 | Writeback capability is recorded per architecture; no pressure workload. |
| Human packet | `UPSTREAM_CONFIG_PUBLIC_REDACTION` | static/manual | #13 | No private/secret/raw host material. |
| Human packet | `NO_EXTERNAL_KERNEL_PR_REFUSAL` | refusal | #13/#18 | No unsolicited Microsoft-repository PR or auto-post action. |
| Human packet | `UPSTREAM_OWNER_ROUTE` | static/manual | #18 | #41054 stays WSL feature request; code route is normal upstream, not community PR. |
| Route audit | `UPSTREAM_PR_ROUTE_AUDIT` | source/static | #3/#18 | Public PR population and decisive maintainer comments support the selected route. |
| Human packet | `MAINTAINER_REQUESTED_PR_GATE` | refusal | #13/#18 | A patch-ready state without an explicit maintainer request cannot open a PR. |
| N3 boundary | `N3_SCOPE_REFUSAL` | refusal | #2/#18 | N3 host RFC remains a separate pack. |
| Evidence process | `UPSTREAM_EVIDENCE_REPLAY_IDEMPOTENCY` | process | #17 | Revalidation creates no duplicate issue comment or host effect. |
| Maintenance | `UPSTREAM_REVALIDATION_TRIGGER` | process | #3/#15 | SHA/path/branch/Kconfig/build/30-day drift yields `NEEDS_REVALIDATION`. |

### Platform-correct future gates

The user explicitly authorized this campaign on the exact external target
tree. Use the target tree's own commands in isolated build directories:

```bash
make KCONFIG_CONFIG=Microsoft/config-wsl olddefconfig
make KCONFIG_CONFIG=Microsoft/config-wsl W=1
make KCONFIG_CONFIG=Microsoft/config-wsl C=1
make INSTALL_MOD_PATH="$PWD/modules" modules_install
```

For arm64, resolve `Microsoft/config-wsl-arm64` and
`arch/arm64/configs/config-wsl-arm64` from the exact target tree. Do not use a
generic distro defconfig. Capability boots must be isolated QEMU guests and
must not run host swap or pressure.

## Validation checklist

Approved campaign:

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

- Any kernel code, dxgkrnl/FORTIFY patch, unsolicited external kernel PR,
  mailing-list submission, tag, or claim of internal adoption;
- any NBD/ublk product implementation, native VRAM/N3 implementation, custom
  kernel promotion, swap/pressure/GPU workload, reboot, or WSL shutdown;
- any release, product code, CI/workflow, validation, or `MEMORY.md` edit.
