# Public-safe contribution checklist — WSL upstream-native strategy

> **Status:** preparation only. This checklist prepares material for human
> review. It must not be posted, sent, or used to create an external issue,
> comment, pull request, patch, email, branch, or tag without a new explicit
> decision.

## 1. Safety envelope

- [ ] Confirm the packet is limited to one lane: **A — configuration**, **B —
  dxgkrnl FORTIFY**, or **C — native NUMA refusal**.
- [ ] Confirm no packet combines #41054 with Lane B.
- [ ] Confirm no packet asks for a guest-native VRAM-as-NUMA/HMM change.
- [ ] Confirm no claim says that a FORTIFY warning caused or fixed a freeze,
  reset, TDR, display failure, or other runtime incident.
- [ ] Confirm all source facts link only to official Microsoft or Linux primary
  sources.
- [ ] Remove private paths, usernames, machine or account identifiers, distro
  names, IP addresses, tokens, dump files, raw host logs, screenshots, and
  unredacted diagnostic output.
- [ ] Replace any local result with a sanitized test ID, public target identity,
  command class, result, and observable stop/rollback state.
- [ ] Confirm no test result is described as `DONE` when its required
  environment was unavailable; use `PARTIAL`, `REFUSED`, or
  `NEEDS_REVALIDATION` instead.

Passing this section is `UPSTREAM_PUBLIC_EVIDENCE_REDACTION`.

## 2. Common evidence sheet

Use this table in a local human-review packet only. Do not fill it with
environment identifiers that are not public.

| Field | Required value |
| --- | --- |
| Lane | `CONFIG`, `DXG_FORTIFY`, or `NATIVE_NUMA` |
| Target | Public repository, branch or tag, and full commit ID |
| Date | UTC date of the source revalidation |
| Base configuration | Architecture and public config file name |
| Tests | Named SPEC test IDs and `PASS` / `PARTIAL` / `REFUSED` / `NEEDS_REVALIDATION` |
| Commands | Sanitized command class and exit/result, without local path or host data |
| Stop state | `NONE`, `TDR_STOP`, `BOOT_ROLLBACK`, or source/layout refusal |
| Recovery | Sanitized confirmation of known-good selection or campaign stop |
| Reviewer decision | `HOLD`, `READY_FOR_HUMAN_REVIEW`, or `REJECTED` |

Revalidation is required when the target branch, tag, release, Kconfig
dependencies, dxgkrnl layout, FORTIFY state, build result, or thirty-day review
window changes. This is `UPSTREAM_BRANCH_REVALIDATION`.

## 3. Lane A — #41054 configuration-adoption packet

### Required evidence before human review

- [ ] `UPSTREAM_CONFIG_X86_DELTA_EXACT`: the x86 request is exactly:

  ```text
  CONFIG_BLK_DEV_UBLK=m
  CONFIG_ZRAM_WRITEBACK=y
  ```

- [ ] `UPSTREAM_CONFIG_ARM64_DELTA_REVALIDATION`: arm64 is checked
  independently; do not request redundant zram writeback if it is already
  enabled.
- [ ] `UPSTREAM_CONFIG_OLDDEFCONFIG`, `UPSTREAM_CONFIG_BUILD_W1`,
  `UPSTREAM_CONFIG_SPARSE_C1`, and `UPSTREAM_CONFIG_CHECKPATCH` have the
  recorded status.
- [ ] `UPSTREAM_CONFIG_MODULES_PACKAGE`, `UPSTREAM_CONFIG_UBLK_MODULE_LOAD`,
  and `UPSTREAM_CONFIG_ZRAM_WRITEBACK_CAPABILITY` have the recorded status, or
  are clearly `PARTIAL` because live approval was absent.
- [ ] `UPSTREAM_CONFIG_BOOT_1`, `UPSTREAM_CONFIG_BOOT_2`, and
  `UPSTREAM_CONFIG_BOOT_3` are recorded only after explicit approval on an
  isolated surface.
- [ ] `UPSTREAM_SAFETY_COUNTER_BASELINE` records a `0` increment for reset/TDR,
  guest BUG/Oops, and unexpected dxg errors for every approved boot.
- [ ] `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL` confirms that the symbols do not
  make RamShared ublk transport ready; existing custom-kernel and NBD paths
  remain unchanged.
- [ ] No TDR/reset, display instability, guest BUG/Oops, dxg error, boot
  failure, or rollback failure was encountered. If one occurred, stop rather
  than preparing a packet.

### Prepared status update — do not post automatically

````markdown
Title: Request WSL x86 kernel configuration support for ublk and zram writeback

This is a narrowly scoped configuration request for the x86 WSL kernel:

```text
CONFIG_BLK_DEV_UBLK=m
CONFIG_ZRAM_WRITEBACK=y
```

The reviewed public target is `<branch-or-tag>` at `<full-commit-id>`.
The corresponding arm64 configuration was checked independently; its result is
`<sanitized-arm64-state>`.

This request does not propose a WSL feature, a kernel-driver change, a GPU
memory change, or a product transport policy change. The two symbols are
requested independently of any product adoption decision.

Sanitized evidence: `<named-config-and-build-test-IDs with status>`.
````

