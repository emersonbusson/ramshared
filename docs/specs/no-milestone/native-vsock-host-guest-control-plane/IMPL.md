# IMPL — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 3 · SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

## Status

**PARTIAL (protocol, gate, and transport source; product path not wired)** · Linux slice coverage and Clippy re-run on 2026-10-01 · live E2E and `BINARY_MATCH` are pending. Linux AF_VSOCK connect uses nonblocking connect, `poll`, `SO_ERROR`, and a hard five-second ceiling; Windows AF_HYPERV bind/listen/accept is implemented with bounded nonblocking accept and RAII socket cleanup. Neither daemon starts this listener/client. The DT-3 three-message role-separated HMAC-SHA256 handshake is implemented in `ramshared-ipc`: `Handshake` carries a fresh 32-byte guest nonce and guest proof, `HandshakeAck` carries a fresh host nonce and host proof over both nonces and the complete transcript, and `HandshakeFinish` (`MSG_HANDSHAKE_FINISH = 22`) proves the same transcript back. Nonces come from the OS CSPRNG (`random_nonce`) and an all-zero nonce is refused; proof comparison is constant-time; role labels make a proof from one role unusable in another. `ControlPlaneAuthority` in `host_gate` composes lease and origin identity with cache admission: a vsock disconnect enters `safe_mode` and revokes cache authority while preserving the verified origin, lease expiry leaves `origin_only`, origin identity loss blocks every I/O path, and `lease_after_connect` never mints a lease after a failed connect (no file fallback). The handshake is still not composed with live lease or manifest delivery on a real socket. `VhdxLifecycle` takes an injectable `CommandRunner`, so the attach success path, the PartUUID idempotency skip, and detach cleanup are unit-provable without a Windows host; the production `WslRunner` still requires `wsl.exe`.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `crates/ramshared-ipc/src/lib.rs` | ITEM-1 / RF-2, DT-2, DT-3 | Shared protocol crate: `VsockFrameHeader` (24-byte binary framing, magic `0x52414D53`, version 3, `correlation_id`), 22 message types including `MSG_HANDSHAKE_FINISH = 22`, serde JSON control messages with 4KB cap, raw manifest encoding, HMAC-SHA256 (manual implementation using `sha2`), version negotiation, and the DT-3 role-separated handshake transcript (`guest_proof` / `host_proof` / `guest_finish_proof` with `random_nonce` from the OS CSPRNG and constant-time `ct_eq32`). Unit tests include `mutual_handshake_requires_fresh_role_bound_proofs` and `handshake_finish_rejects_replayed_host_challenge`. |
| `crates/ramshared-ipc/src/vsock.rs` | ITEM-2 / RF-1, DT-1 | Functional source adapters: Linux AF_VSOCK nonblocking connect with caller timeout capped at 5s and `SO_ERROR` result checks; Windows AF_HYPERV service listener with canonical GUID conversion, wildcard VM ID, nonblocking accept deadline, typed errors, and RAII ownership. `vsock_available` probes socket creation. Hermetic tests cover deadline handling, accept timeout/error, GUID conversion, stream behavior, and platform refusal. Live Windows/WSL socket exchange remains untested. |
| `crates/ramshared-wsl2d/src/host_gate.rs` | ITEM-3 / RF-5, DT-6 | Absorbed gate logic from `ramshared-host-gate.sh`: origin manifest validation (SHA-256, size bounds, JSON fields), guardian health check (stale/unhealthy rejection), safe-mode gate (foreign boot_id rejection), lease minting (all-gates-required), lease expiry detection. Added `ControlPlaneAuthority` (cache admission needs a live lease plus a verified origin; origin I/O needs only the verified origin; disconnect enters `safe_mode` and revokes cache; lease expiry leaves `origin_only`; origin identity loss blocks every path) and `lease_after_connect` (mint runs only after a successful connect). Shadow comparison and the ITEM-3 tests remain in this module and in `main.rs`. |
| `crates/ramshared-winsvc/src/control_plane.rs` | ITEM-4, ITEM-5, ITEM-6 / RF-1, RF-3, RF-4, DT-4, DT-5 | HeartbeatTracker (guest-initiated, 3× interval lease timeout), VhdxLifecycle (`wsl.exe --mount`/`--unmount` with mutex serialization, 10s timeout, and an injectable `CommandRunner` — production `WslRunner`), ControlPlaneTelemetry (status JSON fields). 21 unit tests. |
| `crates/ramshared-ipc/Cargo.toml` | ITEM-1 | Crate manifest with `serde`, `serde_json`, `sha2` dependencies. |
| `crates/ramshared-ipc/README.md` | ITEM-1 | Crate README with Scope & Responsibility, Workspace Dependencies, Safety Invariants, Testing sections. |
| `Cargo.toml` (workspace) | ITEM-1 | Added `crates/ramshared-ipc` to workspace members. |
| `ARCHITECTURE.md` | ITEM-6 | Added Layer 6: Host-Guest IPC with `ramshared-ipc` reference. Updated crate count to 16. |

## Validation Results

