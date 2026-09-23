# AUDIT-2.5 — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 2.5 · SPEC: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| High | DT-1 | `SOCK_STREAM` would require manual byte-level framing and buffer accumulation, risking partial frame reads on IPC stall. | **Resolved:** DT-1 uses `SOCK_STREAM` with strict 32-byte header + `payload_len` framing. Full frame assembled before single `write()` dispatch prevents partial headers. `SOCK_SEQPACKET` is unstable in Rust std. |
| High | DT-2 | Unbounded worker response times could stall kernel swap daemon in D-state during severe GPU bus congestion. | **Resolved:** Hard 50ms read timeout with immediate fallback to SSD origin enforced; writes use single-write non-blocking dispatch. Handshake uses separate 5s timeout for GPU context init. `WouldBlock` with zero bytes = `Skipped`; partial write = permanent fail-closed. |
| Critical | DT-3 | Overcommitting GPU memory could cause Windows desktop compositor crash or game crashes on the host. | Strict mathematical headroom floor enforced: `reserve_floor = max(1536 MiB, 20% host VRAM)`. |
| High | DT-5 | Parent daemon termination (e.g. `SIGKILL` or crash) could leave worker process orphaned and holding VRAM allocations. | Child process sets `prctl(PR_SET_PDEATHSIG, SIGTERM)` immediately upon spawn to ensure automatic termination. |
| Medium | Observability | Telemetry read races could read partial JSON while daemon writes status. | Atomic write via temporary file rename (`/run/ramshared/wsl2-cache-status.json.tmp` -> `.json`). |
| Medium | PRD §7 | Frame header `msg_type` lists values 1–7 but implementation defines 1–10 (HandshakeReq=9, HandshakeResp=10, HeartbeatResp=8 missing from PRD). | PRD §7 updated to include all 10 message types. |
| Medium | PRD RF-2 | States "two distinct lanes: Data lane / Control lane" but implementation multiplexes all messages over a single socket with type-based dispatch. | PRD RF-2 reworded to "logical lanes over a single multiplexed socket". |
| Medium | Kahneman #16 | Test name `isolated_worker_read_timeout_falls_back_to_origin` does not match any existing test. Actual: `read_timeout_falls_back_cleanly`. | SPEC Kahneman map updated to reference the real test name. |
| Low | Test matrix | Missing `worker_teardown_is_idempotent_and_bounded` and `worker_evicts_coldest_chunk_on_pressure` rows. | SPEC test matrix updated. |
| Low | PRD §8 | Arguments listed as `--target-kib` / `--reserve-floor-kib` but code uses `--target-bytes` / `--reserve-floor`. | PRD §8 updated to match actual CLI flags. |
| Low | PRD §8 | Binary listed as `/usr/local/bin/ramshared-gpu-worker` but implementation uses re-exec `/proc/self/exe __gpu_worker`. | PRD §8 updated to reflect re-exec model. |
| Low | IMPL.md | Coverage numbers outdated after gap fixes (86.0%/90.1% → 86.7%/88.7%). | IMPL.md updated. |

## Open questions

1. *GPU Adapter selection:* In multi-GPU systems, should the worker default to the primary DirectX adapter with display or the highest-capacity discrete GPU?
   - *Resolution:* Default to primary rendering adapter exposed by `/dev/dxg` with fallback to adapter with largest dedicated VRAM.
2. *WDDM Driver Reset recovery:* Can the worker restart automatically after a GPU reset, or should it stay in fail-closed origin-only mode until next daemon lifecycle?
   - *Resolution:* Stay in fail-closed origin-only mode for the remainder of the session to prevent thrashing during unstable host conditions; re-attempt upon explicit restart or service reload.

## Verdict

**`go`** — The architecture strictly enforces process isolation, non-blocking fail-closed origin fallback, mathematical host reserve preservation, and clean child reaping. The design prevents any possibility of GPU driver instability causing Linux kernel swap hangs. All hard no-go criteria pass: Kahneman present on critical steps, Day-0 clean, test matrix real, privilege boundary covered, platform gate correct, host floor calculated, DMA bounded. Documentation inconsistencies identified above are non-blocking and corrected in this revision.
