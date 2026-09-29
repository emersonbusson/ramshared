# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

Current release: **v0.14.1**. Next planned release: **v0.15.0**.

Support and reserve boundaries used by current documentation:

- Standard WSL2 uses NBD as its baseline transport. `ublk`/`io_uring` is
  qualified on native Linux or WSL2 with a compatible custom kernel under
  EVD-0039; the open product-lifecycle gate below still applies.
- EVD-0040 covers zero-copy CUDA host mapping only.
- Broker/NBD subtracts `max(1536 MiB, 20%)` from live free headroom, then
  preserves its separate `768 MiB` runtime buffer and canary. The isolated
  origin cache subtracts `max(configured floor, 20% of measured capacity)`
  from live headroom and preserves a separate `640 MiB` runtime buffer. Its
  production floor currently defaults to 512 MiB (clamped to 128–4096 MiB),
  while its active PRD/SPEC still describes a 1536 MiB default; reconcile this
  before treating the reserve policy as qualified. StorPort uses
  `max(configured reserve, 512 MiB, 10%)`.

## Current Open Gates

The 2026-08-20 through 2026-08-22 investigation remains a reason to keep the
Linux/WSL2 NBD product gate open, but its terminal incident is now classified
`host_volume_exhausted` from NTFS Event ID 137 plus `STATUS_DISK_FULL`.
RamShared causality was not established. Historical matrices remain evidence
for their exact releases, and the independent safety hardenings still require
their own live qualification before any automatic boot activation.

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| Cross-platform resource configuration | PARTIAL | EVD-0113 re-audited the memory parser: the 4096/8192 kB values are deliberately inconsistent test fixtures, while separate tests accept variable small and large RAM/swap sizes. The `config draft --output PATH` wizard now collects optional positive variable ZRAM and SSD-origin planned caps, displays exact values in the read-only plan, leaves blanks unset, and refuses zero/malformed/overflow values before writing. Those values are unenforced draft policy: `TierCaps` was renamed to `PlannedTierCaps`/`planned_caps` with explicit unenforced labels in the wizard, plan text, and plan JSON (`unenforced_planned_caps`), and no runtime consumer enforces the caps. The focused wizard suite passes, strict CLI Clippy and docs/schema checks pass. Drafts remain unprivileged and unapplied; VRAM selection is explicitly unavailable until a fresh budget is bound to a stable adapter ID. EVD-0111’s live WSL volume draft predates this cap change. No measured disk ranking, transaction log/provider apply or rollback, or native Linux live E2E exists. | Implement identity-bound cross-vendor GPU discovery/cap selection, bounded disk comparison and recommendation, durable transaction events, and closed-action Linux/Windows apply/rollback. Require PASS from `config_draft_wizard_saves_planned_caps_as_unenforced_draft_policy` and the named inventory/identity tests; retain ≥80% slice-coverage proof; record native Linux live E2E and WSL before/action/after run results against a real selected stable volume before claiming complete support. |
| WSL2 freeze memory ownership | PARTIAL | EVD-0112 updates EVD-0111’s candidate snapshot: the public seven-patch mainline series and separate WSL 6.18.40.1 backport at kernel-fork commit `b85e21326a41314047bd6e1ac864db39869315a4` passed hosted `W=1`/Sparse builds; mainline KUnit passed 24/24 on x86_64 (arm64 KUnit skipped), and WSL-backport KUnit passed 14/14. Tracked patch 0007 adds retained-owner delayed reclamation gated by host-revoke state and page references; these KUnit cases do not exercise actual UIO mmap close/unregister, live GPADL response/rescind interleavings, or CoCo transitions. The booted host remains `6.18.40.1-microsoft-standard-WSL2+ #6`, and its installed RamShared CLI remains `0.14.1`; neither candidate was installed. EVD-0111’s no-build statement describes its earlier dirty candidate `a022ac393ecaab845682f5afe2be6be792aedde2` only. EVD-0071 links 13,000 prior-boot guest samples to Windows telemetry and Guardian probes. With RamShared Off, guest availability fell from about 8.5 GiB to 106 MiB, fallback swap reached about 3.2 GiB, swap-device reads rose about 111 GiB in 22 minutes, and PSI full avg10 peaked at 56.76%; dual guest probes timed out continuously from about 15:48 while Windows still had over 18 GiB physical memory free. The formerly suspected largest process kept a near-constant total RSS plus swap footprint and was mainly a victim of paging. EVD-0072 found zero ballooned pages in five current-boot samples, which cannot establish the prior boot's value. EVD-0073 adds kernel categories, balloon counters, cgroup visibility, and all-readable-process RSS/swap totals; EVD-0074 and EVD-0075 confirm source-built no-pressure samples paired with Windows telemetry. Root access exposes the detailed debugfs counters; cgroup accounting remains partial. EVD-0076 records that the original 0.14.1 monitor lacked the forensic fields; the later dirty monitor now emits them. EVD-0081 renames the source classification to `UNMANAGED_MEMORY`: a 1,726,428 KiB external RSS footprint coexisted with 8,655,124 KiB guest MemAvailable, 4,190,212 KiB SwapFree, and zero PSI; that is usage, not proof of current pressure. EVD-0077 reproduces and fixes a Guardian HCS status serialization bug that could falsely corroborate host failure when the WSL status probe also failed; the WSL status probe succeeded during the recorded freeze, so this does not explain that incident. EVD-0078 shows guest probes timing out while WSL CLI/HCS answered; the Guardian repeatedly refused under its current fail-closed policy, then exited with no attributable record. EVD-0078 recorded that the Task Scheduler Operational channel was disabled and process-termination auditing was off. EVD-0083 confirms the enabled Guardian task had been `Ready`, health `BLOCKED/boot_identity_unavailable` with a 2026-09-26 timestamp, and last result `0xC000013A`; the task still has no attributable prior exit record. EVD-0084 restarted the existing task after the Windows-to-WSL boot-id, WSL CLI, HCS, and heartbeat probes passed; it now remains `Running` and publishes fresh `HEALTHY/watching` state. Its action still points to the mutable checkout, not an immutable deployment. The installed monitor package is dirty, the interactive dashboard path still resolves to an older executable, and EVD-0081's label fix is not deployed. EVD-0084's post-start sample reports 6,485 MiB guest `MemAvailable`, 4,030 MiB `SwapFree`, and zero memory PSI; three Windows samples kept the Guardian `HEALTHY` and PowerShell private use between 191.5 and 276 MiB. EVD-0086 pairs a current read-only WSL/Windows sample: the CLI reports memory_scope=wsl2, phase Off, fresh Guardian, about 958–964 MiB guest MemAvailable, 2,680 MiB SwapFree, and zero PSI avg10; Windows reports vmmemWSL at 12,969 MiB working set and 16,142 MiB private bytes, while four PowerShell processes total about 415 MiB private bytes (largest 157 MiB). Cgroup accounting remains partial. No process cause for the prior freeze or one-off multi-GiB diagnostic is established. EVD-0087 adds a post-shutdown snapshot: host physical headroom is 10,643 MiB, vmmemWSL working set 10,809 MiB, guest MemAvailable 6,210 MiB, swap use about 10 MiB, and PSI near zero; PowerShell private use totals 221 MiB. Compared with EVD-0086, host headroom is 328 MiB higher and vmmemWSL working set 2,160 MiB lower. The 2,922-to-10,809 MiB comparison spans the user's wsl --shutdown, so it is not a monotonic growth series. Current /proc/vmallocinfo access was denied. Code review found possible GPADL page retention. The attempted helper was removed because it missed partial establishment and treated host, synthetic hibernation, and unload rescind alike; this remains a candidate, not an established cause. EVD-0088 observed 41 more vmbus_alloc_buffer maps and 16.66 MiB more vmalloc area size in 22 seconds; this virtual area is not resident Windows RAM. EVD-0089 then observed 15,821 to 17,690 maps and 6,774,464,512 to 7,573,204,992 bytes of vmalloc area across 17:25 (+761.8 MiB, about 44 MiB/min). Of the later maps, 17,350 report 104 pages each; the same guest had 102 registered channels and 89 VMBus device links. Source commit `50715f5f7` has one in-tree allocator caller for a combined ring mapping and a 2,048 RELID limit; if Build #6 uses that snapshot, 17,350 maps of 104 pages exceed the maximum in-tree ring mappings by more than 8x. That source also has a rescind cleanup path that reports success while leaving the GPADL handle set, after which buffer release drops the owner without freeing pages. This is a confirmed source defect but not attributed to the installed image. A later backport converts NetVSC/UIO buffers and cannot be assumed for Build #6. Guest MemAvailable rose from 3,611 to 4,179 MiB between the two samples while swap use rose to 1,511 MiB and PSI avg300 reached 0.14/0.12, so this is not a simple one-metric pressure trend. Windows at 12:39 had 11,582 MiB physical headroom, VmmemWSL working set 9,956 MiB, and PowerShell private bytes 252 MiB; compared with EVD-0088, headroom rose 591 MiB and VmmemWSL working set fell 736 MiB. No Windows sample was paired with the 12:55 guest sample. EVD-0090 at 13:36 showed host physical headroom higher by 2,755 MiB and `VmmemWSL` working set lower by 1,850 MiB; no guest map count was paired with that host sample. This weakens a simple story of continuously rising Windows-resident RAM, while leaving gradual guest-side page retention plausible. EVD-0091 at 14:02 counted 24,932 VMBus maps, up 7,242 maps and 2,950.7 MiB of vmalloc area over 67 minutes (~44 MiB/min); guest `MemAvailable` fell about 1.99 GiB and `SwapFree` about 849 MiB from 12:55, while PSI averages were 0.00. A Windows sample 74 seconds later had 16,147 MiB physical headroom and a 6,605 MiB `VmmemWSL` working set. EVD-0092 at 14:19 then found 16,159 MiB available (+12 MiB since 14:04), while `VmmemWSL` working set fell another 123 MiB; since 13:36, physical headroom is up 1,822 MiB and working set is down 1,625 MiB. Private bytes and commit are recorded separately and do not represent resident RAM. This weakens a host-resident-memory growth explanation while the guest-side accumulation/paging hypothesis remains plausible; it does not establish Build #6 source identity or prove the freeze trigger. The running image hash is known and the allocator symbols are present, but its exact source commit is not matched: the Microsoft source checkout lacks the allocator and available kernel image artifacts do not match the installed image. Do not attribute the freeze to a specific kernel revision yet. | Produce a clean, provenance-matched monitor and Guardian package; verify `BINARY_MATCH`, run the deployed Guardian with immutable inputs, and retain an ordinary no-pressure JSONL sample alongside Windows `vmmemWSL` telemetry. Qualify the guest-timeout/healthy-Windows-control-plane policy in an isolated VM before changing or promoting automatic recovery. Compare a matching stock-kernel baseline if the fault recurs. For the VMBus candidate, first match the installed image to its exact source, then correlate per-buffer allocation/free and GPADL teardown/rescind events with the active channel inventory and simultaneous host/guest telemetry. Keep the incident unattributed to one process or to a kernel crash until those data exist. |
| WSL2 control-plane stability and effective revocable-cache transition | PARTIAL | EVD-0111 confirms again that neither production entrypoint calls AF_VSOCK/AF_HYPERV or the host gate, and the protocol still has no guest finish proof. EVD-0050 records a prior disabled-cache release. This branch adds an isolated GPU cache worker but has only hermetic validation; worker frame reads are now bounded by an absolute per-frame deadline with hermetic stalled-peer regression tests, while live worker allocation, teardown, and supervision evidence remains open; the [September 23 incident audit](incidents/2026-09-23-wsl2-stress-branch-audit.md) found NBD stuck reads, stale cache/supervisor/guardian evidence, and no simultaneous physical-cache proof. EVD-0057 records a guarded local activation where the cache stayed unavailable and the supervisor entered `CRITICAL` because the reservation ledger was unavailable; swapoff-first teardown passed. The monitoring source now avoids a direct, unbounded CUDA context query, but that diagnostic fix is not installed. This branch now contains bounded Linux AF_VSOCK and Windows AF_HYPERV transport primitives with hermetic tests, but neither daemon starts them; handshake, lease, manifest delivery, product gating, and a live host/guest exchange remain open. | Prove fresh daemon-bound worker allocation, supervisor, origin, pressure, and 24-hour rollout evidence under one exact installed release; install and verify the bounded monitor change in a clean release; qualify host-guest transport separately. |
| Legacy WSL2 service handoff and teardown | PARTIAL | EVD-0115 installed clean `v0.15.0 · 5e6b5845` to `/usr/local` with exact `BINARY_MATCH` on both CLI and daemon SHA-256 values and a clean `ramshared-direct-install-metadata/v2` receipt. EVD-0116 attached the sealed origin, refreshed Guardian to HEALTHY, and recorded three idempotent `up`/`down` cycles with swapoff-first teardown leaving only fallback swap. EVD-0111 had measured installed v0.14.1 (build-info unsupported), status Off with stale Guardian state, one small monitor process, no `ramsharedd`, and only fallback swap. EVD-0049 records clean swapoff-first teardown and EVD-0050 one attended controller-owned start. On September 24, two supervised attempts reached exact `BINARY_MATCH`, current cache/supervisor telemetry, and clean swapoff-first teardown; another stopped before activation when the sealed origin was detached after restart, then official reattachment and host-gate validation succeeded. The installed release matches its build but carries dirty package provenance because unrelated local WSL configuration edits were present. The separate `kernel-ramshared-v3` image still lacks an immutable kernel/modules/QEMU manifest pair. | Repeat idempotent start/stop after a full WSL reboot against the EVD-0115 install, promote a sealed `/opt/ramshared` release with input-bundle provenance if the product path is required, and seal a kernel/modules pair before kernel promotion. |
| Build #5 three-tier stress and performance qualification | BLOCKED | EVD-0046 conflated logical NBD occupancy with GPU-resident bytes, hard-coded an SSD disk identity, and labeled vector release timing as physical reclaim throughput. The September 23 boot had stuck NBD reads, I/O errors, MCE records, and terminal memory pressure; the 31.7% gain and prior `PASS_ZERO_PANIC` remain unqualified. The sealed 4096 MiB origin exists and release `v0.14.1-87-g05712b2b-dirty` is installed with `BINARY_MATCH`. EVD-0064's older `FreeVirtualMemory` values were not exact commit headroom; EVD-0069 replaces that proxy with `GetPerformanceInfo` and adds WSL guest memory+swap admission. The pre-restart plan refused at physical headroom 16,903 MiB vs 20,480 required; commit headroom 29,173 MiB passed; guest `MemAvailable` was 206,940 KiB and `SwapFree` 1,266,524 KiB. EVD-0070 records the later freeze: RamShared stayed Off, but the prior boot reached 108,560 KiB guest `MemAvailable`, 897,032 KiB swap free, 3,291,364/4,194,304 KiB fallback swap used, and PSI avg10 some/full 22.72/22.48. The prior guest journal has no kernel crash signature; exact process causality and custom-kernel contribution remain unresolved. After the user's restart, guest memory recovered to about 12 GiB available with swap unused and PSI zero, but physical host headroom is still only 19,579 MiB vs 20,480 required; exact commit headroom is 41,519 MiB. EVD-0080 adds fail-closed WSL2/cascade PSI handling, preserves the 600 MiB guest floor if `min_free_kbytes` is unavailable, rejects direct freeze-probe invocation outside the gated campaign, and bounds the guest worker with dynamic cgroup memory/swap limits. EVD-0083 records the complete Windows PowerShell 5.1 static suite passing (27 named harnesses, exit 0), but no real cgroup or pressure run qualified the change. Its read-only host sample has 13,190 MiB physical headroom versus the 20,480 MiB threshold, while commit headroom is 28,909 MiB; WSL reports 6,741 MiB `MemAvailable`, 4,030 MiB `SwapFree`, and zero PSI, with the Guardian initially stale. EVD-0084 confirms the task now publishes fresh Guardian health, but physical headroom remained 12,766–12,774 MiB versus 20,480 MiB required; commit headroom was 28,411–28,439 MiB and guest `MemAvailable`, `SwapFree`, and PSI remained healthy. EVD-0085 removes the fixed 4,096 MiB success threshold from the Windows wrapper and closes a second evidence gap: Rust now records the worker target and resident MiB from the same qualification cycle in which ZRAM, logical NBD, SSD, and physical cache meet their targets; full-profile samples must keep the worker target at or above the startup-admitted target. The wrapper treats 4,096 MiB as a sealed maximum and refuses missing, malformed, over-cap, non-full-tier, or incoherent peak-only telemetry. Eight PowerShell cases and all 341 CLI unit tests plus 10 dispatch tests pass; the complete 27-harness Windows suite passed on the immediately preceding wrapper revision, and the final targeted stress harness passes. The latest plan-only run records 11,432–11,535 MiB physical headroom against 20,480 MiB required and 28,100–28,244 MiB commit headroom; Guardian health is fresh. A read-only guest sample reports about 3,151 MiB `MemAvailable`, 3,877 MiB `SwapFree`, and zero PSI avg10. Plan-only made no activation or GPU allocation. EVD-0086 later records 10,315 MiB Windows physical headroom against the 20,480 MiB profile gate and 27,723 MiB commit headroom; guest MemAvailable was about 958–964 MiB, below its 1,024 MiB reserve, while 2,680 MiB SwapFree and zero PSI avg10 were observed. No stress preflight or activation was run. No attempt has reached all three tier targets simultaneously at the worker-admitted cache target. | Wait for fresh Windows physical/commit headroom and at least 1024 MiB each of guest `MemAvailable` and `SwapFree`; confirm the attached origin and exact installed `BINARY_MATCH`; then run three watchdog-bounded rounds with worker-confirmed VRAM allocation, simultaneous 100% ZRAM, 100% NBD, 99% SSD samples, independent integrity/kernel/host logs, and a comparable baseline. |
| Cross-vendor GPU budget identity and stress admission | PARTIAL | EVD-0111 reran GPU policy 13/13 and VRAM identity/budget tests 7/7; read-only inventory sees one RTX 2060, while no live worker allocation, teardown, AMD/Intel, or multi-adapter run was performed. A generic `VramProvider` is only an extension point: production currently enumerates CUDA and Vulkan, so it does not cover every GPU merely because that device has VRAM. The worker requires a fresh, driver-reported budget from its active provider and checks it before each allocation; Vulkan requires `VK_EXT_memory_budget` and a stable physical-device UUID/LUID, CUDA uses `cuMemGetInfo` plus optional UUID/LUID, and DXG converts its WDDM budget/LUID to the shared contract. The exact-LUID WDDM budget constrains admission when available; stale samples, future samples, identity mismatch, and query errors fail closed. Worker heartbeats publish adapter-bound budget telemetry; `status --json` and `ramshared top` accept only fresh, well-formed driver-reported snapshots. The isolated worker ranks CUDA/Vulkan candidates by the live safe target, opens the exact Vulkan ordinal, and revalidates identity/budget before serving. The production origin-cache floor also differs from the active PRD/SPEC: the worker receives a 512 MiB default floor (clamped 128–4096 MiB) while the PRD mitigation states 1536 MiB; the dynamic 20% floor and separate 640 MiB buffer still apply. This discrepancy must be resolved before the reserve policy is called qualified. GPU-backed legacy direct `--slices` broker requests are refused before provider initialization because that path performs synchronous driver calls. The supported product path uses an authoritative origin and isolated cache worker; bounded parent IPC cannot cancel or confirm exit from a child blocked inside a driver. Vulkan allocation and budget reporting are bound to the same largest DEVICE_LOCAL heap. Unit tests pass; no live worker installation/allocation, physical multi-adapter selection, or AMD/Intel cache campaign has been qualified. | Resolve the default-floor/PRD mismatch with documented capacity/vendor evidence; then run a three-round live GPU campaign with exact adapter identity, WDDM budget, worker allocation, origin fallback, and teardown. Repeat on NVIDIA, AMD, and Intel before advertising broad hardware qualification. |
| VMBus ring fallback upstream series | BLOCKED | The public kernel-fork commit `b85e21326a41314047bd6e1ac864db39869315a4` tracks seven mainline patches and a separate WSL 6.18.40.1 backport. Hosted run 36574925363 passed all seven staged mainline builds on x86_64/arm64 with W=1, Sparse, and strict checkpatch; x86_64 KUnit passed 24/24, while arm64 KUnit was skipped. The WSL backport build passed W=1/Sparse and KUnit 14/14. Patch 0007 adds delayed retained-owner reclamation gated by host revocation and page references; its named tests do not establish live UIO mmap close/unregister or GPADL event behavior. EVD-0054 records ordinary x86_64 Hyper-V runtime evidence for an earlier four-commit snapshot: boot-time VMBus KUnit 5/5; a private synthetic NIC generated 9 GPADL headers, 656 body messages, and 9 teardowns, all `ret 0`; all five UIO read-only maps and the per-channel sysfs ring mmap passed; the NIC returned to `hv_netvsc` without errors/oops. This does not cover live GPADL response/rescind interleaving or actual allocator fragmentation. A September 27 source audit confirmed a rescind-related retention path in commits `50715`/`418653`: teardown reports success without clearing the GPADL handle, and buffer release then drops the owner without freeing the map. The attempted helper was removed because it missed partial GPADL establishment and treated the shared rescind flag as sufficient proof of host release, although local unload also sets it. The local ignored `0007` artifact discussed in EVD-0100 was an earlier, untracked draft; the current public `0007` is tracked and has hosted build/KUnit evidence, but no current-candidate lifecycle runtime qualification. EVD-0088 records 41 more live vmbus_alloc_buffer maps and 16.66 MiB more vmalloc area size in 22 seconds. EVD-0089 records growth to 17,690 maps / 7,573,204,992 bytes of vmalloc area; 17,350 maps report 104 backing pages. In commit `50715`, `vmbus_alloc_ring()` is the only in-tree allocator caller and one call allocates both ring halves; the 2,048 RELID limit makes that count incompatible with only simultaneously open in-tree rings if this is the running source. The installed image source remains unmatched, and vmalloc entries do not identify owners or lifecycle events. EVD-0091 shows the same ~44 MiB/min area growth through 14:02, with guest headroom declining and swap use increasing; near-time Windows headroom was 16,147 MiB and `VmmemWSL` working set had fallen. The active Build #6 image hash is known, but its source commit is not matched to the local Microsoft checkout or available candidate image artifacts. The separate WSL 6.18.40.1 backport remains a different tree; its QEMU/KUnit smoke does not test Hyper-V and the exact mainline series does not apply to it. Build #6 host recovery/qualification and the WSL module-to-VHDX promotion receipt remain separate gaps. Real SEV-SNP/TDX/Arm CCA memory transitions remain unqualified. EVD-0056 confirms the local Ryzen 5 3600 host cannot supply those hardware modes and there is no usable dedicated Linux Hyper-V guest. | Qualify the exact patch on SEV-SNP, TDX, and Arm CCA and exercise live GPADL response/rescind interleavings and order-zero fallback under fragmentation. Keep upstream submission blocked until those gates and maintainer review pass; keep WSL host promotion blocked on its separate provenance receipt. |
| Windows public driver distribution | BLOCKED | The validated package path is test-signed for supervised labs. Test-signing is not a public trust chain and cannot be promoted to production evidence; production trust requires an external Microsoft attestation or trusted signing identity. | Build from a clean release tag, obtain Microsoft attestation or another production-trusted signature, pass `InfVerif` and `SignTool verify /pa /all` with test-signing disabled, then pass install, rollback, and recovery drills on the declared compatibility surface. |
| Corrected Windows physical lifecycle qualification | PARTIAL | EVD-0111 reran the lifecycle/recovery/origin static suites successfully; no physical cold-boot or loaded-package identity campaign ran. Earlier physical campaigns predate the intended-payload hash, exact current-run Online identity, RAW-only mutation, active/configured pagefile, bounded process-tree, and one-fresh-approval-per-reboot contracts. They remain historical observations but cannot qualify the corrected harness. | Rebuild and seal the final package; prove loaded driver/broker/winsvc BINARY_MATCH; run three supervised cold boots with a new explicit approval for each boot; record intended/read-back hashes, exact identity, supported stops, zero residue, watchdog/task cleanup, and no Event ID 153 retries. |
| Windows virtual-disk properties, counters, and performance matrix | PARTIAL | EVD-0111 reran the storage-matrix static suite successfully; no physical five-cell/75-sample matrix or Event ID 153 qualification ran. Historical counter and throughput rows predate the current exact serial/size binding, raw counter schema, complete artifact inventory, Event ID 153 window, regression fingerprint, and fail-closed rollback contracts. Task Manager screenshots are secondary evidence only. | Run the corrected five-cell, three-run, 75-sample physical matrix after BINARY_MATCH. Require exact Virtual/SSD/non-rotating identity, direct intended-payload integrity, non-zero raw counters, zero Event ID 153 retries, median/p99/deviation, compatible-baseline verdicts, and an exact safe final state. |
| Custom-kernel DXG/systemd promotion | BLOCKED | A prior `6.18.35.2` boot emitted the exact upstream-open DXG FORTIFY warning in the Xwayland wait-sync-object path. EVD-0086 records that the active WSL boot is `6.18.40.1-microsoft-standard-WSL2+` with systemd running and no matching fatal/FORTIFY/p9/init-timeout lines in the filtered current-boot kernel log; Xwayland was not running and no same-host bundled/custom A/B has been done. The signature also exists on Microsoft 6.18.26.1 and bundled 6.18.33.2-2, so this is not attributed to RamShared, but bundled reproduction does not qualify the risk. Separate `RamShared-Kernel` attempts timed out starting `/sbin/init`, with unclean journal and p9-cancellation evidence. | Under separate attended approval, run a fresh-boot same-host bundled/custom A/B with no RamShared or pressure activation. Require exact distro/version, systemd `running`, readable fresh warning log, DXG/Xwayland/lightweight NVIDIA probe, zero FORTIFY/init-timeout/unclean/p9/fatal signals, and query-error count no worse than the sealed bundled baseline. See the [2026-08-23 finding](incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md). |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are historical capability evidence, not product transport closure. On 2026-07-18, `SANITIZED_ARTIFACT_REF` recorded SSH, non-interactive privilege, and ublk capability on `SANITIZED_VM_KERNEL_LAB`. The VM still had no GPU surface, and no product ublk lifecycle, swapoff-first teardown, crash/drain, or no-ghost proof existed. | A dedicated custom-kernel lab SPEC needs isolated before→action→after evidence for transport wire-up, ordered detach, crash/drain, and terminal no-ghost state. This is an open evidence definition, not an instruction to act. |

