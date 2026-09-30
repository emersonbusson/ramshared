# IMPL — Fragmentation-resilient VMBus rings across confidential guests

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/SPEC.md`

## Status

**PARTIAL — the current public v2 candidate has six tracked patches.** The
six-patch shape is the 2026-09-30 hygiene pass: the order-vector correction
is folded into the patch that introduces the KUnit case it repairs, the
`test(hv):` subjects are retitled, and every mail is regenerated with
`git format-patch`. The applied tree is byte-identical to the previous
seven-patch candidate, so the source-level findings below still describe it.
**Hosted run
[36763981097](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36763981097)
on commit `7e4ccc98d32f` qualifies the pinned six-patch `series/SHA256SUMS`:
all three jobs green** — `wsl-backport` (15/15 KUnit), `kernel (x86_64)`
(every stage applied and built, strict checkpatch, both CoCo gate self-tests,
20/20 named KUnit cases), and `kernel (arm64)` (build + Sparse). Runs
36574925363 and 36590352003 qualify the predecessor bytes and must not be
cited for this candidate.

A kernel built from this worktree's `vmbus-ring-buffer-upstream-v2` branch
head `a5cedb4de6f8` is installed on the daily WSL host as Build #9
(`kernel-ramshared-v6`, receipt-matched source, EVD-0117/0118); that is the
WSL-derived candidate, not a boot of the six-patch mainline series against
v7.3-rc4. Live GPADL/UIO lifecycle, forced order-zero fallback, and
SEV-SNP/TDX/Arm CCA evidence remain open.

EVD-0111 reviewed a dirty candidate at `a022ac393ecaab845682f5afe2be6be792aedde2` and found that its attempted cleanup helper confused host rescind with local unload and missed partial GPADL establishment. That audit's statement that no candidate build or KUnit had run described that dirty snapshot at that time. The current public series ends at `0006-gpadl-lifetime-reclaim.patch`; EVD-0112 records the hosted build and KUnit results for its predecessor seven-patch bytes. Those tests do not establish live VMBus protocol behavior or CoCo safety.

EVD-0088 and EVD-0089 record cumulative read-only growth in
`vmbus_alloc_buffer` vmalloc entries: 14,003 at 12:21, 15,821 at 12:38, and
17,690 at 12:55. The summed vmalloc area grew by 761.8 MiB from 12:38 to
12:55; this includes virtual guard space and is not a Windows resident-RAM
measurement. At 12:55, 17,350 mappings reported 104 backing pages each,
against 102 registered VMBus channels. This is a strong buffer-retention
candidate. The `50715f5f7` source snapshot has one in-tree allocator caller:
`vmbus_alloc_ring()` creates one allocation for both ring halves, and its
configured RELID limit is 2,048. If Build #6 came from that snapshot, 17,350
104-page maps cannot be explained by simultaneously open in-tree rings. A
separate later backport (`418653fde`) also converts NetVSC and UIO allocations;
that branch must not be conflated with Build #6. Source review confirmed a
retention bug in both snapshots: the rescind path reports teardown success
without clearing the nonzero GPADL handle, then buffer release skips freeing
and clears the owner structure. This is a concrete source-level explanation
for cumulative retention, conditional on the running image containing this
code and reaching that path. The Build #6 source revision is not matched, and
the maps have not been correlated with channel lifecycle events. Guest
MemAvailable rose between the samples while swap use increased; the latest
Windows sample showed physical headroom rising and `VmmemWSL` working set
falling. EVD-0091 at 14:02 then counted 24,932 maps and 10,667,855,872 bytes
of vmalloc area: +7,242 maps and +2,950.7 MiB over 66:58, again about
44 MiB/min. In that interval guest `MemAvailable` fell by about 1.99 GiB and
`SwapFree` by about 849 MiB. A Windows sample 74 seconds later had 16,147 MiB
physical headroom and a 6,605 MiB `VmmemWSL` working set; compared with 13:36,
host headroom rose 1,810 MiB and working set fell 1,501 MiB. The evidence now
strongly supports gradual guest-side accumulation with growing swap use, not
continuous exhaustion of Windows physical RAM. It still does not prove that
the running Build #6 contains the audited rescind path or that this was the
freeze trigger.

The versioned six-patch draft is based on Linux `v7.3-rc4`
(`93f51579e7df248780214094418f205253383cc5`). The local draft at
`docs/upstream/patches/vmbus-ring-buffer-v2-draft.patch` remains a separate
working diff. The ordinary Hyper-V boot evidence below is for an earlier
four-commit snapshot; it is not a boot of this six-patch candidate. The
current public series is not a distribution backport or an upstream
submission.

## Implemented draft

| Path | Intended change |
| --- | --- |
| `include/linux/hyperv.h` | Aggregate ring buffer ownership and separate GPADL layout from decryption. |
| `drivers/hv/channel.c` | Allocate every ring with the accepted chunk allocator; preserve teardown errors and unsafe-to-free state; reclaim retained pages only after GPADL, page-state, and mapping-reference gates pass. |
| `drivers/hv/ring_buffer.c`, `drivers/hv/hyperv_vmbus.h` | Resolve each wraparound page from a virtual mapping. |
| `drivers/net/hyperv/hyperv_net.h`, `drivers/net/hyperv/netvsc.c` | Group netvsc allocation fields and retain memory after failed revoke/teardown. |
| `drivers/uio/uio_hv_generic.c` | Map noncontiguous ring pages (ring, control, receive, and send) through virtual UIO and sysfs paths with page protection matching shared/private backing. |

## Evidence so far

The bullets below retain earlier implementation milestones. EVD-0112 is the
current hosted build/KUnit result; it does not replace live Hyper-V or CoCo
qualification.

- A scratch structural contract test was RED on the unmodified source and
  GREEN (6/6) after the first draft edits. Two additional ownership regressions
  were RED on the draft and GREEN (8/8) after guarding GPADL teardown and UIO
  cleanup. These are **not** KUnit or runtime tests.
- `git diff --check` passed in the upstream worktree.
- Upstream `scripts/checkpatch.pl --no-tree --terse --strict` reported zero
  errors, zero warnings, and zero checks on the draft diff.
- `git apply --check --reverse` confirmed the saved patch matches the local
  upstream worktree.
- On Linux `v7.3-rc4`, `make O=<isolated-build-dir> -j4 W=1
  drivers/hv/channel.o drivers/hv/ring_buffer.o
  drivers/net/hyperv/netvsc.o` passed with no compiler diagnostics.
- The follow-up `make O=<isolated-build-dir> -j4 W=1 drivers/hv/
  drivers/net/hyperv/` also passed with no compiler diagnostics. `sparse` is
  not installed in this environment, so it was not run.
- After adding conservative ownership tracking for a partially posted GPADL,
  the same `W=1` directory build passed again with no compiler diagnostics;
  strict checkpatch still reported zero errors/warnings/checks.
- An explicit `uio_hv_generic.o` build was RED because the old UIO code still
  required `ringbuffer_page`. After conversion to virtual/page-array mapping,
  a combined `W=1` build of Hyper-V, netvsc, and UIO passed with no compiler
  diagnostics. This is still not a live mmap test.
- The current host runs WSL2 `6.18.40.1-microsoft-standard-WSL2+` and has
  neither a CCA nor a TDX guest. It cannot prove the maintainer's cross-CoCo
  objection is closed.
- A later source-only audit found that a failed GPADL teardown could still
  re-encrypt pages, and UIO could free buffers after an ambiguous post or
  teardown. The draft now records an unsafe-to-free flag in each GPADL,
  avoids re-encryption on teardown failure, and checks teardown errors in UIO.
  The 8 structural tests, `git diff --check`, and strict checkpatch pass after
  this change.
- After this audit, the four touched objects (`channel.o`, `ring_buffer.o`,
  `netvsc.o`, and `uio_hv_generic.o`) compiled with `W=1` against the isolated
  v7.3-rc4 build tree. The Hyper-V and netvsc directory builds also completed
  their `built-in.a` archives with `W=1` and no compiler diagnostics. These
  are compile checks, not a linked/booted kernel or fault-injection evidence.
- The booted WSL2 6.18.40.1 source tree was checked read-only. It still owns
  rings through `ringbuffer_page` and does not provide `vmbus_alloc_buffer()`.
  `git apply --check` of this v7.3-rc4 draft failed for all seven touched
  files. This is a confirmed API/backport boundary, not a patch to install
  directly on the host.

## Blocking gaps

1. Hosted run 36763981097 passed all six patch stages of the **current
   pinned bytes** on x86_64 and arm64, the WSL 6.18.40.1 backport build
   (15/15 KUnit), and x86_64 KUnit 20/20 named cases, with both CoCo gate
   self-tests green. Hosted run 36574925363 covered the predecessor
   seven-patch bytes and is not evidence for this candidate. The order-zero
   fallback test exercises injected allocation failures. Live allocator
   fragmentation and host response/rescind interleaving are runtime questions
   and are being closed by the Hyper-V drill pipeline (see **Next gate**).
   Order-zero fallback of the **chunked** allocator is not a runtime-drill
   question at all: `vmbus_alloc_buffer()` only takes that path when
   `vmbus_uses_shared_page_chunks()` is true (host-visible buffer and
   Hyper-V isolation or `CONFIG_ARM64`), and it allocates with
   `__GFP_NORETRY | __GFP_NOWARN`, so an order-7 failure prints nothing. Its
   runtime proof is the CoCo platform gate, the same as SEV-SNP/TDX/Arm CCA.
   Arm64 KUnit is skipped.
2. An earlier four-commit v7.3-rc4 snapshot was linked and booted on ordinary
   x86_64 Hyper-V. This does not qualify the current six-patch v2 series, the
   separate WSL backport, or a CoCo platform.
3. Normal GPADL create/teardown succeeded on that earlier snapshot. Current
   KUnit injects outgoing header/body/teardown post failures and tests
   response-state mapping. Live host response/error delivery and rescind
   interleavings remain untested; the first runtime drill (EVD-0133, run
   36777284600) could not exercise them because the UIO dynid was written
   brace-wrapped and `guid_parse()` rejected it — 30 cycles, 30 `bind_fail`,
   BUG-3 never reached. Fixed and re-running.
4. Ordinary Hyper-V UIO and sysfs ring mmap passed on the earlier snapshot.
   The current KUnit mapping-preparation tests do not exercise a real
   mmap-close/unregister lifecycle. CoCo memory-state tests for SEV-SNP, TDX,
   and Arm CCA, plus a matched performance run, remain absent.

## September 24 candidate update

The reviewable, versioned diff and contribution dossier are maintained in the
public kernel fork at
[`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/vmbus-ring-buffer-upstream-v2/Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2),
based on `93f51579e7df248780214094418f205253383cc5`. The local mainline
checkout contains four individually compiling commits; the versioned patches
and hosted workflow are maintained in the public kernel fork.

