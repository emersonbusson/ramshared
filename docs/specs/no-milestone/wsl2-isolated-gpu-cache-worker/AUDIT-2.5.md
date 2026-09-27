# AUDIT-2.5 — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 2.5 · SPEC: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| High | DT-1 | `SOCK_STREAM` requires manual byte-level framing and buffer accumulation, risking partial frame reads on IPC stall. | **Source resolved:** fixed 32-byte header and bounded `payload_len` are validated and reassembled. Reads use absolute deadlines; a mutation frame that cannot be queued in one nonblocking send revokes the cache. This is one FIFO stream, not separate control/data lanes. |
| High | DT-2 | Per-syscall timeouts could be extended by a worker that trickles response bytes, and a cache failure left the socket open. | **Source resolved:** one absolute monotonic deadline now covers request writes, headers, and payloads; expiry marks the cache unavailable and shuts down both socket directions. Named slow-trickle regression passes. This does not cancel a driver call or establish a physical no-hang guarantee. |
| Critical | DT-3 | Overcommitting GPU memory could cause Windows desktop compositor crash or game crashes on the host. | **Source policy implemented:** reserve and current headroom constrain cache admission. This does not prove compositor safety; live host GPU qualification remains open. |
| High | DT-5 | Parent daemon termination or a driver call may leave worker exit and GPU allocation release unconfirmed. | Parent requests `SIGTERM` on parent death; normal teardown is bounded to 5s graceful plus 500ms after `SIGKILL`, then hands the child handle to a background reaper. Signal delivery is not proof of process exit or VRAM release. |
| Medium | Observability | Telemetry read races could read partial JSON while daemon writes status. | Atomic write via temporary file rename (`/run/ramshared/wsl2-cache-status.json.tmp` -> `.json`). |
| Medium | PRD §7 | Frame header `msg_type` lists values 1–7 but implementation defines 1–10 (HandshakeReq=9, HandshakeResp=10, HeartbeatResp=8 missing from PRD). | PRD §7 updated to include all 10 message types. |
| Medium | PRD RF-2 | Earlier text claimed an independent control lane that could not be starved, but data, heartbeat, and disable frames share one FIFO socket and worker loop. | PRD and SPEC now document one ordered stream and make no independent scheduling guarantee for control operations. |
| Medium | Kahneman #16 | Test name `isolated_worker_read_timeout_falls_back_to_origin` does not match any existing test. Actual: `read_timeout_falls_back_cleanly`. | SPEC Kahneman map updated to reference the real test name. |
| Low | Test matrix | Missing `worker_teardown_is_idempotent_and_bounded` and `worker_evicts_coldest_chunk_on_pressure` rows. | SPEC test matrix updated. |
| Low | PRD §8 | Arguments listed as `--target-kib` / `--reserve-floor-kib` but code uses `--target-bytes` / `--reserve-floor`. | PRD §8 updated to match actual CLI flags. |
| Low | PRD §8 | Binary listed as `/usr/local/bin/ramshared-gpu-worker` but implementation uses re-exec `/proc/self/exe __gpu_worker`. | PRD §8 updated to reflect re-exec model. |
| Low | IMPL.md | Coverage numbers outdated after gap fixes (86.0%/90.1% → 86.7%/88.7%). | IMPL.md updated. |
| High | RF-6/RF-7, DT-6/DT-7 | The isolated cache worker chooses CUDA/Vulkan independently from the daemon's WDDM budget reader. Identity metadata is published, but WDDM headroom does not constrain cache allocations; choosing DXG by enumeration order could apply the wrong adapter's budget. | Select DXG by the active provider's normalized LUID only; intersect allocator and WDDM available bytes; reject stale/query-failed samples after attaching the guard. Retain driver-only admission when DXG is unavailable or the active provider exposes no LUID. Add deterministic same-adapter, mismatch, stale, and error-path tests. |
| High | RF-8 / DT-8 | Worker selected CUDA ordinal 0 and `VulkanProvider::open(0)` preferred the first discrete device, so it could ignore a larger safe adapter and could not prove the Vulkan ordinal selected. | Enumerate candidates from both APIs, rank by the actual reserve-adjusted and WDDM-constrained target, open Vulkan by exact ordinal, and revalidate the selected identity and budget before starting. Add pure policy tests; retain physical adapter exercise as an environment gate. |
| High | RF-2 / RF-3 | A sequence of partial reads, each shorter than the per-call timeout, could extend total parent wait; failure also left the worker peer socket open. | **Source resolved:** read/heartbeat request and response paths share an absolute monotonic deadline; failures shut down the socket. `trickled_response_cannot_extend_the_absolute_read_deadline` reproduced the 251 ms wait under the old 30 ms per-call timeout. |
| High | RF-2 / RF-3 | Blocking mutation writes could hold the origin-serving thread through socket backpressure; oversized mutations could also exceed the worker frame limit. | **Source resolved:** mutation frames are sent with one nonblocking write and are capped at 64 KiB. A partial frame or oversized mutation closes the socket and revokes cache use. `saturated_mutation_socket_does_not_block_origin_thread`, `oversize_mutation_disables_cache_without_touching_ipc`, and `oversized_mutation_disables_cache_before_worker_frame_is_sent` cover the refusal paths. |

## Open questions

1. *GPU Adapter selection:* Which adapter should the worker use on multi-GPU systems?
   - *Resolution:* Select the adapter with the largest fresh safe cache target after the configured reserve and exact-LUID WDDM intersection. Ties prefer CUDA, then lower ordinal, then normalized adapter key. This policy is source-tested; physical multi-adapter qualification remains open.
2. *WDDM Driver Reset recovery:* Can the worker restart automatically after a GPU reset, or should it stay in fail-closed origin-only mode until next daemon lifecycle?
   - *Resolution:* Stay in fail-closed origin-only mode for the remainder of the session to prevent thrashing during unstable host conditions; re-attempt upon explicit restart or service reload.

## Verdict

**GO for source-level budget correlation, deterministic adapter selection, and bounded parent IPC** after the named policy, deadline, saturation, and frame-limit regressions pass. This is not a no-panic or physical driver qualification. Physical WSL GPU validation remains environment-bound and is not qualified by mocks; do not claim cross-vendor product support until the exact worker is exercised with fresh allocator/WDDM samples and multi-adapter selection on supported hardware.