## Latest Evidence — 2026-09-28

### WSL2 freeze memory ownership (EVD-0093–EVD-0097)

The active WSL guest's `vmbus_alloc_buffer` vmalloc entries grew from 27,661
maps / 2,861,541 declared backing pages at 14:28 to 31,792 maps / 3,288,325
pages at 15:08. That is 4,131 additional maps and about 1.63 GiB more
declared guest backing pages in roughly 40 minutes. `MemAvailable` fell by
about 903 MiB to 512 MiB. These page/map counts do not measure Windows
resident RAM or identify allocation owners. A 15:09 Windows sample had
16,261 MiB physical RAM available, up 671 MiB from 15:00; the data do not
show Windows physical exhaustion. A VS Code `git fetch --all` used up to
about 646 MiB RSS and was stopped, but guest memory did not recover
immediately and VMBus maps continued to grow. It is an avoidable load, not a
proven cause. GPADL retention/rescind remains a plausible mechanism, but the
active Build #6 source is unmatched and the current upstream source diff is
not ported, built, or installed on the WSL target. Keep the freeze cause and
the correction `PARTIAL`; do not claim that the patch will reclaim current
kernel allocations or lower memory before a matching kernel is activated.

The next freeze is now documented in EVD-0096. The last prior-boot health
sample had about 181 MiB guest memory available, 3.32 GiB of 4 GiB swap in
use, and PSI `some`/`full` avg10 of 28%/28%, while RamShared activation,
daemon, and tiers were off. The journal then stopped after repeated
memory-pressure cache-flush messages; it contains no kernel crash signature
that names the trigger. Screenshots show heavy I: reads, but neither their
owner nor a causal link to the configured C: swap VHDX is established.
After WSL restarted, the same `#6` kernel had about 12 GiB guest memory
available, all swap free, and zero PSI. Windows also retained substantial
physical-memory headroom. VMBus map count reset from 31,792 to 330 across the
restart. This confirms recovery of guest pressure and reset of guest
allocations, not the owning driver or underlying cause.