The candidate checks the rounded `u32` allocation size before rounding and
uses `cc_platform_has(CC_ATTR_GUEST_MEM_ENCRYPT)` alongside Hyper-V isolation
to avoid sending arm64 CCA shared pages through `vzalloc()`. UIO's receive and
send GPADL buffers now use `vmbus_alloc_buffer()` and aggregate teardown
ownership. A failed teardown metadata allocation marks the buffer unsafe to
free. These changes have not been built or tested on CCA, TDX, or SEV-SNP.

Local `git diff --check`, reverse `git apply --check`, and Linux
`checkpatch.pl --strict` passed. Hosted run 36040552037 passed the WSL
VMBus/NetVSC/UIO Sparse build, separate DXG compile, per-commit x86_64/arm64
builds, and all five named VMBus KUnit cases (nine KUnit cases passed in
total). Runs 36038457091 and 36039517554 exposed and led to fixes for the WSL
make target and DXG trace-only variables. The workflow pins the base and
records series SHA, configurations, and logs. KUnit runs on x86_64 only.
The versioned patch files remove an unsupported universal CoCo claim and
carry descriptions, matching authors, and `Signed-off-by` trailers on all four
commits. Hosted run 36046920733 passed against these exact files: WSL
VMBus/NetVSC/UIO Sparse, separate DXG compile, per-commit x86_64/arm64 compile
and Sparse, and all five named VMBus KUnit cases (nine total). The workflow
builds Sparse from a pinned revision and fails if it is unavailable or
silently disabled. CI does not cover GPADL stage fault injection, UIO mmap,
or live Hyper-V/CoCo behavior; those runtime gates are recorded below.

