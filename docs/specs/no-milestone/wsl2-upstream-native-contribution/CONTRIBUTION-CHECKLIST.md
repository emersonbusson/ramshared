# Public-safe contribution checklist — WSL #41054 config-only request

> Issue-first contribution packet. A reviewed fork branch and one evidence
> update to #41054 are allowed after all mandatory gates pass. An unsolicited
> pull request to the Microsoft kernel repository remains forbidden. All
> material is English and public-safe.

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
- [ ] Confirm no unsolicited PR to `microsoft/WSL2-Linux-Kernel` is proposed.
- [ ] Confirm [`RESEARCH.md`](RESEARCH.md) remains current
      and no maintainer has explicitly requested a PR.
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

Generated `olddefconfig` evidence must also record
`CONFIG_BLKDEV_UBLK_LEGACY_OPCODES=y`, the upstream default selected by
enabling ublk. It is a disclosed compatibility/security review point, not a
third source-line request.

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
| `NO_EXTERNAL_KERNEL_PR_REFUSAL` | `PASS` when route is respected | `No unsolicited Microsoft PR` |
| `N3_SCOPE_REFUSAL` | `PASS` for this local packet | `N3 is separate` |
| `UPSTREAM_PR_ROUTE_AUDIT` | `PASS` at 2026-08-14 snapshot | `RESEARCH.md` |
| `MAINTAINER_REQUESTED_PR_GATE` | `REFUSED` until requested | `<maintainer request URL or NONE>` |

The result is `READY_FOR_HUMAN_REVIEW` only when source/path/config evidence
passes and all unavailable environment rows are explicitly accepted as
`PARTIAL`; it is never an adoption or product-ready result.

## 4. #41054 configuration packet — post once after mandatory gates

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

Would the WSL kernel team consider these two independently reviewable config
changes for the stock kernel? If so, should they be integrated internally, or
would you like the prepared two-commit patch series submitted as a pull
request against `linux-msft-wsl-6.18.y`?
```

The approved campaign prepares one logical config commit per independently
reviewable decision and may publish them in the author's fork. A PR from that
branch to Microsoft remains a separate decision that requires an explicit
maintainer request.

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

## 6. Issue and milestone mapping

These local records reconcile existing open trackers with the milestone labels
recorded in the corresponding PRD frontmatter. They do not edit or close an
issue or milestone. The approved campaign may publish the tested fork branch
and one evidence comment, but not a tag or unsolicited pull request. The actual
upstream discussion remains `microsoft/WSL#41054`.

| Tracker item | Required state | Scope owner | Current milestone | Boundary |
| --- | --- | --- | --- | --- |
| `emersonbusson/ramshared#194` | `OPEN_PARTIAL` | WSL2 NBD product readiness | `v0.9.0-beta.1 — WSL2 NBD` | Tracked readiness work; no live completion or `READY` claim. |
| `emersonbusson/ramshared#196` | `OPEN_PARTIAL` | Microsoft-native N3 public host contract | `Microsoft-native N3 — Design` | Host-authoritative RFC; no adoption, live host, or product claim. |
| `emersonbusson/ramshared#197` | `OPEN_PARTIAL` | WSL upstream config tracker | `Microsoft-native N3 — Design` | Local tracker only; upstream discussion remains `microsoft/WSL#41054`. |
| `microsoft/WSL#41054` | `ISSUE_UPDATE_READY` | WSL feature/config request | External issue; no local milestone | Config-only; no unsolicited community kernel PR. |
| `emersonbusson/ramshared#145` | `KEEP_OPEN_PARTIAL` | Post-NBD ublk research/retirement evidence | `M-UBLK-POST-NBD` | Must not own or block NBD readiness. |
| `emersonbusson/ramshared#156` | `KEEP_OPEN_PARTIAL` | Windows production signing + packaged supervised broker | `M-WINDOWS-PRODUCT-GATES` | External release gates; not NBD or N3. |

### #194 tracking note — WSL2 NBD readiness

`#194` is the current local tracker for the NBD readiness pack and is mapped
to `v0.9.0-beta.1 — WSL2 NBD`. The product transport remains NBD-only, with
sealed-release, exact `BINARY_MATCH`, Relay, swapoff-first, lower-tier
capacity, and bounded GiB evidence gates. The pack remains `PARTIAL` until
those same-surface live gates are measured; this mapping does not claim
completion, adoption, or a host action.

### #196 tracking note — Microsoft-native N3

`#196` is the current local tracker for the public host-contract RFC and is
mapped to `Microsoft-native N3 — Design`. The contract remains
`REFUSED_HOST_CONTRACT` until Microsoft-owned semantics and independent
evidence exist. The guest model remains pure and advisory: it cannot invent
PFNs, guest NUMA ownership, or host residency. This mapping does not claim
native VRAM adoption or live host validation.

### #197 tracking note — upstream configuration lane

`#197` is the current local tracker for the upstream configuration lane and is
mapped to `Microsoft-native N3 — Design`. It tracks the narrow x86/arm64
configuration evidence packet only. The actual upstream discussion is
`microsoft/WSL#41054`; one reviewed evidence update and the tested fork branch
are allowed, while an external pull request remains maintainer-gated. Missing
build, boot, and capability evidence remains `PARTIAL`, `REFUSED`, or
`NEEDS_REVALIDATION`.

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

- [ ] `UPSTREAM_TEMPLATE_SCOPE_SEPARATION` passes: #41054, #145, #156, #194,
      #196, #197, NBD, and N3 have distinct owners and claims.
- [ ] #145 and #156 are explicitly `KEEP_OPEN_PARTIAL`.
- [ ] #194 is mapped to `v0.9.0-beta.1 — WSL2 NBD` without a live-completion claim.
- [ ] #196 is mapped to `Microsoft-native N3 — Design` and remains
      `REFUSED_HOST_CONTRACT` pending host-owned semantics and evidence.
- [ ] #197 is mapped to `Microsoft-native N3 — Design`; the actual upstream
      discussion remains `microsoft/WSL#41054`.
- [ ] No milestone mutation was performed.
- [ ] No unsolicited Microsoft pull request is proposed or submitted.
- [ ] All target-sensitive rows have a current full-SHA revalidation or
      `NEEDS_REVALIDATION`.
- [ ] A human chooses `HOLD`, `READY_FOR_HUMAN_REVIEW`, or `REJECTED`.

The checklist allows only the external actions authorized in the PRD: a tested
fork branch, a RamShared evidence PR, and one reviewed #41054 update. Any
Microsoft-repository PR still requires a new explicit maintainer request.