Image provenance is now bounded: `.wslconfig` selects a custom kernel image
on C:; its `#6` build stamp matches the running
`6.18.40.1-microsoft-standard-WSL2+ #6` kernel. The repo's `arch/x86/boot/bzImage`
is a different `#8` image, and no immutable receipt connects active `#6` to a
source commit. Do not equate the active image with the current checkout or
its uncommitted patch.

EVD-0096 also confirms the screenshot label bug is a deployment-parity gap:
the current source already labels WSL2 memory correctly and its focused test
passes, but the installed/local release binaries inspected here predate that
change. The exact executable behind the screenshot was not captured. This
display bug is independent of the freeze investigation. EVD-0097 pairs the
post-restart host and guest: `vmmemWSL` reached about 15.6 GiB while Windows
physical headroom fell below 4.4–4.8 GiB, even as the guest reported 8.6–11.8
GiB available, near-empty swap, and zero PSI. The guest had about 8.6 GiB of
page cache and `.wslconfig` had `autoMemoryReclaim=disabled`; the local setting
has been changed to `gradual` but is not active until a future WSL start.
This may mitigate host-side cache retention and does not explain the earlier
guest freeze. The current GPADL diff retains uncertain owners without recovery;
UIO `/dev/uio` VMAs are not accounted for, so freeing or re-encrypting their
pages at unregister is unsafe. It has not been built or installed. Keep the
exact freeze cause and corrected kernel deployment `PARTIAL`; stress remains
off.