Run 36049418582 repeated these gates on public branch HEAD
`59e6fbfb8f47238b9347cad2060923260bb9f2ad`; all three jobs passed.

Run 36140064936 validated the four-patch series on the public fork: the WSL
backport with W=1 and Sparse, and x86_64/arm64 patch application, checkpatch,
Sparse, and builds passed. x86_64 KUnit ran all nine tests successfully; the
arm64 KUnit step was skipped. The first GPADL test-patch attempt targeted an
obsolete GPADL structure and was reverted before that run.

Run 36143196834 passed the corrected five-patch series on the public fork.
The WSL backport and x86_64/arm64 patch application, strict checkpatch, Sparse,
and W=1 builds passed. x86_64 KUnit passed all 13 tests; the
`hyperv-vmbus-buffer` suite passed all nine cases, including the four new
callback-injected GPADL tests;
arm64 KUnit remains skipped. The tests inject failures at the outgoing GPADL
header, each of two body posts, and teardown post, and check response-state
mapping. Live response/rescind interleaving and CoCo memory transitions remain
unverified.

## September 24, 2026 host smoke check

The WSL host was already booted from `C:\wsl\kernel-ramshared-v5` as
`6.18.40.1-microsoft-standard-WSL2+` Build #6. Its image SHA-256 was
`46dba8cc9e2b0d9789917b329d2cdf4aaf5dc30ee982b4dd0f7d783cd41e4cc8`, and
`vmbus_alloc_buffer` / `vmbus_free_buffer` appeared in the running kernel's
symbol table. The existing `/mnt/c/wsl/Validate-KernelBuild6.sh` returned 7
passes and one failure: `zram` was not loaded. Windows interop, a fresh
`wsl.exe --exec` session, absence of an order-7 allocation failure, and
absence of `accept4` failure passed. The host exposed 73 VMBus devices.

