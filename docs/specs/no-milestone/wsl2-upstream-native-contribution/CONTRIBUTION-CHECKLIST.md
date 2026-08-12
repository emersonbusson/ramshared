# Public-safe contribution checklist — WSL #41054 config-only request

> Preparation only. This file renders a local human-review packet. It must not
> post an issue, comment, pull request, patch, email, branch, tag, or milestone
> change. All material is English and public-safe.

## 1. Safety envelope

- [ ] Confirm the packet is for **#41054 configuration only**.
- [ ] Confirm the exact target is
      `14794180686c2fb6307fbe359c359bec765249f3`.
- [ ] Confirm the canonical files are exactly:
      `arch/x86/configs/config-wsl` and
      `arch/arm64/configs/config-wsl-arm64`.
- [ ] Confirm x86 requests only:

  ```text
  CONFIG_BLK_DEV_UBLK=m
  CONFIG_ZRAM_WRITEBACK=y
  ```

- [ ] Confirm arm64 is independent: ublk is a candidate and
      `CONFIG_ZRAM_WRITEBACK=y` is already on; no duplicate writeback request.
- [ ] Confirm no driver code, UAPI, native VRAM, NBD product, ublk product,
      or N3 host-contract request appears in the packet.
- [ ] Confirm no community PR to `microsoft/WSL2-Linux-Kernel` is proposed.
- [ ] Confirm no private path, username, machine/account identifier, IP,
      token, dump, screenshot, raw log, or internal Microsoft route appears.
- [ ] Confirm missing build/boot/host evidence is `PARTIAL`, `REFUSED`, or
      `NEEDS_REVALIDATION`, never `DONE`.

Passing this section is `UPSTREAM_CONFIG_PUBLIC_REDACTION` plus
`NO_EXTERNAL_KERNEL_PR_REFUSAL`.

## 2. Source and architecture evidence sheet

| Field | Required value |
| --- | --- |
| Repository | `microsoft/WSL2-Linux-Kernel` |
| Target revision | `14794180686c2fb6307fbe359c359bec765249f3` |
| x86 source | `arch/x86/configs/config-wsl` |
| arm64 source | `arch/arm64/configs/config-wsl-arm64` |
| Build entry | `Microsoft/config-wsl` (resolve in target tree) |
| arm64 build entry | `Microsoft/config-wsl-arm64` where supported by target tree |
| Revalidation UTC | `<date>` |
| Source result | `PASS` / `PARTIAL` / `NEEDS_REVALIDATION` |

| Architecture | Current `CONFIG_BLK_DEV_UBLK` | Current `CONFIG_ZRAM_WRITEBACK` | Requested action |
| --- | --- | --- | --- |
| x86 | `not set` at the pinned SHA | `not set` at the pinned SHA | Request `m` and `y`, respectively. |
| arm64 | `not set` at the pinned SHA | `y` at the pinned SHA | Keep ublk as candidate; request no writeback delta. |

Use the exact target files, not a generated distro config or an older branch.
Any branch/tag/SHA/path/Kconfig dependency change invalidates this sheet.

## 3. Evidence status contract

| Test ID | Status | Evidence reference |
| --- | --- | --- |
| `UPSTREAM_SOURCE_SHA_REVALIDATION` | `UNVERIFIED` | `<public target + full SHA>` |
| `UPSTREAM_CANONICAL_ARCH_PATHS` | `UNVERIFIED` | `<both exact paths>` |
| `X86_CONFIG_PAIR` | `UNVERIFIED` | `<two current values + exact delta>` |
| `ARM64_INDEPENDENT_PAIR` | `UNVERIFIED` | `<ublk candidate/writeback already y>` |
| `UPSTREAM_CONFIG_OLDDEFCONFIG_X86` | `PARTIAL` until run | `<target-tree result>` |
| `UPSTREAM_CONFIG_OLDDEFCONFIG_ARM64` | `PARTIAL` until run | `<target-tree result>` |
| `UPSTREAM_CONFIG_BUILD_W1` | `PARTIAL` until run | `<target-tree result>` |
| `UPSTREAM_CONFIG_SPARSE_C1` | `PARTIAL` until run | `<target-tree result>` |
| `UPSTREAM_CONFIG_MODULE_PACKAGE` | `PARTIAL` until run | `<target-tree result>` |
| `UPSTREAM_CONFIG_UBLK_CAPABILITY_NO_PRODUCT` | `REFUSED` unless approved | `<isolated capability result>` |
| `UPSTREAM_CONFIG_WRITEBACK_CAPABILITY` | `REFUSED` unless approved | `<architecture result>` |
| `UPSTREAM_CONFIG_PUBLIC_REDACTION` | `UNVERIFIED` | `<manual review>` |
| `NO_EXTERNAL_KERNEL_PR_REFUSAL` | `PASS` for this local packet | `No external action performed` |
| `N3_SCOPE_REFUSAL` | `PASS` for this local packet | `N3 is separate` |

The result is `READY_FOR_HUMAN_REVIEW` only when source/path/config evidence
passes and all unavailable environment rows are explicitly accepted as
`PARTIAL`; it is never an adoption or product-ready result.

## 4. #41054 configuration packet — do not post automatically

```markdown
Title: Request WSL x86 kernel configuration support for ublk and zram writeback

This is a narrowly scoped configuration request for the x86 WSL kernel. At
the reviewed public target `<branch-or-tag>` / `<full-commit-id>`, the
canonical source is `arch/x86/configs/config-wsl`.

Requested x86 values:

CONFIG_BLK_DEV_UBLK=m
CONFIG_ZRAM_WRITEBACK=y

The arm64 configuration was checked independently at
`arch/arm64/configs/config-wsl-arm64`: ublk remains a candidate and
CONFIG_ZRAM_WRITEBACK=y is already enabled, so no redundant arm64 writeback
request is made.

This is configuration evidence only. It does not request a driver, UAPI,
native VRAM memory, NBD/ublk product policy, or a community pull request to
the WSL2-Linux-Kernel repository. RamShared continues to use its NBD product
path independently of Microsoft adoption.

Sanitized evidence: `<named test IDs and statuses>`.
```