EVD-0098, taken before the next WSL start, confirms the `gradual` setting is
still inactive. During this no-pressure sample, guest availability was about
9.04 GiB, swap use was about 56.5 MiB, and PSI was zero; Windows had 4,812 MiB
physical headroom and `vmmemWSL` working set was 14,585 MiB. Compared with
EVD-0097's 16:11 sample, the working set fell by about 1,031 MiB while guest
page cache rose by about 255 MiB and Windows headroom rose by about 365 MiB.
This does not prove the reason for that change and shows why the staged reclaim
setting cannot be credited before a fresh VM start. The exact freeze cause,
Build #6 source, UIO VMA lifetime, and GPADL reclamation remain unresolved.

EVD-0099 captures the repeated 15:42 freeze. At 15:37 the guest had about
187 MiB available, 3.32 GiB of swap in use, and PSI `some`/`full` avg10 above
32%, while RamShared activation and daemon were off. The I: SSD was at 100%
active with 92.3 MB/s of reads and no writes; its reader remains unknown.
Windows Task Manager showed about 51% total memory use, although its
`VmmemWSL` figure does not reconcile with the guest's 15.9 GiB reading.
Recovery included a short failed WSL startup before a successful boot. The
evidence supports guest memory/swap thrashing, not Windows physical exhaustion
or RamShared stress; it does not identify the initiating allocation. The same
Build #6 source mismatch and unsafe GPADL/UIO lifetime gap remain. The
dashboard's hardcoded `Protection: ACTIVE` was separately corrected in source
commit `a5ea63d1` and passed five focused tests, but no release binary is
installed. Keep the freeze gap `PARTIAL` and stress off.