1. **Transport and Protocol Tests (`ramshared-ipc`)**: 44 passed, 0 failed on Linux (re-run 2026-10-01).
   - Covers protocol frames and HMAC primitives, Linux connect boundedness, late poller deadline rejection, socket error propagation, five-second cap, bounded accept timeout, accept error propagation, canonical Hyper-V service GUID validation, read timeout, disconnect detection, stream read/write, and platform-specific unsupported behavior.
   - `tests::mutual_handshake_requires_fresh_role_bound_proofs` — **PASS** (legitimate three-message flow; guest proof refused as host and as finish, host proof refused as guest and as finish, wrong secret refused, stale guest or host nonce refused, zeroed proof and zeroed nonces refused).
   - `tests::handshake_finish_rejects_replayed_host_challenge` — **PASS** (a finish bound to one host challenge fails against a different challenge, a fresh guest nonce, and a replayed host proof).
   - `tests::frame_accepts_handshake_finish_and_rejects_type_above_max` — **PASS** (type 22 decodes; type 23 is `UnknownMessageType`).
   - `tests::frame_rejects_unsupported_version` — **PASS** (versions 1 and 4 are `UnsupportedVersion` before any payload is read).
   - `tests::frame_error_display_is_total` / `tests::handshake_error_display_is_total` — **PASS** (every error arm renders a distinct, total Display; telemetry never fails to describe a refusal).
   - `tests::control_and_manifest_caps_are_enforced` — **PASS** (control payload > 4 KiB and manifest total > 64 KiB are `PayloadExceedsCap`; a hex-length field that overruns the payload is refused).
   - `tests::hmac_long_secret_and_length_mismatch_are_handled` — **PASS** (RFC 2104 long-secret hashing; truncated expected MAC refused by length).
   - `tests::random_nonce_is_fresh_and_nonzero` — **PASS** (OS CSPRNG `getrandom(2)`; two draws are non-zero and distinct).
   - `tests::handshake_refuses_empty_claims_and_bad_nonces` — **PASS** (empty `boot_id`/`distro_id` → `EmptyIdentityClaim`; all-zero guest or host nonce → `NonceNotFresh`; the three verify functions return `false` rather than panic).
   - `cargo fmt --all -- --check` — **PASS**.
   - `cargo clippy -p ramshared-ipc --all-targets -- -D warnings` — **PASS**.
   - `CARGO_BUILD_JOBS=1 cargo check -p ramshared-ipc --all-targets --target x86_64-pc-windows-gnu` — **PASS** (historical); this type-checks Windows library and test code but does not execute a Windows listener. Re-run required after the DT-3 transcript landed if a Windows type-check is claimed again.

2. **Gate Logic Tests (`ramshared-wsl2d`)**: 179 library + 126 `ramsharedd` binary tests passed, 0 failed (re-run 2026-10-01).
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
   - `host_gate::tests::lease_expiry_is_detected` — **PASS** (checks only whether the deadline has passed; authority revocation is covered by the ITEM-3 tests below)
   - `host_gate::tests::host_gate_shadow_comparison` — **PASS**
   - `tests::lease_expiry_revokes_cache_and_keeps_verified_origin` — **PASS** (ITEM-3: cache refused after expiry, verified origin still admitted, already-expired token refused at accept).
   - `tests::origin_identity_loss_blocks_io` — **PASS** (ITEM-3 companion: losing the verified origin blocks cache and origin I/O).
   - `tests::vsock_disconnect_triggers_safe_mode` — **PASS** (state becomes `safe_mode`, cache refused, verified origin preserved; a bare re-handshake does not restore cache admission).
   - `tests::connection_failure_never_grants_lease` — **PASS** (mint closure does not run after a failed connect; a failing mint yields no token; only a successful connect can mint).