If Microsoft requests patch-ready material, a human may prepare one logical
config commit per x86 symbol in the exact target tree. That is a separate,
explicit decision; this checklist never creates it.

## 5. Capability boundary — do not promote

- [ ] `UPSTREAM_CONFIG_UBLK_CAPABILITY_NO_PRODUCT`: a module/control node, if
      approved and observed, is capability evidence only.
- [ ] `UPSTREAM_CONFIG_WRITEBACK_CAPABILITY`: writeback behavior is recorded
      per architecture without a pressure workload.
- [ ] `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL`: no capability result changes
      RamShared’s NBD-only product decision.
- [ ] `N3_SCOPE_REFUSAL`: no config result creates host-authoritative VRAM,
      guest PFNs, NUMA/HMM ownership, or an N3 interface.

The accepted terminal language is `CAPABILITY_ONLY`, `PARTIAL`, or `REFUSED`.
Do not use `READY`, `ADOPTED`, or `DONE` for a symbol observation.

## 6. Issue drafts and milestone mapping

These are local English drafts and planning mappings. They do not edit or close
any external issue or milestone. The proposed milestone names are internal
labels until a separately approved owner assigns a real public milestone.

| Tracker item | Required state | Scope owner | Proposed mapping | Boundary |
| --- | --- | --- | --- | --- |
| `microsoft/WSL#41054` | `HUMAN_REVIEW_ONLY` | WSL feature/config request | `M-UPSTREAM-CONFIG` | Config-only; no community kernel PR. |
| `emersonbusson/ramshared#145` | `KEEP_OPEN_PARTIAL` | Post-NBD ublk research/retirement evidence | `M-UBLK-POST-NBD` | Must not own or block NBD readiness. |
| `emersonbusson/ramshared#156` | `KEEP_OPEN_PARTIAL` | Windows production signing + packaged supervised broker | `M-WINDOWS-PRODUCT-GATES` | External release gates; not NBD or N3. |
| New RamShared issue, number unassigned | `DRAFT_ONLY` | WSL2 NBD product readiness | `M-NBD-READINESS` | Create only if the NBD owner explicitly requests a new issue. |
| New RamShared issue, number unassigned | `DRAFT_ONLY` | Microsoft-native N3 host contract | `M-N3-HOST-CONTRACT` | Host-authoritative RFC; not #41054 or #145. |

### Draft for the future NBD issue — do not create automatically

```markdown
Title: WSL2 NBD-only product readiness and immutable release gate

Define the supported WSL2 product transport as NBD-only. Before a READY claim,
require a sealed `/opt/ramshared/releases/<version>` artifact, exact live
BINARY_MATCH, a Relay `--check` pass, swapoff-first lifecycle, and usable
lower-tier capacity of at least
V + max(ceil(10% of V), 512 MiB) for the configured VRAM tier V.

Qualify 1 GiB first, then 2 GiB and 4 GiB only after the preceding cell passes
with n>=3 integrity/performance runs. Retire legacy ublk product service
ownership without unloading `ublk_drv`. No reboot or WSL shutdown is part of
the gate. Product-off, ready, and blocked states must remain distinct.

Current status: DRAFT_ONLY; no external issue created by this packet.
```

### Draft for the N3 RFC issue — do not create automatically

```markdown
Title: Define a host-authoritative N3 contract before native VRAM tier work

Prepare a public, versioned RFC that makes Windows host/WDDM/VIDMM authority
explicit for VRAM budget, residency, pressure, reset/TDR, migration, revoke,
and offline semantics. The guest implementation is a pure Rust state model:
it accepts bounded, fresh, epoch-tagged observations and emits advisory intent;
it never invents PFNs, guest NUMA ownership, or host residency.

Until that host contract and independent evidence exist, the result is
REFUSED_HOST_CONTRACT and the WSL2 NBD product remains independent.

Current status: DRAFT_ONLY; no external issue or RFC submitted by this packet.
```

### #145 status note — keep open partial

`#145` remains `KEEP_OPEN_PARTIAL` for post-NBD ublk research/retirement
evidence. It is not the owner of the NBD-only product, must not block the NBD
readiness pack, and must not be closed by a config-only or NBD result. Close it
only after its own ublk lifecycle contract and evidence satisfy its owner.

### #156 status note — keep open partial

`#156` remains `KEEP_OPEN_PARTIAL` for the two documented Windows product
gates: production/Microsoft signing and a packaged supervised broker suitable
for autonomous SCM daily use. It is independent of #41054, NBD readiness, and
the N3 host contract. Existing physical evidence does not close those gates.

## 7. Final handoff

- [ ] `UPSTREAM_TEMPLATE_SCOPE_SEPARATION` passes: #41054, #145, #156, NBD,
      and N3 have distinct owners and claims.
- [ ] #145 and #156 are explicitly `KEEP_OPEN_PARTIAL`.
- [ ] The NBD future issue is draft-only and has no number or external write.
- [ ] The N3 future issue is draft-only and states `REFUSED_HOST_CONTRACT`.
- [ ] No milestone mutation was performed.
- [ ] All target-sensitive rows have a current full-SHA revalidation or
      `NEEDS_REVALIDATION`.
- [ ] A human chooses `HOLD`, `READY_FOR_HUMAN_REVIEW`, or `REJECTED`.

The checklist never sends material itself. A future external action requires a
new explicit decision and must not be inferred from a green local check.