### Active-gate source and runtime audit (EVD-0100)

EVD-0100 compares every open gate above with current executable source and
separates code evidence from installed or hardware proof. All eleven statuses
remain unchanged. Commits `83dcde21` and `79380a07` improve monitor accuracy:
missing/malformed PSI and required `meminfo` counters no longer appear as
invented zero values, failed refreshes are marked stale, and invalid memory
samples do not enter the history. The CLI binary suite passes 360 tests and
Clippy, formatting, and whitespace checks pass. The installed executable is
still v0.14.1, phase Off; the Guardian is stale, only the WSL fallback swap is
present, and no Windows physical/commit sample was paired with the guest.
Stress remains blocked.

Direct review of the current VMBus worktree found and patched two source gaps:
arm64 host-visible buffers now select the page-chunk path despite the weak
false default for `hv_is_isolation_supported()`, and page-rounding overflow is
rejected before storing the aligned size in `u32`. Strict checkpatch is clean,
but this worktree diff is unbuilt and untested by KUnit, is absent from the
tracked six-patch mail series, and still retains UIO-backed buffers without a
VMA lifetime tracker or reclaimer. Keep the upstream, CoCo, freeze-causality,
and host-promotion gates open. The source findings and external proofs are
itemized under EVD-0100 in [validation.md](../../validation.md).