This is smoke evidence for the already-installed WSL allocator backport. It
does not identify the running image with the exact four-patch v7.3-rc4 series,
and the fallback was not forced. `wsl-kernel.sh status` reports `NEED_ARM`
because its immutable promotion receipt is missing. A read-only
`git apply --check` of the exact series against the fork's WSL 6.18.40.1
checkout failed in all seven touched source files. Do not claim this as an
installation or runtime test of the exact upstream series.

## WSL 6.18.40.1 backport draft

A separate public-fork branch,
[`vmbus-ring-buffer-wsl-backport-6.18.40.1`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/vmbus-ring-buffer-wsl-backport-6.18.40.1),
at commit `418653fde` ports the allocator safeguards onto the existing WSL
API. It adds checked `u32` page rounding, a fallback-order helper with order-0 KUnit
coverage, the `cc_platform_has(CC_ATTR_GUEST_MEM_ENCRYPT)` selection guard,
and a null guard before `vunmap()` during partial-allocation cleanup. It also
adds KUnit coverage for overflow, ownership refusal, and repeatable partial
cleanup. The original `418653fde` snapshot was not compiled or pushed when it
was first audited. The currently validated public-branch snapshot and hosted
results are recorded below; they must not be confused with the active WSL
kernel image.

An earlier backport snapshot was built with `W=1`, booted under generic QEMU,
and exercised with KUnit and module loading in isolated guests. This does not
establish that the current public snapshot at `b85e21326a41` was booted. The
running Build #6 image and `.wslconfig` remain unchanged because the promotion
receipt and module-to-VHDX provenance gate are unresolved.

