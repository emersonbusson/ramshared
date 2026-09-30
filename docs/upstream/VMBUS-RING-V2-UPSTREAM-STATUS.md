# Upstream Proposal: Add Virtual Memory Fallback for VMBus Ring Allocations Under Fragmentation

> **Provenance.** Relocated 2026-09-30 from the public kernel contribution fork
> (`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/UPSTREAM-STATUS.md`)
> into RamShared. A contribution fork must not carry internal send-tracking.
> Sections below dated "September NN" are **point-in-time records and are not
> rewritten**; only the status header and cross-repository paths are current.

- **Target Repository:** [`microsoft/WSL#41634`](https://github.com/microsoft/WSL/issues/41634) (combined proposal) · [`microsoft/WSL#40795`](https://github.com/microsoft/WSL/issues/40795#issuecomment-5716513649) (solution comment) & Linux Hyper-V Subsystem (LKML)
- **Kernel Subsystem:** `drivers/hv/` (Hyper-V Synthetic Transport)
- **Patch Reference:** [seven-patch v2 series](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/vmbus-ring-buffer-upstream-v2/Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/series) (in the contribution fork) · [older consolidated draft](patches/vmbus-ring-buffer-v2-draft.patch)
- **Cover letter:** [DRAFT — do not send](vmbus-ring-v2-cover-letter-DRAFT.md). Prepared so the send is mechanical once the gates close; the PRD send gate is closed and the draft carries its own checklist.
- **Status (2026-09-30):** v1 proposal submitted 2026-09-17 and reviewed by
  Michael Kelley on 2026-09-22. The seven-patch v2 candidate is an **unsent
  draft by policy**. It is source-complete against the maintainer's five-point
  refactor request and its hosted build/KUnit gates pass (run 36574925363, and
  run 36590352003 for audit SHA `de5138b5`), but live GPADL/UIO lifecycle,
  forced order-zero fallback under real fragmentation, and SEV-SNP/TDX/Arm CCA
  evidence are all open. **Not ready to send.** Blocking gates live in
  [`GAP-REGISTER.md`](../reliability/GAP-REGISTER.md) and the SSDV3 suite at
  [`docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/`](../specs/no-milestone/vmbus-ring-buffer-upstream-v2/).

---

## 1. Problem Statement

Hyper-V synthetic channel rings (`vmbus_alloc_ring()`) require high-order contiguous physical page allocations—specifically Order-7 ($2^7 \times 4\text{ KiB} = 512\text{ KiB}$ contiguous blocks).

Under extended WSL2 developer sessions with heavy memory churn (compilers, language servers, containers, memory tiering), physical memory fragments heavily. Under these conditions, `alloc_pages(GFP_KERNEL | __GFP_ZERO, 7)` fails with:

```text
page allocation failure: order:7, mode:0xdc0(GFP_KERNEL|__GFP_ZERO)
```

even when multiple gigabytes of physical RAM remain free across lower orders (orders 0 through 3).

When `vmbus_alloc_ring()` fails, `vmbus_open()` aborts channel initialization. The Windows Host Compute System (`wslservice.exe`) deadlocks waiting for the channel handshake or fails with:
`Wsl/Service/E_UNEXPECTED (0x8000ffff)`
freezing the guest instance and requiring a full `wsl --shutdown`.

---

## 2. Pre-fix evidence status

The previously quoted order-7 kernel stack, buddy allocator snapshot, and
Windows `E_UNEXPECTED` output have no preserved run identity or raw artifact
linked to this proposal. They are treated as an unverified historical report,
not a reproduced trace. A future upstream submission needs the original log,
kernel build identity, time window, and a direct mapping from the VMBus
allocation failure to the observed host symptom.

---

## 3. Root Cause Analysis

Upstream Hyper-V guest drivers assume that physical memory contiguity can always be granted by the buddy allocator for order-7 requests. When high-order fragmentation occurs, there is no virtual allocation fallback in `vmbus_alloc_ring()`, which can cause channel initialization to fail. A direct causal link to the reported WSL host failure remains unproven here.

---

## 4. The Fix (Patch Series v2: `vmbus_alloc_buffer` Architecture)

### A. Non-Contiguous Chunked Buffer Allocation (`drivers/hv/channel.c`, `include/linux/hyperv.h`)
The candidate uses an owned `struct vmbus_buffer` for ring and selected VMBus
buffers. It retains the prior exported allocator/free/GPADL interfaces through
compatibility adapters while migrated in-tree users call descriptor-aware
`_owned` entry points:
- Automatically attempts high-order contiguous physical allocations first (`alloc_pages_node()`).
- Under physical fragmentation, dynamically falls back to decomposing the requested buffer into smaller contiguous physical chunks down to Order-0 individual pages.
- Maps the physical chunks into a contiguous kernel virtual address range via `vmap()`.

### B. Page-State Handling and Confidential-Guest Limits
The candidate calls the memory-encryption helpers on direct-map chunk
addresses before joining those chunks with `vmap()`. It records unknown or
failed page-state transitions and keeps those pages allocated. This is an
implementation strategy, not a compatibility claim: no SEV-SNP, TDX, or Arm
CCA transition has been qualified by the current candidate. No universal
Confidential Computing support is claimed.

### C. Unified Buffer Lifecycle Management
The descriptor groups buffer pages, virtual mapping, GPADL state, and the
retained-owner record. Ring, NetVSC, and UIO paths use it; StorVSC has not
been converted. The retained reclaimer waits for GPADL state, page-state, and
mapping references before freeing backing pages. No regression-free runtime
claim is established.

---

## 5. Validation status

Build #5 boot and swap observations are useful smoke evidence, but do not prove
that order-7 allocation failed and the fallback path ran. The previous stress
report (EVD-0046) is unqualified for physical VRAM residency and performance;
see EVD-0047. No measured channel latency, GPADL leak audit, or CoCo VM
decrypt/re-encrypt test is available. The v2 patch remains **PARTIAL** until
fault-injection, teardown, and confidential-VM tests pass on the exact patch.

---

## 6. Full Patch Reference

See the local [v2 patch draft](patches/0002-hv-vmbus-dedicated-ring-pool-and-virtual-fallback.patch). The current seven-patch series lives in the contribution fork (link in the header).

---

## 7. Reference implementation

A development implementation is maintained in the [emersonbusson/WSL2-Linux-Kernel](https://github.com/emersonbusson/WSL2-Linux-Kernel) repository. The commits below are implementation references, not release qualification:

- **Repository:** [`emersonbusson/WSL2-Linux-Kernel`](https://github.com/emersonbusson/WSL2-Linux-Kernel)
- **Reference Branches:** [`main`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/main) (default) & [`linux-msft-wsl-6.18.y`](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/linux-msft-wsl-6.18.y)
- **Implementation commits:**
  - Backport `vmbus_alloc_buffer` and `struct vmbus_buffer` for CoCo safety: [`812533440`](https://github.com/emersonbusson/WSL2-Linux-Kernel/commit/812533440)
  - Documentation and Enterprise Qualification: [`0f2c68208`](https://github.com/emersonbusson/WSL2-Linux-Kernel/commit/0f2c68208)
- **Testing on Host:** Follow the deployment guide in the fork's README to point `.wslconfig` directly to the compiled kernel.

## 8. September 24, 2026 review status

The installed WSL kernel is `6.18.40.1-microsoft-standard-WSL2+` build #6. Its
`bzImage` SHA-256 matches `C:\wsl\kernel-ramshared-v5`, and the running kernel
reports the same build identity. This proves the local candidate image booted;
it does not prove that the order-7 fallback was exercised or that the patch is
ready for upstream.

The proposal is now a five-commit mainline series, separate from the WSL 6.18
backport. Review fixed rounded-size overflow handling, audited the DXG
caller's GPADL ownership, and removed an unsupported universal CoCo
compatibility claim from the first commit message. The WSL backport preserves
pinned DXG pages when GPADL teardown is uncertain.

Michael Kelley's September 22 reply endorses using `vmbus_alloc_buffer()` for
ring allocations, grouping buffer and GPADL lifetime state, and preserving
memory when teardown or CoCo re-encryption cannot be proven. The WSL
[PR #41690](https://github.com/microsoft/WSL/pull/41690) now reduces the ring
order for selected host-initiated hv_sock listeners and describes a kernel
allocator change as complementary. It does not remove the high-order
allocation requirement from all VMBus users.

The exact source changes have passed hosted per-commit x86_64/arm64 builds and
the VMBus KUnit cases, along with the WSL VMBus/NetVSC/UIO Sparse build
and DXG compile. Allocation fault injection, UIO mmap, and Confidential VM
transitions remain untested. The ordinary Hyper-V runtime test is recorded in
EVD-0054. The September 24 issue comment records the relationship without
claiming the bug was reproduced or fixed.

The September 25 [mainline patch series in the kernel fork](https://github.com/emersonbusson/WSL2-Linux-Kernel/tree/vmbus-ring-buffer-upstream-v2/Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/series)
contains five commits: ring ownership (`50aac3dc3`), allocator and cleanup
safety (`ca42ecd6b`), UIO ownership (`cd8c10eab`), the corrected fallback
test vector (`5959b9109`), and GPADL post fault injection. The first commit avoids a universal CoCo
compatibility claim. It adds an arm64 CCA allocation guard, checked page
rounding, UIO GPADL buffer
ownership, and a guard against `vunmap(NULL)` during partial-allocation
cleanup. Nine VMBus KUnit cases cover rounding, overflow, order descent,
uncertain release ownership, partial cleanup, and injected GPADL post errors.
The hosted workflow now uses only
runner-provided tools, generates its KUnit config, and builds after every
patch. Run 36046920733 passed with the refreshed patch files: WSL
VMBus/NetVSC/UIO Sparse, separate DXG compile, per-commit x86_64/arm64
compile and Sparse, and all five named VMBus KUnit cases (nine KUnit cases
passed in total). It uses pinned Sparse source and fails if the checker is not
functional or gets silently disabled. Run 36040552037 passed the same code
with the prior patch mail; 36042727085 passed WSL but checkpatch found missing
descriptions and `Signed-off-by` trailers on commits 2–4, now fixed. Runs
36038457091 and 36039517554 exposed
and led to fixes for an invalid WSL make target and trace-only DXG variables
with DEBUG disabled. No cross-architecture or CoCo compatibility claim is
qualified. The new fifth patch adds callback-injected KUnit failures for GPADL
header, body, and teardown sends plus response-state mapping. Run 36143196834
passed all five patch builds and x86_64 KUnit 13/13, including the VMBus
buffer suite 9/9; arm64 KUnit is skipped. Live response/rescind interleaving,
UIO mmap, and CoCo tests remain open.
The unversioned September 17 `[PATCH 2/2]` makes
the next send v2, subject to
the ordinary Hyper-V and CoCo lab gates. The WSL 6.18 backport remains
separate. Its DXG destroy path now retains pinned user pages and its `vmap()`
when GPADL teardown is uncertain; the hosted backport build enables
`DXGKRNL`. DXG's externally pinned page encryption contract is still not
qualified for CoCo guests.

Sparse logs retain diagnostics in unchanged source, including a VMBus driver
context-imbalance warning and a flexible-array warning in the GPADL header.
The hosted artifacts preserve these logs; strict checkpatch reported no
warnings for the four original patches. The fifth patch passes strict
checkpatch, exact-state apply, hosted builds, and its KUnit cases.


## September 25 order-zero fallback test candidate

The versioned v2 draft now has six patch files. All subjects are numbered
`[PATCH v2 n/6]`, and the consolidated patch snapshot is regenerated from the
ordered series. Patch 6 factors the production page-allocation descent loop
behind a private callback. Its KUnit test injects failure at every order
above zero, permits a real order-0 allocation, frees the page, then checks
that injected order-0 exhaustion returns cleanly without underflow. Strict
checkpatch passes locally. Hosted run 36148296003 passed all six patch stages
on x86_64 and arm64, the WSL backport, and x86_64 KUnit 14/14, including all
ten `hyperv-vmbus-buffer` cases. The artifact records base
`93f51579e7df248780214094418f205253383cc5` and series commit
`dbec28671d5f7bb3c1017151574a7649019671aa`. This proves the fallback logic
under deterministic injected failures; it does not prove allocator
fragmentation on a live host. Live response/rescind races and SEV-SNP, TDX,
and Arm CCA transitions remain unqualified. Ordinary Hyper-V UIO mmap is
recorded separately in EVD-0054. No v2 email is sent until the required
runtime/platform gates and maintainer review are complete.

## September 28 seven-patch candidate re-audit

The current candidate is based on Linux `v7.3-rc4`
(`93f51579e7df248780214094418f205253383cc5`) and has seven ordered patch
files. Patch 7 adds channel-keyed retained buffer owners, GPADL acknowledgment
and host-rescind state updates, delayed reclamation, page-reference checks
for UIO mappings, and adapters that preserve the previous exported APIs.
The independent re-audit found the earlier status text stale: this is a
production reclaimer in the source tree, not test-only cleanup. The host
rescind path still relies on a protocol ordering assumption that the Linux
documentation does not explicitly define for GPADL page access.

The seven patches apply sequentially to the pinned base. `git diff --check`
passes after each patch, cumulative `checkpatch.pl --strict` reports zero
errors/warnings/checks at every stage, and the applied tree matches the exact
candidate source byte-for-byte. The workflow now checks that same cumulative
source diff after each patch; applying checkpatch directly to the mail files
had reported blank context lines as trailing whitespace. The stale duplicate
`0007-gpadl-rescind-reclaim.patch` was removed so the workflow applies exactly
seven files. The consolidated snapshot was regenerated from those seven
files.

The current patch 7 is a plain unified diff, not a signed-off mail patch. It
is an apply/build candidate and is not ready for LKML transmission. The latest
API-adapter source has not yet run hosted compilation or KUnit. Prior hosted
run `36148296003` qualified only the earlier six-patch series and is not
evidence for this patch. No local heavy build, install, host stress, CoCo
transition, or mailing-list send was performed.

**Current status:** PARTIAL and blocked for upstream send/installation until
hosted x86_64/arm64 build and KUnit pass, ordinary Hyper-V rescind/close and
UIO mmap interleavings are exercised on this exact source, and supported CoCo
page-state transitions receive platform-specific evidence. No all-architecture
or universal CoCo claim is made.

## September 28 UIO page-protection follow-up

Source review found that UIO's ring, receive, and send mappings could retain
the default encrypted page protection even when the owned allocator had
already transitioned their backing pages to shared/decrypted state. The
monitor page is also decrypted by VMBus setup. The candidate now maps the
explicit page arrays, validates map/range selection, requires `MAP_SHARED`,
keeps the normal UIO no-expand/no-dump flags, and applies decrypted protection
only to the shared buffers and monitor page. Four named UIO KUnit cases and
workflow assertions were added. The cases have not yet run on this revision.

The exact source remains PARTIAL until the hosted x86_64/arm64 build and KUnit
run passes, then live Hyper-V UIO close/unregister and CoCo page-state tests
qualify the lifetime. Existing hosted run `36148296003` covers six earlier
patches only. The series is not ready for LKML or kernel installation.

## September 28 WSL mapping-reference follow-up

Source review found that the WSL backport's KUnit test required mapping
references to defer re-encryption, while its production free helper did not
check those references. The free path now retains chunk-backed or `vzalloc()`
pages while UIO PTE references remain and retries only after the GPADL and
encryption gates are clear. Module exit cancels pending delayed work and waits
for active reclaim callbacks. The disconnected VMBus message path also frees
its allocated context.

The WSL hosted job now records its source SHA and kernel version, builds the
changed VMBus, NetVSC, UIO, and DXG objects with W=1/Sparse, and runs the
11-case GPADL lifetime and three-case UIO mmap/protection KUnit suites under
QEMU. The legacy UIO callback now maps pages with references, requires shared
mappings, and applies decrypted protection to the shared ring/receive/send and
monitor regions. Sysfs ring offsets are applied once. These checks have not
yet run on the revised source. Live UIO mmap close/unregister, ordinary
Hyper-V GPADL interleavings, and supported CoCo page transitions remain
environment gates; the source is not qualified for installation or upstream
send.