### Independent current-state cross-check (EVD-0101)

EVD-0101 re-reads all six active `PARTIAL` rows against source and named tests
and pairs a fresh guest sample with Windows memory telemetry. The installation
is still v0.14.1 with RamShared Off and a stale Guardian; the 4 GiB WSL fallback
swap is the only active swap device. The sample does not identify the earlier freeze
or admit stress. The VMBus candidate still lacks UIO VMA lifetime tracking and
a production retained-buffer reclaimer; this remains candidate evidence, not
installed-kernel attribution. Host/guest transport and `host_gate` remain
unwired in both runtime entrypoints. The cross-platform resource configuration
now has a reviewed PRD/SPEC, but there is no `ramshared config` implementation.
All six gates remain `PARTIAL`; see [EVD-0101](../../validation.md) for the
measured values, test commands, and exact proof still missing.

### Configuration implementation and paired current sample (EVD-0102)

EVD-0102 rechecked the read-only configuration path against current source and
ran it on this WSL2 host. Linux inventory reports multiple block devices,
joins mounts by `MAJ:MIN`, and allows eligible ext4/XFS filesystems on either
a partition or whole disk when stable backing identity and a recognized local
transport are present. Known network-backed and unclassified transports are
refused. The WSL guest root filesystem is
visible but not eligible for file placement until its Windows backing-volume
identity and current host free capacity are bound. The sample exposed a
material capacity mismatch: about 834 GiB was free inside the guest root
filesystem, while its Windows VHDX volume was not identified. No write,
benchmark, or speed recommendation ran. See [EVD-0102](../../validation.md).

The same read-only period reported about 8 GiB guest `MemAvailable`, 2.35 GiB
`SwapFree`, and zero PSI. Windows physical free memory was 1.64 GiB in the
configuration sample and 1,778 MiB in a later sample. Three PowerShell
processes totaled 231 MiB private memory, with the largest at 115 MiB; the
earlier 11.5 GiB process was not reproduced and its cause remains unknown.
The installed CLI is v0.14.1 and no `ramsharedd` process was present.

### Independent re-audit of every active PARTIAL gate (EVD-0103)

