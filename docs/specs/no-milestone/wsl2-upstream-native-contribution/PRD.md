---
slug: wsl2-upstream-native-contribution
title: "WSL #41054 config-only contribution boundary"
milestone: Microsoft-native N3 — Design
issues:
  - "microsoft/WSL#41054"
  - 197
---

# PRD — WSL #41054 config-only contribution boundary

## Summary

This pack describes one narrow request to Microsoft: evaluate two x86 WSL
kernel configuration symbols through the existing
[`microsoft/WSL#41054`](https://github.com/microsoft/WSL/issues/41054)
feature request. It is configuration evidence plus a patch-ready, config-only
series. It is not kernel code, an unsolicited external kernel pull request, a
RamShared product gate, or a native VRAM proposal.

The source identity for this revision is the exact public commit
[`14794180686c2fb6307fbe359c359bec765249f3`](https://github.com/microsoft/WSL2-Linux-Kernel/commit/14794180686c2fb6307fbe359c359bec765249f3)
in `microsoft/WSL2-Linux-Kernel`. The canonical configuration paths are:

```text
arch/x86/configs/config-wsl
arch/arm64/configs/config-wsl-arm64
```

At that pinned source:

| Architecture | Canonical file | `CONFIG_BLK_DEV_UBLK` | `CONFIG_ZRAM_WRITEBACK` | Decision |
| --- | --- | --- | --- | --- |
| x86 | `arch/x86/configs/config-wsl` | not set | not set | Request both symbols as config-only evidence for #41054. |
| arm64 | `arch/arm64/configs/config-wsl-arm64` | not set | `y` | ublk is a separate candidate; writeback is already on and must not be requested redundantly. |

The official build entry point uses `KCONFIG_CONFIG=Microsoft/config-wsl`
(`Microsoft/config-wsl-arm64` for arm64 where supported); the canonical source
files above are the architecture-specific facts that must be revalidated. A
path or branch change invalidates the result.

## Technical context

The [WSL2-Linux-Kernel README](https://github.com/microsoft/WSL2-Linux-Kernel/blob/14794180686c2fb6307fbe359c359bec765249f3/README.md)
directs WSL feature requests to the WSL project,
states that the kernel repository does not accept issue reports, and directs
kernel code contributions to the normal upstream Linux process. Therefore this
pack uses an issue-first route. It may publish a tested branch in the author's
fork and one evidence update to #41054, but it does not create a community PR
to `microsoft/WSL2-Linux-Kernel` unless a Microsoft maintainer explicitly asks
for that route.

The [x86 config](https://raw.githubusercontent.com/microsoft/WSL2-Linux-Kernel/14794180686c2fb6307fbe359c359bec765249f3/arch/x86/configs/config-wsl)
and [arm64 config](https://raw.githubusercontent.com/microsoft/WSL2-Linux-Kernel/14794180686c2fb6307fbe359c359bec765249f3/arch/arm64/configs/config-wsl-arm64)
are the pinned primary-source inputs. `CONFIG_BLK_DEV_UBLK=m` and `CONFIG_ZRAM_WRITEBACK=y` are capabilities, not a
RamShared product transport decision. NBD remains the supported WSL2 product
path. The separate [`wsl2-nbd-product-readiness`](../wsl2-nbd-product-readiness/PRD.md)
pack owns product readiness. The separate
[`microsoft-native-vram-memory-tier`](../microsoft-native-vram-memory-tier/PRD.md)
pack owns the host-authoritative N3 RFC; N3 is not part of #41054.

`olddefconfig` also exposes the upstream-default
`CONFIG_BLKDEV_UBLK_LEGACY_OPCODES=y` when ublk is enabled. That derived value
is not hidden from the evidence packet or misrepresented as a third requested
source delta. It remains an explicit compatibility/security review point for
Microsoft.

The current public contribution-route evidence is recorded in
[`RESEARCH.md`](RESEARCH.md). Its 2026-08-14 snapshot found
no human external pull-request merge after the repository's 2021 route
statement. The accepted external contribution pattern is maintainer-owned
internal application, as demonstrated by WSL2-Linux-Kernel #247. Therefore a
maintainer request, not local patch readiness alone, gates any future PR.

## Recommended option

Keep one public request, one exact source identity, and two architecture
records:

1. Revalidate the pinned SHA and both canonical files.
2. Prepare the x86 request exactly as:

   ```text
   CONFIG_BLK_DEV_UBLK=m
   CONFIG_ZRAM_WRITEBACK=y
   ```

3. Record arm64 independently: ublk remains a candidate, while
   `CONFIG_ZRAM_WRITEBACK=y` is already enabled at the pinned source.
4. Run the named source/build/package/capability gates in isolated external
   worktrees under the user's explicit approval; no host swap or pressure
   workload is needed for this config request.
5. Prepare an English human-review draft for #41054 with source, build, size,
   capability, and cleanup evidence.
6. Ask Microsoft whether it will integrate the two independently reviewable
   changes internally or explicitly wants the prepared patch series as a PR.

If Microsoft asks for patch-ready material, a human may prepare one logical
config commit per symbol in the exact target tree. That possible future action
does not authorize a community pull request in this repository or a product
dependency on adoption.

## Functional requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| RF-U-1 | Pin #41054 evidence to the exact SHA. | Full SHA and revalidation date are recorded. |
| RF-U-2 | Use the canonical x86 and arm64 config paths. | No generic defconfig or remembered path is accepted. |
| RF-U-3 | Request only the two x86 config deltas. | x86 request is exactly ublk=m and zram writeback=y. |
| RF-U-4 | Record arm64 independently. | ublk is candidate; existing zram writeback is not requested redundantly. |
| RF-U-5 | Keep the route config-only. | No driver, UAPI, product, or native-memory request appears in the packet. |
| RF-U-6 | Refuse an unsolicited external kernel PR. | A tested fork branch and issue evidence may be published, but no Microsoft-repository PR opens without an explicit maintainer request. |
| RF-U-7 | Preserve product independence. | NBD remains the WSL2 product path while #41054 is pending/rejected. |
| RF-U-8 | Keep N3 separate. | N3 RFC links only to its own pack and is not a #41054 acceptance gate. |
| RF-U-9 | Revalidate branch/tag/config drift. | Any target change returns status to `NEEDS_REVALIDATION`. |
| RF-U-10 | Keep evidence public-safe and English. | No private host data, credentials, raw logs, or unreviewed text. |
| RF-U-11 | Gate a pull request on an explicit maintainer request. | Patch readiness alone cannot open a known unsupported route. |
| RF-U-12 | Disclose every derived Kconfig value. | Generated x86/arm64 evidence records the ublk legacy-opcode default instead of describing only the two source lines. |

## Non-functional requirements

| ID | Requirement |
| --- | --- |
| NFR-U-1 | External writes are limited to the reviewed fork branch, RamShared evidence PR, and one #41054 update; no host reboot, WSL shutdown, swap, or pressure is performed. |
| NFR-U-2 | Source facts use official Microsoft/Linux primary sources only. |
| NFR-U-3 | Missing live target evidence is `PARTIAL`, `REFUSED`, or `NEEDS_REVALIDATION`, never `DONE`. |
| NFR-U-4 | Build and capability evidence records command class, target identity, and sanitized result. |
| NFR-U-5 | The config request never changes RamShared transport or release policy. |
| NFR-U-6 | A 30-day review or any source/Kconfig/dependency change invalidates the packet. |

## Flows

### Source revalidation

1. Record repository, branch/tag, and full target revision.
2. Verify that the target resolves to the exact requested SHA, or mark
   `NEEDS_REVALIDATION`.
3. Read `arch/x86/configs/config-wsl` and
   `arch/arm64/configs/config-wsl-arm64` from that revision.
4. Record the two x86 current values and arm64 current values independently.
5. Stop on a path, architecture, branch, or value mismatch.

### Human-review packet

1. Fill the common evidence sheet in
   [`CONTRIBUTION-CHECKLIST.md`](CONTRIBUTION-CHECKLIST.md).
2. Include the exact x86 delta and the arm64 non-duplicate observation.
3. Mark every test `PASS`, `PARTIAL`, `REFUSED`, or `NEEDS_REVALIDATION`.
4. Publish one reviewed evidence update to #41054 after all mandatory local
   gates pass; do not open a Microsoft-repository PR.

### Capability boundary

If a custom kernel proves that ublk or writeback loads, record capability only.
Do not call ublk product-ready, do not replace the NBD path, and do not infer
N3 host ownership. Return `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL` for any such
attempt.

## Data/state model

| Field | Values | Meaning |
| --- | --- | --- |
| `target_repo` | public repository | Exact source owner. |
| `target_revision` | full SHA | Must equal `14794180686c2fb6307fbe359c359bec765249f3` for this revision. |
| `architecture` | `x86`, `arm64` | Independent evidence row. |
| `config_path` | canonical architecture path | Source file read from target revision. |
| `symbol_state` | `not_set`, `m`, `y`, `unknown` | Exact Kconfig value. |
| `requested_delta` | x86 pair or none | Only x86 has the two requested changes. |
| `status` | `UNVERIFIED`, `PASS`, `PARTIAL`, `REFUSED`, `NEEDS_REVALIDATION` | Evidence status, not product status. |
| `external_action` | `NONE`, `FORK_BRANCH`, `ISSUE_UPDATE`, `MAINTAINER_REQUESTED_PR` | Direct Microsoft PR remains maintainer-gated. |

`ADOPTED` is deliberately not a local status: Microsoft’s released source
and product decision own adoption. A rolling source result is not installed
release evidence.

## Interfaces

| Interface | Contract |
| --- | --- |
| `microsoft/WSL#41054` | One feature request; human-mediated review only. |
| `arch/x86/configs/config-wsl` | Canonical x86 source at the pinned revision. |
| `arch/arm64/configs/config-wsl-arm64` | Canonical arm64 source and independent revalidation. |
| `Microsoft/config-wsl` | Official build invocation/config entry point; resolve against target tree. |
| WSL2-Linux-Kernel README | Route authority: WSL feature request, normal upstream code process. |
| RamShared NBD pack | Product owner; unaffected by adoption. |
| N3 pack | Host-authoritative RFC owner; separate issue and gate. |

## Dependencies/risks

| Risk | Mitigation and rollback trigger |
| --- | --- |
| SHA or branch changes | Stop and return `NEEDS_REVALIDATION`; do not copy old values. |
| Canonical path differs | Stop; verify the official target tree rather than guessing. |
| arm64 writeback request duplicates existing `y` | Remove the redundant request; retain independent ublk candidate note. |
| Capability becomes product claim | Refusal test preserves NBD and custom-kernel separation. |
| Unsolicited community PR is proposed | Refuse; publish evidence through the issue-first route instead. |
| A patch is ready but no maintainer requested a PR | Post only the evidence update to #41054 and retain the patch locally. |
| N3 is bundled into #41054 | Refuse and link the separate host-authoritative N3 pack. |
| Missing target build/boot evidence | `PARTIAL`; never infer adoption or product readiness. |

Rollback trigger: any public text or action claims Microsoft adoption, changes
the target tree, opens an external PR, routes RamShared through ublk, or treats
N3 as a config consequence. Remove the claim from the packet and return to the
last source-pinned status.

## Implementation strategy

This task executes the named tests in the SPEC, records an `IMPL.md`, publishes
the tested config-only series in the author's fork, and posts one reviewed
evidence update to #41054. It must not open a Microsoft-repository pull request
without a maintainer request. N3 and NBD remain separate workstreams.

## Documents

| Document | Action |
| --- | --- |
| `PRD.md` | Revise the config-only decision and source pin. |
| `SPEC.md` | Revise the executable matrix and boundary rules. |
| `CONTRIBUTION-CHECKLIST.md` | Revise English packet, issue drafts, and milestone mapping. |
| `RESEARCH.md` | Record current accepted/closed PR evidence and route decision. |
| `wsl2-nbd-product-readiness/` | Separate NBD product owner; no issue ownership is imported. |
| `microsoft-native-vram-memory-tier/` | Separate N3 host-authority owner. |
| `IMPL.md` | Record exact source, patch, build, package, capability, and route evidence. |
| `validation.md`, release docs | Not changed here. |
| `docs/INDEX.md` | Regenerate as a generated index after the complete pack update. |

## Out of scope

- An unsolicited Microsoft-repository PR or any claim of Microsoft adoption;
- dxgkrnl/FORTIFY work, kernel code, driver/UAPI changes, or maintainer guesses;
- ublk product transport, NBD product readiness, or custom-kernel promotion;
- host-authoritative N3 implementation or guest-native VRAM memory;
- host reboot, WSL shutdown, swap, pressure, GPU workload, or host kernel boot;
- edits to release documentation, product code, CI/workflows, or validation
  records.

## Acceptance

| ID | Criterion |
| --- | --- |
| A-U-1 | Exact SHA and canonical x86/arm64 paths are recorded. |
| A-U-2 | x86 request is exactly `CONFIG_BLK_DEV_UBLK=m` + `CONFIG_ZRAM_WRITEBACK=y`. |
| A-U-3 | arm64 records ublk as candidate and writeback already `y`, without a duplicate request. |
| A-U-4 | #41054 packet is config-only and no unsolicited external kernel PR is proposed. |
| A-U-5 | NBD product and N3 RFC remain separate owners/gates. |
| A-U-6 | Issue drafts/milestone mapping retain #145/#156 as `KEEP_OPEN_PARTIAL`. |
| A-U-7 | Named source/build/capability/refusal tests and rollback are present. |
| A-U-8 | External actions are limited to a tested fork branch, RamShared evidence PR, and one #41054 update; no live host action is claimed. |
| A-U-9 | A direct PR remains gated on an explicit Microsoft maintainer request; internal application counts as contribution success. |

## Validation

Validation combines repository index/check/hygiene/diff with the exact
source/build/package/Sparse/QEMU gates in `SPEC.md`. Missing or failed evidence
remains `PARTIAL` or `REFUSED`. A green config build or isolated capability
boot cannot close NBD readiness or N3 host ownership.