If Microsoft asks for commits, prepare at most one logical configuration commit
per symbol and use the exact target tree. Example subjects, subject to target
tree policy:

```text
config: enable CONFIG_BLK_DEV_UBLK
config: enable CONFIG_ZRAM_WRITEBACK
```

Do not invent a `Signed-off-by`, a maintainer, a mailing list, a target branch,
or an internal Microsoft route.

## 4. Lane B — separate dxgkrnl FORTIFY release-adoption packet

### Required triage before human review

- [ ] `DXG_FORTIFY_RELEASE_ANCESTRY`: evaluate public target ancestry for
  `1dda0bd6b031fec40afb3c6c9d59b8b89fd9e8db`.
- [ ] If the target contains that commit, record regression verification only;
  do not prepare a duplicate report, patch, or request.
- [ ] If it lacks the commit, `DXG_FORTIFY_SOURCE_LAYOUT` and
  `DXG_FORTIFY_WIRE_LAYOUT` verify the exact target header, `cmd_size`,
  allocation, and both variable copies.
- [ ] `DXG_FORTIFY_REPRO_OBJECT_COUNT_1` covers the observed one-object case;
  `DXG_FORTIFY_REPRO_OBJECT_COUNT_MULTI` covers a valid multi-object boundary
  for the first copy. `NOT_REPRODUCED` stops the report/patch path.
- [ ] Any candidate is the exact one-line flexible-array change or an
  equivalently verified backport. Refactors, UAPI changes, and replacement
  designs are out of scope.
- [ ] `DXG_FORTIFY_CHECKPATCH`, `DXG_FORTIFY_BUILD_W1`,
  `DXG_FORTIFY_SPARSE_C1`, named regression checks, and three approved manual
  boots have recorded status before any release-adoption decision.
- [ ] `UPSTREAM_SAFETY_COUNTER_BASELINE` records a `0` increment for reset/TDR,
  guest BUG/Oops, and unexpected dxg errors for every approved boot.
- [ ] Run `get_maintainer.pl` against the verified exact source only if a
  human-approved contribution route still requires it. Do not prefill
  recipients.
- [ ] `DXG_FORTIFY_CAUSALITY_REFUSAL` is recorded: this is a scoped FORTIFY
  diagnostic and release-adoption question, not a root-cause claim.

### Prepared separate report — do not post automatically

```markdown
Title: Verify release adoption of dxgkrnl flexible-array FORTIFY fix

This report is separate from #41054 and concerns only the public target
`<branch-or-tag>` at `<full-commit-id>`.

Target ancestry for
`1dda0bd6b031fec40afb3c6c9d59b8b89fd9e8db` is `<contains-or-does-not-contain>`.

On an explicitly approved isolated test surface, the scoped diagnostic in
`dxgvmb_send_wait_sync_object_gpu` was `<reproduced-or-not-reproduced>` for
`<object-count-case>`. Source review confirmed that the wire layout is
`fences[object_count]` followed by `handles[object_count]`; both
variable-length copy bounds and the command allocation were reviewed.

If target ancestry lacks the known minimal fix and the exact source/reproducer
matches, request review of the already identified flexible-array change or an
equivalently verified backport. This report does not claim that the diagnostic
caused a freeze, GPU reset, TDR, or display problem.

Sanitized evidence: `<named-source-build-regression-test-IDs with status>`.
```

### Maintainer discovery record — do not send automatically

```text
Source revision: <full-public-commit-id>
Files: drivers/hv/dxgkrnl/dxgvmbus.c, drivers/hv/dxgkrnl/dxgvmbus.h
Command class: scripts/get_maintainer.pl against the verified source
Result: <sanitized-human-reviewed-recipient-output or NOT_RUN>
```

Names or addresses discovered by this command stay out of this repository until
the human-approved submission workflow specifically requires them.

## 5. Lane C — native VRAM-as-NUMA refusal response

### Prepared refusal — do not post automatically

```markdown
This scope does not propose guest-native VRAM-as-NUMA, HMM,
MEMORY_DEVICE_PRIVATE, synthetic PFNs, or guest-side add_memory(). WSL GPU-PV
does not by itself establish the required guest-owned device-memory contract.

Before reconsideration, the host owner must provide a documented WDDM/GPU-PV
ownership and capacity contract, guest-visible cooperative-driver-owned memory,
host/guest migration-reclaim-reset-TDR-offline semantics, and an isolated
hardware validation surface. Until then, this is
REFUSED_NATIVE_NUMA_CONTRACT.
```

Passing this section is `NATIVE_NUMA_CONTRACT_REFUSAL`.

## 6. Final human review

- [ ] `UPSTREAM_TEMPLATE_SCOPE_SEPARATION` passes: Lane A stays on #41054,
  Lane B remains a separate release-adoption path, and Lane C remains a
  refusal.
- [ ] All material is English and public-safe.
- [ ] No external communication or host-mutating action is queued or implied.
- [ ] No branch/release claim remains stale.
- [ ] If any live test recorded a safety stop, the packet is rejected until a
  separate approved recovery/diagnostic plan exists.

The final state is always a human decision: `HOLD`, `READY_FOR_HUMAN_REVIEW`,
or `REJECTED`. This checklist never sends material itself.
