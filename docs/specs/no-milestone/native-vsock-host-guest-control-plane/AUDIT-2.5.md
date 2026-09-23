# AUDIT-2.5 — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 2.5 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Medium | DT-2 | Payload encoding says "JSON for control messages ≤ 4KB, raw bytes for OriginManifest ≤ 64KB" but does not specify what happens if a control message exceeds 4KB. Ambiguous boundary. | SPEC DT-2 updated: control messages use serde JSON with hard cap 4KB; messages exceeding cap are rejected at deserialization with `PayloadTooLarge`. |
| Medium | DT-3 | HMAC secret storage in `winsvc.toml` and `config.toml` — plaintext secret at rest. SPEC says "HMAC secret never transmitted in plaintext" but does not address at-rest protection. | SPEC DT-3 updated: HMAC secret is file-permission-restricted (0600 root/SYSTEM) and may be sourced from environment variable override. No secret in logs or status JSON. |
| Medium | ITEM-3 | `lease_expiry_revokes_origin_authority` test references mid-I/O behavior but SPEC does not define the atomicity of a write in flight when lease expires. | SPEC Atomicity frontier updated: in-flight writes complete or abort cleanly; lease expiry sets `origin_authoritative = false` before next I/O dispatch; current I/O is not interrupted mid-write. |
| Low | Kahneman #15 | `vsock_connect_timeout_falls_back` abort condition says "Connect exceeds 5s without fallback" but the5s timeout value is not stated in any DT. | SPEC DT-1 updated: connect timeout is 5s, after which DT-7 file-based fallback activates. |
| Low | DT-4 | `wsl.exe --mount` invocation does not specify how the service handles concurrent attach/detach requests. | SPEC DT-4 updated: VHDX operations are serialized through a mutex; concurrent requests are queued, not parallel. |
| Low | Test matrix | Missing `vsock_disconnect_detected_within_interval` row (listed in vsock.rs required tests but not in matrix). | SPEC test matrix updated. |
| Low | Files CREATE | `crates/ramshared-ipc/src/vsock.rs` lists `VsockEndpoint` struct but does not define its fields. | SPEC updated: `VsockEndpoint` wraps platform socket with `cid`, `port`, `guid` metadata for diagnostics. |

## Open questions

1. *WSL2 kernel config variability:* Not all WSL2 kernels ship `CONFIG_VSOCKETS`. What is the minimum kernel version for Day-0?
   - *Resolution:* Day-0 targets WSL2 kernel ≥ 5.10 (standard Microsoft build). Fallback (DT-7) covers kernels without vsock. Shadow comparison validates gate logic regardless of transport.
2. *HMAC secret rotation:* How is the shared secret rotated without downtime?
   - *Resolution:* Out of scope for Day-0. Secret is provisioned at install time. Rotation requires restart of both services. Documented in operational runbook.

## Verdict

**`go`** — All hard no-go criteria pass: Kahneman present on all 4 critical items (#15, #13, #17, #16), Day-0 clean (single vsock path with documented file fallback exception), test matrix complete with real test names (17 rows), privilege boundary covered (HMAC + GUID allowlisting), platform gate correct (cargo for userspace, no checkpatch/WDK), no shared-hardware overcommit, no unbounded DMA. The7 findings above are documentation precision issues, not structural gaps. All corrections applied to SPEC.md in this revision.
