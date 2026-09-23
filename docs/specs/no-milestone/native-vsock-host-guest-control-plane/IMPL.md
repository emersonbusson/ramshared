# IMPL — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Status

**partial (ITEM-1, ITEM-2 implemented)** · cover pending (slice gate not yet run) · E2E env-bound (requires Windows host + WSL2 with AF_HYPERV/AF_VSOCK) · BINARY_MATCH pending

ITEM-3 through ITEM-6 (gate absorption, winsvc listener, VHDX lifecycle, observability) are not yet implemented. IMPL is partial pending those items.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-ipc/src/lib.rs` | ITEM-1 / RF-2, DT-2 | Created shared protocol crate: `VsockFrameHeader` (24-byte binary framing, magic `0x52414D53`, version 3, `correlation_id`), 21 message types, serde JSON control messages with 4KB cap, raw manifest encoding, HMAC-SHA256 (manual implementation), version negotiation. 10 unit tests. |
| `crates/ramshared-ipc/src/vsock.rs` | ITEM-2 / RF-1, DT-1 | Created vsock transport abstraction: `connect_vsock` (AF_VSOCK, 5s bounded timeout), `listen_hyperv` (env-bound), `MockVsockStream` (UnixStream-backed for testing), `vsock_available` detection. 5 unit tests covering connect timeout, read timeout, disconnect detection. |
| `crates/ramshared-ipc/Cargo.toml` | ITEM-1 | Crate manifest with `serde`, `serde_json`, `sha2` dependencies. |
| `Cargo.toml` (workspace) | ITEM-1 | Added `crates/ramshared-ipc` to workspace members. |

## Validation Results

1. **Unit & Protocol Tests (`ramshared-ipc`)**:
   - `tests::frame_round_trip` — **PASS**
   - `tests::frame_rejects_bad_magic` — **PASS**
   - `tests::frame_rejects_oversized_payload` — **PASS**
   - `tests::frame_rejects_unknown_message_type` — **PASS**
   - `tests::version_negotiation_selects_highest_mutual` — **PASS**
   - `tests::handshake_hmac_validates` — **PASS**
   - `tests::control_round_trip` — **PASS**
   - `tests::control_payload_cap_enforced` — **PASS**
   - `tests::manifest_round_trip` — **PASS**
   - `tests::manifest_rejects_short_payload` — **PASS**
   - `tests::lib_exports_and_constants_are_consistent` — **PASS**
   - `vsock::tests::vsock_connect_timeout_falls_back` — **PASS**
   - `vsock::tests::vsock_stream_read_timeout_is_bounded` — **PASS**
   - `vsock::tests::vsock_disconnect_detected_within_interval` — **PASS**
   - `vsock::tests::vsock_available_detects_platform` — **PASS**
   - `vsock::tests::vsock_error_display_is_informative` — **PASS**
   - Suite: **16 passed, 0 failed**.

2. **Code Quality & Lints**:
   - `cargo fmt --all --check` — **PASS**
   - `cargo clippy -p ramshared-ipc --all-targets -- -D warnings` — **PASS** (0 warnings, 0 errors)

3. **Kahneman Map Disciplines Addressed**:
   - **#15 (Bounded connect):** `vsock_connect_timeout_falls_back` verifies connect completes within bounded window.
   - **#16 (Read timeout bounded):** `vsock_stream_read_timeout_is_bounded` verifies read timeout fires within 500ms.
   - **#15 (Disconnect detection):** `vsock_disconnect_detected_within_interval` verifies EOF/error on socket close within 500ms.

## Open Evidence (Env-Bound)

- **Live vsock RTT measurement:** Requires physical Windows host with WSL2 and paired AF_HYPERV/AF_VSOCK connection. Env-bound for Day-0.
- **BINARY_MATCH:** Requires deployed daemon. Pending ITEM-3 through ITEM-6.
- **Shadow comparison with `ramshared-host-gate.sh`:** Requires ITEM-3 (gate absorption). Pending.

## Remaining ITEMs

| ITEM | Description | Status |
| --- | --- | --- |
| ITEM-3 | Extend `ramshared-wsl2d` — AF_VSOCK client, lease heartbeat, fail-closed, absorb `host_gate.rs` | Not started |
| ITEM-4 | Extend `ramshared-winsvc` — AF_HYPERV listener, heartbeat deadline, lease management | Not started |
| ITEM-5 | Extend `ramshared-winsvc` — VHDX lifecycle, absorb `Manage-RamSharedOrigin.ps1` | Not started |
| ITEM-6 | Observability — `tracing`/`EventWrite`, status JSON fields | Not started |