3. **Control Plane Tests (`ramshared-winsvc`)**: 229 passed, 0 failed, 1 ignored in the library suite; 4 probe tests passed (re-run 2026-09-30). All 21 `control_plane::tests::*` pass:
   - `control_plane::tests::heartbeat_deadline_revokes_lease` — **PASS**
   - `control_plane::tests::heartbeat_lease_remaining_counts_down` — **PASS**
   - `control_plane::tests::heartbeat_tracker_lease_remaining_zero_when_no_heartbeat` — **PASS**
   - `control_plane::tests::heartbeat_lease_remaining_is_zero_once_expired` — **PASS** (zero-timeout expiry reports 0, no underflow)
   - `control_plane::tests::heartbeat_recovers_from_a_poisoned_lock` — **PASS**
   - `control_plane::tests::vhdx_attach_records_partuuid_on_success` — **PASS** (success path via injected `CommandRunner`)
   - `control_plane::tests::vhdx_attach_is_idempotent` — **PASS** (**Kahneman #17 proof**: second attach of the same PartUUID issues exactly one spawn)
   - `control_plane::tests::vhdx_attach_failure_leaves_list_empty` — **PASS**
   - `control_plane::tests::vhdx_detach_clears_attached_list` — **PASS**
   - `control_plane::tests::vhdx_recovers_from_a_poisoned_lock` — **PASS**
   - `control_plane::tests::vhdx_attach_timeout_is_bounded` — **PASS**
   - `control_plane::tests::vhdx_detach_is_bounded` — **PASS**
   - `control_plane::tests::vhdx_attached_partuuids_empty_after_failed_attach` — **PASS**
   - `control_plane::tests::vhdx_command_runner_reaps_a_timed_out_child` — **PASS** (`#[cfg(unix)]`)
   - `control_plane::tests::control_plane_telemetry_serializes` — **PASS**
   - `control_plane::tests::control_plane_state_as_str` — **PASS**
   - `control_plane::tests::default_impls_match_new` — **PASS**
   - `control_plane::tests::bounded_command_accepts_success` — **PASS**
   - `control_plane::tests::bounded_command_reports_spawn_failure` — **PASS**
   - `control_plane::tests::bounded_command_reports_nonzero_exit` — **PASS**
   - `control_plane::tests::wsl_runner_adapts_bounded_command_output` — **PASS**

   The retired `command_timeout_extension_does_not_panic` is gone: its subject
   (the timeout-argument extension) no longer exists in `run_command_bounded`,
   which takes a `deadline` and is covered by `bounded_command_*` and
   `vhdx_command_runner_reaps_a_timed_out_child`.

4. **Rust Slice Coverage Gate** (min 80%, metric=lines; single snapshot 2026-10-01):
   - `crates/ramshared-ipc/src/lib.rs`: **96.1%** (273/284) — **PASS**
   - `crates/ramshared-ipc/src/vsock.rs`: **93.0%** (198/213) — **PASS** on Linux. Windows-only FFI branches are excluded from this coverage run; they type-check and pass Clippy on the Windows target.
   - `crates/ramshared-wsl2d/src/host_gate.rs`: **91.9%** (205/223) — **PASS**
   - `crates/ramshared-winsvc/src/control_plane.rs`: **95.9%** (140/146) — **PASS**
   - Combined gate: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-ipc,ramshared-wsl2d,ramshared-winsvc --files crates/ramshared-ipc/src/lib.rs,crates/ramshared-ipc/src/vsock.rs,crates/ramshared-wsl2d/src/host_gate.rs,crates/ramshared-winsvc/src/control_plane.rs --min 80` — **PASS**.

   The 2026-10-01 snapshot is the authority: it is one measurement of the four
   files together, produced by the exact command above, after the DT-3
   transcript and `ControlPlaneAuthority` landed. It replaces the 2026-09-30
   snapshot (`lib.rs` 84.3% (129/153), `host_gate.rs` 90.0% (117/130)) and
   earlier figures recorded under a different llvm-cov line denominator
   (`lib.rs` 215/239, `vsock.rs` 330/385, `host_gate.rs` 247/260,
   `control_plane.rs` 181/208). `lib.rs` sat at 79.6% (226/284) after the
   handshake landed — below the 80% gate — until the refusal-path tests in
   section 1 covered the Display arms, the version/oversize/hex-length
   refusals, the RFC 2104 long-secret branch, `random_nonce`, and the
   `Err(_) => false` verifier arms.

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
- **#17 (VHDX idempotency):** `vhdx_attach_is_idempotent` asserts the injected `CommandRunner` is invoked exactly once across two `attach` calls with the same PartUUID. Before 2026-09-30 this test asserted `partuuids.len() <= 1` and never reached the skip branch, so the retry-safety claim was unproven.
- **#13 (Lease revocation):** `lease_expiry_revokes_cache_and_keeps_verified_origin` proves cache admission is denied after lease expiry while the verified origin path stays available; `origin_identity_loss_blocks_io` proves origin identity loss blocks every path; `vsock_disconnect_triggers_safe_mode` proves disconnect fails cache admission closed into `safe_mode`; `connection_failure_never_grants_lease` proves a failed connect never produces a token. `lease_expiry_is_detected` remains as the deadline-detection helper test.
- **#16 (VHDX bounded):** `vhdx_attach_timeout_is_bounded` verifies attach completes within 15s window.

## Open Evidence (Env-Bound)

- **Live vsock RTT measurement:** Requires physical Windows host with WSL2 and paired AF_HYPERV/AF_VSOCK connection. Env-bound for Day-0.
- **Windows listener runtime:** Cross-target type-check is not a live bind/accept. Host service GUID registration, accepted guest connection, and bounded accept remain to be exercised on Windows.
- **Authenticated control-plane composition:** The DT-3 HMAC handshake and the fail-closed disconnect transition are implemented and unit-proven (`mutual_handshake_requires_fresh_role_bound_proofs`, `handshake_finish_rejects_replayed_host_challenge`, `vsock_disconnect_triggers_safe_mode`). Still unwired in the product path: composing that handshake with live lease and manifest delivery over a real socket, and starting the listener/client from either daemon. Env-bound until a host-guest session runs.
- **BINARY_MATCH:** Requires deployed daemon with vsock path active. Pending live E2E.
- **Live cascade with vsock:** Requires `ramshared up` with VHDX origin + vsock connection. Pending host activation.
