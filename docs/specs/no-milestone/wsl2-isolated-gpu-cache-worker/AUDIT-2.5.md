# AUDIT-2.5 — Process-isolated GPU cache worker for WSL2 origin swap

> SSDV3 Step 2.5 · SPEC: docs/specs/no-milestone/wsl2-isolated-gpu-cache-worker/SPEC.md

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| High | DT-1 | `SOCK_STREAM` would require manual byte-level framing and buffer accumulation, risking partial frame reads on IPC stall. | DT-1 specifies `SOCK_SEQPACKET` or strict 32-byte header with length framing to ensure atomic message delivery. |
| High | DT-2 | Unbounded worker response times could stall kernel swap daemon in D-state during severe GPU bus congestion. | Hard 50ms read timeout with immediate fallback to SSD origin enforced; writes are strictly non-blocking. |
| Critical | DT-3 | Overcommitting GPU memory could cause Windows desktop compositor crash or game crashes on the host. | Strict mathematical headroom floor enforced: `reserve_floor = max(1536 MiB, 20% host VRAM)`. |
| High | DT-5 | Parent daemon termination (e.g. `SIGKILL` or crash) could leave worker process orphaned and holding VRAM allocations. | Child process sets `prctl(PR_SET_PDEATHSIG, SIGTERM)` immediately upon spawn to ensure automatic termination. |
| Medium | Observability | Telemetry read races could read partial JSON while daemon writes status. | Atomic write via temporary file rename (`/run/ramshared/wsl2-cache-status.json.tmp` -> `.json`). |

## Open questions

1. *GPU Adapter selection:* In multi-GPU systems, should the worker default to the primary DirectX adapter with display or the highest-capacity discrete GPU?
   - *Resolution:* Default to primary rendering adapter exposed by `/dev/dxg` with fallback to adapter with largest dedicated VRAM.
2. *WDDM Driver Reset recovery:* Can the worker restart automatically after a GPU reset, or should it stay in fail-closed origin-only mode until next daemon lifecycle?
   - *Resolution:* Stay in fail-closed origin-only mode for the remainder of the session to prevent thrashing during unstable host conditions; re-attempt upon explicit restart or service reload.

## Verdict

**`go`** — The architecture strictly enforces process isolation, non-blocking fail-closed origin fallback, mathematical host reserve preservation, and clean child reaping. The design prevents any possibility of GPU driver instability causing Linux kernel swap hangs.