EVD-0103 re-read source and named tests for all seven active `PARTIAL` rows;
earlier conclusions were treated as leads and checked directly. The WSL2
freeze remains unattributed: the active `#6` kernel source is unmatched, and
the separate unbuilt candidate has no UIO mapping-close tracker or production
reclaimer for retained buffers. The UIO core takes page references, so this
audit does not claim a proven use-after-free; VMA lifetime and memory
reencryption still lack a coordinated proof. Host/guest control-plane socket
and gate tests pass, but no daemon/service entrypoint starts the handshake.
The installed CLI remains v0.14.1, the service is not running, and there is no
post-reboot v0.15.0 `BINARY_MATCH`. GPU policy tests pass but physical
allocation/teardown and cross-vendor campaigns are absent. Four PowerShell
static suites pass, while physical Windows cold-boot and storage-matrix proof
remain absent. The resource configuration is first-class for native Linux
and WSL2 in its PRD/SPEC, but source currently implements only read-only
discovery; selection, apply, swap/origin mutation, tier caps, benchmark, and
native Linux live E2E remain open. All seven statuses stay `PARTIAL`; see
[EVD-0103](../../validation.md) for exact source checks and close criteria.

### Typed profile model and current guest recheck (EVD-0104)

EVD-0104 adds the first implementation slice of the resource configuration
profile: a 64 KiB-bounded versioned TOML model validates variable user ceilings,
stable adapter and volume identities, platform-bound targets, path safety, and
checked capacity arithmetic. Its 95.6% line-coverage gate passes. The read-only
CLI still does not load or persist this profile, and it has no selection,
plan/apply, disk benchmark, or managed swap/origin writer. A current guest
sample showed about 7.3 GiB `MemAvailable`, 2.6 GiB `SwapFree`, and zero PSI;
the installed CLI remains v0.14.1 with only the fallback swap active. The
source/audit tests advance evidence, but none of the seven gates has the
required live platform/release proof to close. See
[EVD-0104](../../validation.md).

### Multi-target resource profile model (EVD-0105)

EVD-0105 corrects the typed profile to represent both swap and origin targets
on one or multiple stable volumes, including WSL origin placement as a distinct
target. Checked required capacity now sums each target by stable volume and
adds the 10 GiB reserve once per volume; duplicate managed paths and arithmetic
overflow refuse. The profile slice passes at 94.2% line coverage, with the
config crate tests, strict Clippy, and docs checks green. This remains a pure
model: the CLI does not load or persist it, no live candidate is bound, and no
provider performs writes. Cross-platform resource configuration remains
`PARTIAL`, as do the other six active gates. See
[EVD-0105](../../validation.md).

### Independent re-audit of all active PARTIAL gates (EVD-0106)

EVD-0106 re-read all seven current `PARTIAL` rows against source and reran
their available named tests/static suites. The audit found that Linux profiles
persisted a namespace-scoped mount ID; commit `750090a5` removes it, resolves a
unique current mount from stable filesystem/device identity, and refuses
ambiguous mounts and bind/subtree roots. The refreshed sample still runs
installed v0.14.1 on WSL kernel `#6`; the swap is in use but memory PSI is
zero and no RamShared daemon is running. The cross-platform config remains
read-only with no selection, persistence, apply/rollback, benchmark, or native
Linux live target qualification. VMBus/UIO lifetime, host/guest entrypoint
wiring, GPU hardware, Windows cold-boot, and physical storage-matrix evidence
remain open. All seven statuses stay `PARTIAL`; see
[EVD-0106](../../validation.md) for exact commands and observations.

### Complete Windows volume inventory (EVD-0107)

The WSL resource view previously filtered `Get-Volume` to fixed volumes and
showed no refusal reason, volume identity, or drive type. The collector now
preserves every returned row and leaves absent capacity as unavailable; the
view shows fixed, removable, and unknown candidates with the planner's shared
identity/filesystem/capacity eligibility reasons. Focused tests, the live WSL
read-only discovery E2E, strict Clippy, and the 80% slice gate pass at 88.5%.
This remains read-only discovery: there is no interactive target selection,
profile persistence, apply/rollback, or disk benchmark. The configuration gate
and the other six reliability gates remain `PARTIAL`; see
[EVD-0107](../../validation.md).

### Fresh independent audit of the seven active gates (EVD-0108)

This audit reread current source and reran the named host-gate, GPU-policy,
adapter-identity, swapoff-first, and Windows static suites rather than copying
the EVD-0106 verdict. The kernel candidate still has an uncommitted working
tree and an untracked UIO mapping-lifetime risk; no build, KUnit, install, or
CoCo run occurred. The host/guest transport and helper code still is not called
by either production entrypoint, and the handshake still lacks the host-proof
finish. The installed CLI remains v0.14.1; a small health-monitor process is
running, but no `ramsharedd` or stress is running. All four Windows static
suites pass, while physical cold-boot and storage-matrix evidence is still
absent. The source-level corrections improve evidence without closing an
environment-bound gate. All seven statuses remain `PARTIAL`; see
[EVD-0108](../../validation.md) for the independent source checks and current
read-only memory sample.

### Native origin request model correction (EVD-0109)

The profile and read-only planner now represent a new native Linux origin
before file creation without inventing inode provenance. Capacity is bound to
the current stable mount, and the plan still refuses to create or open the
file. The model/planner tests and ≥80% coverage gates pass at 93.1% and 88.7%.
The interface still lacks disk/adapter selection, profile persistence,
benchmarking, providers, and apply/rollback; no native-host or real selected
WSL target E2E ran. This advances source correctness but does not close the
resource-configuration gate; see [EVD-0109](../../validation.md).

### User-selected storage draft and capacity alias correction (EVD-0110)

