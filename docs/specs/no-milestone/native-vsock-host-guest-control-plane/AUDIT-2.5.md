# AUDIT-2.5 — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 2.5 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Historical first-pass findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Medium | DT-2 | Payload encoding says "JSON for control messages ≤ 4KB, raw bytes for OriginManifest ≤ 64KB" but does not specify what happens if a control message exceeds 4KB. Ambiguous boundary. | SPEC DT-2 updated: control messages use serde JSON with hard cap 4KB; messages exceeding cap are rejected at deserialization with `PayloadTooLarge`. |
| Medium | DT-3 | HMAC secret storage in `winsvc.toml` and `config.toml` — plaintext secret at rest. SPEC says "HMAC secret never transmitted in plaintext" but does not address at-rest protection. | SPEC DT-3 updated: HMAC secret is file-permission-restricted (0600 root/SYSTEM) and may be sourced from environment variable override. No secret in logs or status JSON. |
| Medium | ITEM-3 | `lease_expiry_revokes_origin_authority` test references mid-I/O behavior but SPEC does not define the atomicity of a write in flight when lease expires. | SPEC Atomicity frontier updated: in-flight writes complete or abort cleanly; lease expiry sets `origin_authoritative = false` before next I/O dispatch; current I/O is not interrupted mid-write. |
| Low | Kahneman #15 | Historical finding referenced the removed `vsock_connect_timeout_falls_back` test and treated an unimplemented file fallback as available. | Superseded by the 2026-09-27 reassessment: the current test verifies bounded connect; no file heartbeat fallback is implemented. |
| Low | DT-4 | `wsl.exe --mount` invocation does not specify how the service handles concurrent attach/detach requests. | SPEC DT-4 updated: VHDX operations are serialized through a mutex; concurrent requests are queued, not parallel. |
| Low | Test matrix | Missing `vsock_disconnect_detected_within_interval` row (listed in vsock.rs required tests but not in matrix). | SPEC test matrix updated. |
| Low | Files CREATE | `crates/ramshared-ipc/src/vsock.rs` lists `VsockEndpoint` struct but does not define its fields. | SPEC updated: `VsockEndpoint` wraps platform socket with `cid`, `port`, `guid` metadata for diagnostics. |

## Open questions

1. *WSL2 kernel config variability:* Not all WSL2 kernels ship `CONFIG_VSOCKETS`. What is the minimum kernel version for Day-0?
   - *Resolution:* The guest requires `CONFIG_VSOCKETS` and `CONFIG_HYPERV_VSOCKETS`. No heartbeat fallback is implemented; a missing transport must fail closed until a separately specified fallback is available. Shadow comparison validates gate logic only, not transport behavior.
2. *HMAC secret rotation:* How is the shared secret rotated without downtime?
   - *Resolution:* Out of scope for Day-0. Secret is provisioned at install time. Rotation requires restart of both services. Documented in operational runbook.

## Reassessment — 2026-09-27

The original `go` conclusion is withdrawn. It relied on an unsupported claim that the service GUID restricts a connection to the paired WSL VM and on an assumed file-heartbeat fallback. Microsoft documents that a zero VM ID listener accepts connections from all partitions; therefore the GUID is a service endpoint, not peer authentication. The SPEC now requires HMAC authentication before any manifest or lease authority, and states that no fallback is implemented.

The transport source is now present: Linux AF_VSOCK connect uses nonblocking `connect`, `poll`, `SO_ERROR`, and a five-second maximum; Windows AF_HYPERV bind/listen/accept is implemented with a bounded nonblocking accept. Linux tests and a Windows-target type-check pass. These checks do not exercise a real host/guest connection.

**Verdict: `no-go` for product activation or release qualification.** `ramshared-winsvc` and `ramsharedd` do not use the new transport, the HMAC handshake is not wired to manifest/lease exchange, disconnect does not revoke cache authority, and there is no Windows↔WSL2 runtime evidence, `BINARY_MATCH`, or measured RTT. The SPEC's planned control-plane behavior remains open despite the completed library adapter source.