The local WSL build has `CONFIG_KUNIT` unset, so its active kernel does not run
the new KUnit cases. A historical temporary x86_64 KUnit build against the
then-current backport source ran all five named `hyperv-vmbus-buffer-wsl`
cases: 5 passed, 0 failed. In another QEMU boot of the exact WSL image, `modprobe` loaded
`zsmalloc`, `zram`, and `ublk_drv`; `/dev/zram0` and `/dev/ublk-control` were
present. This closes the earlier initramfs packaging failure only. QEMU used
a generic virtual machine, so this is not a Hyper-V VMBus, WSL integration,
or CoCo memory-transition test. The live Build #6 smoke check still reports
7 passes and one failure because `zram` is not loaded in the host.

Sparse logs retain diagnostics in unchanged baseline source, including a
VMBus driver context-imbalance warning and a flexible-array warning in the
GPADL header declaration. Strict checkpatch reports zero warnings for the
patches.

The WSL 6.18 backport remains a separate source tree with its own DXG GPADL
consumer audit. The daily WSL host now boots Build #9 from this branch head
`a5cedb4de6f8` (receipt `/mnt/c/wsl/kernel-ramshared-v6.receipt`); the seven-
patch mainline series itself has still not been booted there, because it
targets Linux v7.3-rc4 and does not apply to the WSL tree. The initial series
was unversioned: its cover is `[PATCH 0/2]` with
`Message-ID` stem `20260918014017.2536753` with cover suffix `-1` and patch
2/2 suffix `-3`. The archive headers
confirm these were sent on September 17, 2026 (local time); the archive
indexed them on September 18 UTC. Because this was the initial unversioned
submission, a revised series must be labeled v2, not v3, if and when all gates
pass. The exact IDs and subjects are preserved in the
[linux-kernel archive](https://lists.openwall.net/linux-kernel/2026/09/18/276)
and for patch 2/2 in the
[patch archive](https://lists.openwall.net/linux-kernel/2026/09/18/265).
No revised email has been sent.

## Initial lab access audit — September 24, 2026

At the time of this audit, the available test environment was an x86_64 WSL2 guest. Generic QEMU/KVM could
boot the candidate kernel but does not provide a Hyper-V VMBus host, so it
cannot run the GPADL protocol or bind `uio_hv_generic` to a synthetic device.
The audit predates the disposable Hyper-V run below. The environment still
exposes no SEV or TDX guest device and cannot run Arm CCA.

## September 25, 2026 ordinary Hyper-V runtime

The exact four-commit series was built from Linux `v7.3-rc4` base
`93f51579e7df248780214094418f205253383cc5`, ending at
`b38b9c3e30feed33224961a5f2834f7775ed8c52`. `make -j4 W=1 bzImage` linked
successfully; the candidate booted as `7.3.0-rc4-ramshared-vmbus+` in an
ordinary x86_64 Hyper-V guest. Only the UIO and Hyper-V storage modules needed
for the lab were built and installed; the all-modules build was stopped to
preserve the approved 16 GiB virtual-disk limit. Unrelated W=1 documentation
and format warnings appeared in DRM, EFI, and TTM files.

Boot-time KUnit ran `hyperv-vmbus-buffer`: 5 passed, 0 failed, 0 skipped,
including rounding, overflow, order-zero fallback selection, failed-teardown
ownership, and partial-allocation cleanup cases. A second Hyper-V synthetic
NIC on a private switch was temporarily bound to `uio_hv_generic`; the primary
NIC stayed on `hv_netvsc` for management access. The trace captured 9 GPADL
headers, 656 body messages, and 9 teardowns, all with `ret 0`. Read-only
`mmap()` passed for all five `/dev/uio0` maps (4 MiB, 4 KiB, 4 KiB, 31 MiB,
and 16 MiB) and for the 4 MiB per-channel VMBus `ring` sysfs mapping. Closing
the UIO descriptor and unbinding the driver completed teardown; the test NIC
was restored to `hv_netvsc`. No BUG, Oops, KASAN, hung-task, or VMBus/GPADL
error was logged. The kernel did print an SRSO mitigation notice for the
virtual CPU.

This proves the normal GPADL/UIO lifecycle on ordinary x86_64 Hyper-V only.
It does not test live host error responses, force allocator fallback in a
live allocation, test rescind races, or qualify SEV-SNP, TDX, or Arm CCA.
Those gates remain open; do not claim universal architecture or CoCo support.

## September 25, 2026 order-zero fallback candidate

At the September 25 snapshot, the public kernel fork carried six ordered
`[PATCH v2 n/6]` patches and a matching consolidated snapshot. Patch 6 factors the production allocation
order-descent loop behind a private callback. Its KUnit test injects failure
at every order above zero, then performs and frees a real order-zero page
allocation; a second pass injects order-zero failure and checks clean
exhaustion. The patch applied exactly after patches 1–5 and passed local strict
checkpatch. Hosted run 36148296003 passed those six build stages on x86_64 and
arm64, the WSL backport, and 14/14 x86_64 KUnit tests (10/10 in the VMBus
suite). Artifacts record the pinned base and exact series SHA. The workflow
required all six patch stages and the new named case at that time.

## September 29, 2026 hosted CI and source re-audit (EVD-0112)

Public kernel-fork commit `b85e21326a41314047bd6e1ac864db39869315a4`
contains the tracked seven-patch series and the separate WSL 6.18.40.1
backport source. Hosted run
[36574925363](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36574925363)
used Linux `v7.3-rc4` base
`93f51579e7df248780214094418f205253383cc5`. It applied and built each patch
stage on x86_64 and arm64 with `W=1`, Sparse (`C=2`), and strict checkpatch;
all required tools, seven stages, and artifacts were enforced by the workflow.
The x86_64 KUnit run passed 24/24 (16 VMBus-buffer cases and four UIO-mmap
cases, plus four interrupt tests). The WSL backport build passed `W=1` and
Sparse for Hyper-V, NetVSC, and UIO, then passed its KUnit run 14/14 (11
GPADL-lifetime and three UIO-mmap cases). Arm64 KUnit was skipped. The run
artifacts record the base SHA, source SHA, series patch hashes, configs, and
logs.

Patch 0007 adds a retained-owner workqueue, a host-revoke state gate, and a
page-reference check before reclamation. The named tests exercise those gates
and simulated page references; they do not run an actual `/dev/uio` map/close
and unregister race, a live host response/rescind interleaving, or a full
channel open/close balance campaign. The build and KUnit run occurred on
hosted runners and did not produce or install a kernel image on the daily WSL
host. The active host remains `6.18.40.1-microsoft-standard-WSL2+ #6`; its
RamShared CLI remains `0.14.1`, with no `ramshared.service` unit registered.
The active image still has no immutable source receipt. No freeze cause is
attributed to this candidate.

## Next gate

Two of the three remaining runtime questions are exercisable in Actions. The
third is not, and the reason is structural rather than a drill bug.

**Exercisable here (ordinary x86_64 Hyper-V guest):**

- live GPADL create/teardown and response/rescind interleaving;
- the UIO mmap-close / unregister lifecycle, including the BUG-3
  hold-in-mmap window;
- that ring allocation survives real buddy fragmentation — the historical
  `accept4 110` claim on ordinary guests.

`EVD-0131` (run 36767912983) measured that a hosted Windows runner boots a
disposable WSL2 guest with 30 real VMBus devices, and `EVD-0132` (run
36768828972) measured that the same runners define and run an arbitrary Gen2
Hyper-V VM with no reboot. The pipeline that turns that into qualification
lives at `Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`,
driven by `.github/workflows/hyperv-runtime-drill.yml`. It builds a
self-booting mainline `bzImage` (`CONFIG_EFI_STUB` + embedded initramfs +
forced cmdline), boots it as a Gen2 guest, streams the COM1 named pipe, and
runs `vmbus-lifecycle-drill.sh` and `vmbus-fragmentation-drill.sh` against
those exact bytes. The guest is disposable, which is exactly the host-safety
contract those scripts require; they must not run on the daily WSL2 host.

**Not exercisable here: order-zero fallback of the chunked allocator.**
`vmbus_alloc_buffer()` only takes its physically-contiguous-chunk path when
`vmbus_uses_shared_page_chunks()` is true — host-visible buffer **and**
(Hyper-V isolation **or** `CONFIG_ARM64`). A hosted x86_64 runner is
neither, so every ring allocation there is `vzalloc()` and the chunked
order-N → order-0 degrade never runs. It would not be visible either: that
path allocates with `__GFP_NORETRY | __GFP_NOWARN`, so an order-7 failure
prints nothing. The fallback is already covered by named KUnit cases with
fault injection; its runtime proof is the same gate as CoCo and lives with
the SEV-SNP / TDX / Arm CCA qualification below. Do not treat a green drill
job as evidence for it.

**Measurement status — EVD-0133 (run 36777284600, fork `f831c80e0a2b`):**
harness green on both runners. The candidate booted as a real Gen2 guest
(`7.3.0-rc4+`, 14 VMBus devices, candidate symbols present) with **map
balance exact across 30 hv_netvsc rebind cycles** (12 maps / 20 279 296
bytes / 4 939 pages before and after), zero splats, zero faults, and channel
open under 1 464 MiB pressure. **Neither stress path executed.** All 30
cycles printed `bind_fail` — the dynid GUID was written brace-wrapped and
`guid_parse()` rejects it, so `uio_hv_generic` (whose `id_table` is `NULL,
only dynamic id's`) never bound and BUG-3 hold-in-mmap never ran — and the
buddy still held 120 order-10 blocks after the hog, so order-7 never had a
reason to fail. Both defects are fixed; the re-run is what closes the
GPADL/UIO half.

Scoring rules for the re-run, and for any future run: a drill that reports
`FAIL` is a real candidate failure. `INCONCLUSIVE` — pressure did not remove
the order-7 supply — is green for the job and `PARTIAL` for the gate.
`PARTIAL` from init means one drill ran and the other did not. A green job
is never by itself a gate closure; the `VERDICT_SCOPE` line names what was
actually proven and must travel with any citation. KUnit fault injection is
not a substitute for runtime evidence, and runtime evidence on an ordinary
guest is not a substitute for CoCo.

Obtain a suitable platform/lab for SEV-SNP, TDX, and Arm CCA memory-state
tests — Azure Confidential VMs are the no-silicon route
(`DCasv5`/`ECasv5`, `DCesv5`/`ECesv5`); Arm CCA has no cloud SKU. Match the
running WSL image to an immutable source/modules receipt before attributing
the prior freeze or promoting the separate backport. Keep the series unsent
until required runtime gates pass and maintainers review it. A disposable
ordinary Hyper-V runtime does not qualify the WSL backport or the actual WSL
host kernel, and a hosted guest is never a confidential guest — so no result
from this drill pipeline closes COCO-1..5.

## Rollback trigger

Any freed page with unconfirmed GPADL removal or unknown encryption state,
kernel warning/oops, ring corruption, or >3% matched throughput loss blocks
promotion; restore the previous booted kernel in a lab rather than hot-swap.