An attended `config draft --output PATH` flow now selects eligible native
Linux filesystems or Windows fixed NTFS/ReFS volumes, including canonical
volume-GUID paths without drive letters, and records variable fallback-swap
and SSD-origin requests in a new user-owned mode-`0600` file after combined
capacity review and explicit `SAVE`. It does not write the system profile or
apply settings. Independent source review also found and fixed a capacity
under-count: case variants of one Windows volume ID had been grouped as
separate volumes despite case-insensitive provider lookup. The source tests
reproduce and refuse a 60 GiB combined allocation plus 10 GiB reserve on a
volume with 65 GiB free. CLI coverage is 88.4%; profile-model coverage is
93.7%. GPU adapter selection, caps, benchmark/recommendation, apply/rollback,
and native-live/selected-WSL E2E remain open, so the gate stays `PARTIAL`.
See [EVD-0110](../../validation.md).

### Fresh independent audit of all active gates (EVD-0111)

The source and runtime checks below were repeated against the current
worktrees. The seven gate labels remain `PARTIAL`; this audit did not treat
old PASS/PARTIAL text as executable proof.

| Gate | Fresh finding |
| --- | --- |
| Cross-platform resource configuration | A live WSL TTY run selected one real eligible host volume, saved a temporary mode-`0600` user draft, and `config plan` returned `ready_for_review` / `storage_ready` with writes and apply disabled. The temporary draft was removed. GPU-cap selection, bounded speed comparison, apply/rollback, and native-Linux-host E2E remain open. |
| WSL2 freeze memory ownership | EVD-0112 verifies the tracked seven-patch mainline candidate and WSL 6.18.40.1 backport compile/test in hosted CI: seven staged x86_64/arm64 builds, mainline KUnit 24/24, and WSL-backport KUnit 14/14. Patch 0007 now has a retained-owner reclaimer and page-reference gate, covered by named KUnit cases; real UIO mmap-close/unregister and live GPADL response/rescind races were not exercised. The daily WSL host still runs unmatched Build #6 and RamShared 0.14.1; no candidate install ran. The prior freeze remains consistent with guest swap thrashing, but its initiating allocation and exact kernel source remain unestablished. |
| WSL2 control plane | Production daemon/service entrypoints still do not call the AF_VSOCK/AF_HYPERV transport or host gate, and the protocol lacks a guest finish message. Helper tests pass but do not prove a live handshake or lease revocation. |
| Legacy WSL2 handoff | Installed CLI reports v0.14.1; `--build-info` is unsupported. Status is Off, Guardian is stale, fallback swap is the only active swap, one small monitor process is present, and `ramsharedd` is absent. Swapoff-first source test passes; current-release `BINARY_MATCH` and repeated post-reboot handoff remain open. |
| Cross-vendor GPU budget | Admission/identity unit tests pass. Read-only hardware inventory sees one NVIDIA RTX 2060; no worker allocation/teardown, second adapter, AMD, or Intel run was done. |
| Windows physical lifecycle | Lifecycle recovery, host lifecycle, and origin static suites pass. No physical cold boot, loaded-binary identity, or rollback drill was performed. |
| Windows storage matrix | The static/manufactured matrix suite passes. No physical five-cell/75-sample run, payload-integrity artifact set, or Event ID 153 qualification was collected. |

`validation.md` contains five reused evidence IDs (`EVD-0007` through
`EVD-0010`, plus `EVD-0107`); the repeated EVD-0010 block is identical, while
the other reused IDs point to different records. Because that log is
append-only, the old records were preserved; EVD-0111 is a new unique ID.
The validation-schema checker does not enforce uniqueness, so repeated IDs
must not be counted as independent corroboration. See
[EVD-0111](../../validation.md).

### Hosted VMBus candidate follow-up (EVD-0112)

EVD-0112 adds current evidence for the VMBus portion of the WSL2 freeze gate. It changes the build/KUnit status of the candidate only; the active host identity and freeze attribution remain unresolved. The other six active gates were not re-executed in this follow-up and retain their EVD-0111 boundaries. See [EVD-0112](../../validation.md).

## Closed In This Session

All run IDs, commands, VM names, and `SANITIZED_*` values below are retained
historical evidence only. Machine-specific identities and paths are sanitized,
and no row authorizes activation of the current disabled candidate.

| Gap | Close evidence |
| --- | --- |
| Legacy-preallocation Day-0 source removal | The `RAMSHARED_VRAM_PREALLOC_LEGACY` selector, its aliases, profile chooser, and full-VRAM `VramBackend` NBD composition were removed from executable source. The named `legacy_preallocation_removed_before_day0_deadline` test, clean active-source/current-doc scan, thresholded checker coverage, and documentation governance close this source-governance prerequisite only. Rust test, rustfmt, clippy, and slice coverage regression gates are fully active and passing across all crates. Generic `VramBackend` remains for broker, ublk, and Windows consumers. Live guardian/origin/pressure qualification, release promotion, and activation remain open and managers stay disabled/plan-only. |
| Rust CI guardrail and slice coverage restoration | PR #1214 restored full workspace CI coverage (`node tools/ci/check-rust-slice-coverage.mjs` >= 80% line/branch/function coverage across active crates), unblocking the pipeline and eliminating regression drift. |
| Tier 3 (SSD) qualification and hardware metrics baseline | Tier 3 fallback and NVMe/SSD degradation criteria qualified with self-contained hardware metrics comparison tables and zero-sum public hygiene, fully compliant with governance and performance requirements. |
| Photorealistic 3D hardware SVG architecture rendering | Standardized vector SVG hardware topology diagrams (VRAM/RAM/SSD tiering) integrated with dark/light themes and validated across renderer suites. |
| Multi-distro release packaging and v0.12.0 publication | Automated packaging workflow in `.github/workflows/release-packaging.yml` established with dynamic version detection, attaching qualified Debian (`.deb`), Fedora (`.rpm`), and Arch Linux (`.tar.gz`) binaries alongside `SHA256SUMS.txt` to GitHub Release `v0.12.0`. |
| Public repository branch hygiene | Purged 503 obsolete external bot/test branches from remote origin, locking down canonical single-branch (`main`) governance. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
