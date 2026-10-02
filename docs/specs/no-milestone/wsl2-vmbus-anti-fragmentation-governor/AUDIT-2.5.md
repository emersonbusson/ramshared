# AUDIT-2.5 — wsl2-vmbus-anti-fragmentation-governor

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Low | §3 DT-2 | Threshold of 8 chunks might be sensitive if memory is pre-fragmented at start of test. | Verify order-7 chunks before initiating ramp; if $<8$ at start, log compaction warning before halting. |
| Low | §7 | Parsing `/proc/buddyinfo` on non-WSL2 environments might fail if format differs. | Gate buddyinfo order-7 check behind `is_wsl2()`. |
| Medium | §6 / RF-6 | The current Tier 3 option is coupled to full-cascade readiness and physical GPU cache evidence; that prevents storage-only testing on hosts without a supported GPU/cache worker. | Add an explicit Tier 3-only path that still requires a live storage-backed swap, kernel-fault baseline, memory floor, PSI limit, and watchdog, but skips all GPU/cache admission and reporting. |
| High | DT-6 | The full-profile budget probe is still NVIDIA-specific, while CUDA, Vulkan, and DXG select or measure devices through different interfaces. Vulkan's current `mem_info()` counts only this provider's allocations when external memory-budget support is absent, and does not identify the selected adapter for telemetry correlation. | Keep generic full-cascade hardware claims open. Before declaring all-GPU support, add a shared adapter identity and budget contract, implement WDDM/DXG and Vulkan `VK_EXT_memory_budget` paths, make unknown/external usage fail closed, and test mismatched/multiple adapters. |

## Open questions

- None. The root cause is empirically proven via the kernel log and buddy allocator counters.

## Verdict

**Disposition:** `go` for the bounded Tier 3-only stress slice after its named tests pass. `NO-GO` for a claim that full-cascade GPU budget selection supports all vendors until the High finding is closed with adapter-bound budget evidence.
