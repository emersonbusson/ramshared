# IMPL — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Status

**PARTIAL (protocol, gate, and transport source; product path not wired)** · Linux slice coverage and Windows-target type-check/Clippy were re-run on 2026-09-27 · live E2E and `BINARY_MATCH` are pending. Linux AF_VSOCK connect now uses nonblocking connect, `poll`, `SO_ERROR`, and a hard five-second ceiling; Windows AF_HYPERV bind/listen/accept is implemented with bounded nonblocking accept and RAII socket cleanup. Neither daemon starts this listener/client, the HMAC handshake is not composed with lease or manifest delivery, and disconnect does not yet revoke cache authority. The prior September 23 audit changed a VHDX command timeout; that separate local test does not qualify a Windows host run.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-ipc/src/lib.rs` | ITEM-1 / RF-2, DT-2 | Shared protocol crate: `VsockFrameHeader` (24-byte binary framing, magic `0x52414D53`, version 3, `correlation_id`), 21 message types, serde JSON control messages with 4KB cap, raw manifest encoding, HMAC-SHA256 (manual implementation using `sha2`), version negotiation. 11 unit tests. |
| `crates/ramshared-ipc/src/vsock.rs` | ITEM-2 / RF-1, DT-1 | Functional source adapters: Linux AF_VSOCK nonblocking connect with caller timeout capped at 5s and `SO_ERROR` result checks; Windows AF_HYPERV service listener with canonical GUID conversion, wildcard VM ID, nonblocking accept deadline, typed errors, and RAII ownership. `vsock_available` probes socket creation. Hermetic tests cover deadline handling, accept timeout/error, GUID conversion, stream behavior, and platform refusal. Live Windows/WSL socket exchange remains untested. |
| `crates/ramshared-wsl2d/src/host_gate.rs` | ITEM-3 / RF-5, DT-6 | Absorbed gate logic from `ramshared-host-gate.sh`: origin manifest validation (SHA-256, size bounds, JSON fields), guardian health check (stale/unhealthy rejection), safe-mode gate (foreign boot_id rejection), lease minting (all-gates-required), lease expiry detection. 14 unit tests including shadow comparison. |
| `crates/ramshared-winsvc/src/control_plane.rs` | ITEM-4, ITEM-5, ITEM-6 / RF-1, RF-3, RF-4, DT-4, DT-5 | HeartbeatTracker (guest-initiated, 3× interval lease timeout), VhdxLifecycle (`wsl.exe --mount`/`--unmount` with mutex serialization and 10s timeout), ControlPlaneTelemetry (status JSON fields). 11 unit tests. |
| `crates/ramshared-ipc/Cargo.toml` | ITEM-1 | Crate manifest with `serde`, `serde_json`, `sha2` dependencies. |
| `crates/ramshared-ipc/README.md` | ITEM-1 | Crate README with Scope & Responsibility, Workspace Dependencies, Safety Invariants, Testing sections. |
| `Cargo.toml` (workspace) | ITEM-1 | Added `crates/ramshared-ipc` to workspace members. |
| `ARCHITECTURE.md` | ITEM-6 | Added Layer 6: Host-Guest IPC with `ramshared-ipc` reference. Updated crate count to 16. |

## Validation Results

1. **Transport and Protocol Tests (`ramshared-ipc`)**: 30 passed, 0 failed on Linux.
   - Covers protocol frames and HMAC primitives, Linux connect boundedness, late poller deadline rejection, socket error propagation, five-second cap, bounded accept timeout, accept error propagation, canonical Hyper-V service GUID validation, read timeout, disconnect detection, stream read/write, and platform-specific unsupported behavior.
   - `CARGO_BUILD_JOBS=1 cargo clippy -p ramshared-ipc --all-targets -- -D warnings` — **PASS**.
   - `CARGO_BUILD_JOBS=1 cargo check -p ramshared-ipc --all-targets --target x86_64-pc-windows-gnu` — **PASS**; this type-checks Windows library and test code but does not execute a Windows listener.

2. **Previously recorded Gate Logic Tests (`ramshared-wsl2d`)**: 142 passed, 0 failed
   - `host_gate::tests::validate_origin_manifest_matches_script` — **PASS**
   - `host_gate::tests::validate_origin_manifest_rejects_bad_hash` — **PASS**
   - `host_gate::tests::validate_origin_manifest_rejects_empty` — **PASS**
   - `host_gate::tests::validate_origin_manifest_rejects_oversized` — **PASS**
   - `host_gate::tests::validate_origin_manifest_rejects_missing_fields` — **PASS**
   - `host_gate::tests::check_guardian_health_rejects_stale` — **PASS**
   - `host_gate::tests::check_guardian_health_rejects_unhealthy` — **PASS**
   - `host_gate::tests::check_guardian_health_accepts_fresh` — **PASS**
   - `host_gate::tests::evaluate_safe_mode_refuses_foreign_boot_id` — **PASS**
   - `host_gate::tests::evaluate_safe_mode_allows_matching_boot_id` — **PASS**
   - `host_gate::tests::evaluate_safe_mode_denies_when_safe_mode_active` — **PASS**
   - `host_gate::tests::mint_lease_requires_all_gates` — **PASS**
   - `host_gate::tests::lease_expiry_revokes_origin_authority` — **PASS**
   - `host_gate::tests::host_gate_shadow_comparison` — **PASS**

3. **Control Plane Tests (`ramshared-winsvc`)**: Current workspace run: 212 passed, 0 failed, 1 ignored in the library suite; 4 probe tests passed.
   - `control_plane::tests::heartbeat_deadline_revokes_lease` — **PASS**
   - `control_plane::tests::heartbeat_lease_remaining_counts_down` — **PASS**
   - `control_plane::tests::heartbeat_tracker_lease_remaining_zero_when_no_heartbeat` — **PASS**
   - `control_plane::tests::vhdx_attach_is_idempotent` — **PASS**
   - `control_plane::tests::vhdx_attach_timeout_is_bounded` — **PASS**
   - `control_plane::tests::vhdx_detach_is_bounded` — **PASS**
   - `control_plane::tests::vhdx_attached_partuuids_empty_after_failed_attach` — **PASS**
   - `control_plane::tests::control_plane_telemetry_serializes` — **PASS**
   - `control_plane::tests::control_plane_state_as_str` — **PASS**
   - `control_plane::tests::default_impls_match_new` — **PASS**
   - `control_plane::tests::command_timeout_extension_does_not_panic` — **PASS**

4. **Rust Slice Coverage Gate** (min 80%):
   - `crates/ramshared-ipc/src/lib.rs`: **90.0%** (215/239) — **PASS**
   - `crates/ramshared-ipc/src/vsock.rs`: **85.7%** (330/385) — **PASS** on Linux. Windows-only FFI branches are excluded from this coverage run; they type-check and pass Clippy on the Windows target.
   - `crates/ramshared-wsl2d/src/host_gate.rs`: **95.0%** (247/260) — **PASS**, previous recorded run.
   - `crates/ramshared-winsvc/src/control_plane.rs`: **87.0%** (181/208) — **PASS**.
   - Combined gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-ipc,ramshared-wsl2d,ramshared-winsvc --files crates/ramshared-ipc/src/lib.rs,crates/ramshared-ipc/src/vsock.rs,crates/ramshared-wsl2d/src/host_gate.rs,crates/ramshared-winsvc/src/control_plane.rs --min 80` — **PASS**.

