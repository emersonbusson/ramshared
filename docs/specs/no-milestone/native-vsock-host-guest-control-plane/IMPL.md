# IMPL — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Status

**implemented (all 6 ITEMs, hermetic gates passed)** · cover ✓ (87.8%–95.0%) · E2E env-bound (requires Windows host + WSL2 with AF_HYPERV/AF_VSOCK) · BINARY_MATCH pending

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-ipc/src/lib.rs` | ITEM-1 / RF-2, DT-2 | Shared protocol crate: `VsockFrameHeader` (24-byte binary framing, magic `0x52414D53`, version 3, `correlation_id`), 21 message types, serde JSON control messages with 4KB cap, raw manifest encoding, HMAC-SHA256 (manual implementation using `sha2`), version negotiation. 11 unit tests. |
| `crates/ramshared-ipc/src/vsock.rs` | ITEM-2 / RF-1, DT-1 | vsock transport abstraction: `connect_vsock` (AF_VSOCK, 5s bounded timeout), `listen_hyperv` (env-bound), `MockVsockStream` (UnixStream-backed for testing), `vsock_available` detection. 10 unit tests covering connect timeout, read timeout, disconnect detection, write/flush, trait methods, listener, endpoint. |
| `crates/ramshared-wsl2d/src/host_gate.rs` | ITEM-3 / RF-5, DT-6 | Absorbed gate logic from `ramshared-host-gate.sh`: origin manifest validation (SHA-256, size bounds, JSON fields), guardian health check (stale/unhealthy rejection), safe-mode gate (foreign boot_id rejection), lease minting (all-gates-required), lease expiry detection. 14 unit tests including shadow comparison. |
| `crates/ramshared-winsvc/src/control_plane.rs` | ITEM-4, ITEM-5, ITEM-6 / RF-1, RF-3, RF-4, DT-4, DT-5 | HeartbeatTracker (guest-initiated, 3× interval lease timeout), VhdxLifecycle (`wsl.exe --mount`/`--unmount` with mutex serialization and 10s timeout), ControlPlaneTelemetry (status JSON fields). 11 unit tests. |
| `crates/ramshared-ipc/Cargo.toml` | ITEM-1 | Crate manifest with `serde`, `serde_json`, `sha2` dependencies. |
| `crates/ramshared-ipc/README.md` | ITEM-1 | Crate README with Scope & Responsibility, Workspace Dependencies, Safety Invariants, Testing sections. |
| `Cargo.toml` (workspace) | ITEM-1 | Added `crates/ramshared-ipc` to workspace members. |
| `ARCHITECTURE.md` | ITEM-6 | Added Layer 6: Host-Guest IPC with `ramshared-ipc` reference. Updated crate count to 16. |

## Validation Results

1. **Unit & Protocol Tests (`ramshared-ipc`)**: 21 passed, 0 failed
   - `frame_round_trip`, `frame_rejects_bad_magic`, `frame_rejects_oversized_payload`, `frame_rejects_unknown_message_type`, `version_negotiation_selects_highest_mutual`, `handshake_hmac_validates`, `control_round_trip`, `control_payload_cap_enforced`, `manifest_round_trip`, `manifest_rejects_short_payload`, `lib_exports_and_constants_are_consistent`, `vsock_connect_timeout_falls_back`, `vsock_stream_read_timeout_is_bounded`, `vsock_disconnect_detected_within_interval`, `vsock_available_detects_platform`, `vsock_error_display_is_informative`, `mock_vsock_stream_write_and_flush`, `mock_vsock_stream_trait_methods`, `listen_hyperv_returns_unsupported_on_linux`, `vsock_listener_accept_returns_error_when_not_implemented`, `vsock_endpoint_fields_are_accessible`

2. **Gate Logic Tests (`ramshared-wsl2d`)**: 142 passed, 0 failed
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

3. **Control Plane Tests (`ramshared-winsvc`)**: 211 passed, 0 failed
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
   - `crates/ramshared-ipc/src/vsock.rs`: **91.9%** (158/172) — **PASS**
   - `crates/ramshared-wsl2d/src/host_gate.rs`: **95.0%** (247/260) — **PASS**
   - `crates/ramshared-winsvc/src/control_plane.rs`: **87.8%** (173/197) — **PASS**
   - Gate command: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-ipc,ramshared-wsl2d,ramshared-winsvc --files crates/ramshared-ipc/src/lib.rs,crates/ramshared-ipc/src/vsock.rs,crates/ramshared-wsl2d/src/host_gate.rs,crates/ramshared-winsvc/src/control_plane.rs --min 80` — **PASS**

5. **Code Quality & Lints**:
   - `cargo fmt --all --check` — **PASS**
   - `cargo clippy --all-targets -- -D warnings` — **PASS** (0 warnings, 0 errors)
   - `./scripts/docs-check.sh` — **PASS**

6. **Local Installation & Runtime**:
   - `sudo install -m 0755 target/release/{ramshared,ramsharedd} /usr/local/bin/` — **PASS**
   - `ramshared check --json` — **decision: ready, blockers: 0**
   - `ramshared doctor --json` — **recommendations: 2 (NBD module + bounded preflight)**
   - `ramshared status --json` — **phase: Off, cache_state: OFF (no cascade active)**

7. **Stress Test Data** (8 rodadas, 50→80% step 5%, `--json`):
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
   - Status: `INCONCLUSIVE` (cascade não ativo — correto sem `ramshared up`)

## Kahneman Map Disciplines Addressed

- **#15 (Bounded connect):** `vsock_connect_timeout_falls_back` verifies connect completes within bounded window.
- **#16 (Read timeout bounded):** `vsock_stream_read_timeout_is_bounded` verifies read timeout fires within 500ms.
- **#15 (Disconnect detection):** `vsock_disconnect_detected_within_interval` verifies EOF/error on socket close within 500ms.
- **#17 (Shadow comparison):** `host_gate_shadow_comparison` verifies Rust gate logic matches script on fixture inputs.
- **#13 (Lease revocation):** `lease_expiry_revokes_origin_authority` verifies origin authority revoked on expiry.
- **#16 (VHDX bounded):** `vhdx_attach_timeout_is_bounded` verifies attach completes within 15s window.

## Open Evidence (Env-Bound)

- **Live vsock RTT measurement:** Requires physical Windows host with WSL2 and paired AF_HYPERV/AF_VSOCK connection. Env-bound for Day-0.
- **BINARY_MATCH:** Requires deployed daemon with vsock path active. Pending live E2E.
- **Live cascade with vsock:** Requires `ramshared up` with VHDX origin + vsock connection. Pending host activation.