5. **Code Quality & Lints**:
   - `cargo fmt --all --check` — **PASS**
   - `cargo clippy --all-targets -- -D warnings` — **PASS** (0 warnings, 0 errors)
   - `./scripts/docs-check.sh` — **PASS**
   - `cargo clippy --target x86_64-pc-windows-gnu --all-targets -p ramshared-ipc -p ramshared-winsvc -- -D warnings` — **PASS**; Windows-target listener/service code compiles, but it does not execute a live AF_HYPERV listener.

6. **Historical Local Installation & Runtime (not vsock qualification)**:
   - `sudo install -m 0755 target/release/{ramshared,ramsharedd} /usr/local/bin/` — **PASS**
   - `ramshared check --json` — **decision: ready, blockers: 0**
   - `ramshared doctor --json` — **recommendations: 2 (NBD module + bounded preflight)**
   - `ramshared status --json` — **phase: Off, cache_state: OFF (no cascade active)**

7. **Historical Stress Test Data** (8 rounds, 50→80% step 5%, `--json`; not a vsock qualification):
   - `total_allocated_mb`: 896 (consistente)
   - `peak_swap_mb`: 6–8
   - `tier3_ssd_mb`: 6–8
   - `tier3_throughput_mbs`: 0.0–4.7
   - `peak_pressure_index`: 1.20–1.42
   - `p50_cycle_latency_ms`: 0.0005
   - `p90_cycle_latency_ms`: 0.0007–0.0012
   - `p99_cycle_latency_ms`: 0.0019–0.0049
   - `buffer_drop_duration_ms`: 77.6–93.1
   - `host_vram_min_free_mb`: 4264–4368
   - Status: `INCONCLUSIVE` (cascade not active — expected without `ramshared up`)

## Kahneman Map Disciplines Addressed

- **#15 (Bounded connect):** `connect_wait_enforces_deadline_when_waiter_returns_late` proves late readiness is rejected; the real Linux path uses nonblocking connect plus `poll` and `SO_ERROR`, capped at five seconds.
- **#15 (Bounded accept):** `vsock_accept_timeout_is_bounded` covers the timeout helper used by the Windows listener.
- **#16 (Read timeout bounded):** `vsock_stream_read_timeout_is_bounded` verifies read timeout fires within 500ms.
- **#15 (Disconnect detection):** `vsock_disconnect_detected_within_interval` verifies EOF/error on socket close within 500ms.
- **#17 (Shadow comparison):** `host_gate_shadow_comparison` verifies Rust gate logic matches script on fixture inputs.
- **#13 (Lease revocation):** `lease_expiry_revokes_origin_authority` verifies origin authority revoked on expiry.
- **#16 (VHDX bounded):** `vhdx_attach_timeout_is_bounded` verifies attach completes within 15s window.

## Open Evidence (Env-Bound)

- **Live vsock RTT measurement:** Requires physical Windows host with WSL2 and paired AF_HYPERV/AF_VSOCK connection. Env-bound for Day-0.
- **Windows listener runtime:** Cross-target type-check is not a live bind/accept. Host service GUID registration, accepted guest connection, and bounded accept remain to be exercised on Windows.
- **Authenticated control-plane composition:** HMAC handshake, lease/manifest exchange, daemon startup wiring, and fail-closed disconnect transition are not implemented in the product path.
- **BINARY_MATCH:** Requires deployed daemon with vsock path active. Pending live E2E.
- **Live cascade with vsock:** Requires `ramshared up` with VHDX origin + vsock connection. Pending host activation.
