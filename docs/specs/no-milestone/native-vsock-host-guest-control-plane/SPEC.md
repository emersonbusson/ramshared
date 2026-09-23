# SPEC — Native vsock host-guest control plane (zero scripts)

> SSDV3 Step 2 · PRD: docs/specs/no-milestone/native-vsock-host-guest-control-plane/PRD.md

## Closed scope

### In now
- Shared `ramshared-ipc` protocol crate (binary framing, message types, version negotiation).
- AF_VSOCK client module in `ramshared-wsl2d` (guest-side transport).
- AF_HYPERV listener module in `ramshared-winsvc` (host-side transport).
- Absorbed origin manifest validation, guardian health, safe-mode gate, lease logic from `ramshared-host-gate.sh` into `ramshared-wsl2d`.
- Absorbed VHDX lifecycle (`wsl.exe --mount`) and heartbeat monitoring from `Manage-RamSharedOrigin.ps1` / `Watch-RamSharedWsl.ps1` into `ramshared-winsvc`.
- Heartbeat/lease over vsock replacing file-based JSON over 9P/Drvfs.
- Fail-closed disconnect handling on both sides.
- ETW events (host) and journald structured logs (guest) for all state transitions.

### Out now
- VMBus ring-buffer zero-copy (`vmbus-ring-buffer-upstream-v2`).
- GPU cache worker changes (`wsl2-isolated-gpu-cache-worker`).
- Windows WDF/StorPort driver (`windows-storport-cuda-vram`).
- CI/build scripts in `scripts/package/` and benchmark scripts in `scripts/p0/`.
- Multi-distro WSL2 support (Day-0 target: `Ubuntu-24.04`).

### Assumed-ready dependencies
- `crates/ramshared-winsvc/src/ipc.rs` — `IpcMessageHeader`, `IpcDeserializeError`, `IPC_MAGIC` (`0x52414D53`), `MAX_PAYLOAD_LEN`.
- `crates/ramshared-winsvc/src/config.rs` — `WinsvcConfig`, `BrokerPipeV1`, `heartbeat_secs`.
- `crates/ramshared-winsvc/src/broker_tenant.rs` — `BrokerTenant`, `LeaseState`, `ReleaseSent`.
- `crates/ramshared-winsvc/src/product_online.rs` — `HostGates`, heartbeat loop.
- `crates/ramshared-wsl2d/src/main.rs` — `ORIGIN_MANIFEST_PATH`, `HOST_ORIGIN_MANIFEST_PATH`, `ORIGIN_MANIFEST_MAX_BYTES`, `read_sealed_origin_manifest`, `validate_host_origin_manifest_bytes`.
- `crates/ramshared-winbroker/src/pipe.rs` — `PipeServer`, `AuthenticatedPipe` pattern (reference for auth).
- WSL2 kernel ≥ 5.10 with `CONFIG_VSOCKETS` and `CONFIG_VSOCKETS_STREAM`.

---

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 (Native Transport) | ITEM-1, ITEM-2, DT-1 |
| RF-2 (Shared Protocol) | ITEM-1, DT-2 |
| RF-3 (Heartbeat & Lease) | ITEM-3, ITEM-4, DT-5 |
| RF-4 (VHDX Lifecycle) | ITEM-5, DT-4 |
| RF-5 (Script Absorption) | ITEM-3, ITEM-4, ITEM-5, DT-6 |
| RF-6 (Fail-Closed Disconnect) | ITEM-2, ITEM-3, DT-7 |
| RF-7 (Observability) | ITEM-6, DT-8 |
| NFR-1 (Latency ≤ 1ms) | ITEM-2, DT-1 |
| NFR-2 (Zero Scripts) | ITEM-5, DT-6 |
| NFR-3 (Atomic Updates) | DT-2 |
| NFR-4 (Host Safety) | DT-4 |
| NFR-5 (Security) | DT-3 |

---

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Host uses `AF_HYPERV` (34) `SOCK_STREAM`; guest uses `AF_VSOCK` (40) `SOCK_STREAM`. Both connect through the Hyper-V socket bus. Guest addresses `VMADDR_CID_HOST` (CID 2) on a configured port; host listens on a RamShared-specific well-known GUID. Connect timeout is 5s, after which DT-7 file-based fallback activates. | AF_HYPERV is the Windows-side API for Hyper-V sockets; AF_VSOCK is the Linux-side. Same underlying VMBus transport, sub-millisecond latency, instant disconnect on VM suspend/crash. |
| DT-2 | Protocol lives in a new shared crate `crates/ramshared-ipc`. Extends `IpcMessageHeader` with `version = 3`, adds `correlation_id: u64` field after `flags`. Message types in `flags` lower 8 bits (1–21 as in PRD §7). Payloads are serde JSON for control messages with hard cap 4KB (exceeding cap → `PayloadTooLarge` at deserialization); raw bytes for OriginManifest ≤ 64KB. Version negotiation: guest sends `min_version..=max_version`, host picks highest mutual. | Reuses existing `IPC_MAGIC` and framing patterns. `correlation_id` enables request-response matching over a single stream. JSON for control messages keeps debuggability; raw bytes for manifest avoids double-encoding. Hard cap prevents resource abuse. |
| DT-3 | Authentication: GUID-based allowlisting (host validates guest connects from expected WSL2 VM) + shared HMAC-SHA256 secret stored in `winsvc.toml` (host) and `/etc/ramshared/config.toml` (guest). Secret is file-permission-restricted (0600 root/SYSTEM) and may be sourced from environment variable override. Handshake includes `HMAC(boot_id + distro_id + nonce)`. No secret in logs or status JSON. No unauthenticated origin authority minting. | GUID allowlisting leverages Hyper-V socket isolation (only the paired VM can connect). HMAC provides cryptographic authentication without PKI complexity. File permissions prevent at-rest exposure. |
| DT-4 | VHDX attach via `CreateProcessW("wsl.exe", "--mount --vhd <path> --bare --type ext4")` with 10s timeout and idempotency check (`/dev/disk/by-partuuid/` already exists → skip). Detach via `wsl.exe --unmount`. Operations serialized through a mutex; concurrent requests are queued, not parallel. Not `virtdisk.dll` (that targets Hyper-V VMs, not WSL2 block devices). | WSL2 `wsl.exe --mount` is the supported API for attaching VHDX as bare block devices. Mutex serialization prevents concurrent mount races. |
| DT-5 | Heartbeat is guest-initiated at `heartbeat_secs` (default 5s). Host tracks `last_heartbeat_at` and revokes lease when `now - last_heartbeat_at > 3 × heartbeat_secs` (default 15s). Guest also monitors its own lease deadline and enters safe mode proactively. In-flight writes complete or abort cleanly; lease expiry sets `origin_authoritative = false` before next I/O dispatch. | Guest-initiated avoids host polling overhead. 3× interval tolerance absorbs scheduling jitter without false positives. Dual-sided monitoring ensures fail-closed even if one side misses. Clean I/O boundary prevents mid-write corruption. |
| DT-6 | Script absorption is phased: (1) transport layer, (2) gate logic into `ramshared-wsl2d`, (3) VHDX/heartbeat into `ramshared-winsvc`, (4) cutover + cleanup. Each phase is independently deployable and testable. | Incremental absorption allows shadow comparison and rollback at each phase. Avoids big-bang migration risk. |
| DT-7 | File-based heartbeat over 9P/Drvfs remains as a deprecated fallback path, activated only when `AF_VSOCK` connection fails at startup (e.g., `CONFIG_VSOCKETS` missing). Fallback is marked deprecated, logs a warning, and will be removed after N+2 releases. | Graceful degradation for WSL2 kernels without vsock support. Explicit deprecation prevents permanent reliance. |
| DT-8 | Observability: guest emits `tracing` structured events to journald (`tracing-journald` crate). Host emits ETW events via `windows-rs` `EventWrite`. Both sides expose `control_plane_state`, `heartbeat_rtt_us`, `lease_remaining_ms` in their status JSON. | Structured logging with machine-parseable fields. `heartbeat_rtt_us` is the key NFR-1 metric. |

---

## Atomicity and rollback

### Atomicity frontier
- **Origin Manifest (Authoritative):** Sealed at `/etc/ramshared/origin.conf` only after full validation. Atomic rename-based write. Guest origin backend operates independently of vsock after sealing.
- **Lease (Ephemeral Authority):** Host-authoritative. Lease expiry revokes guest's origin authority but never corrupts disk state. In-flight writes complete or abort cleanly; lease expiry sets `origin_authoritative = false` before next I/O dispatch. Guest falls back to safe mode (origin-only, no cache).
- **vsock Channel (Boundary):** Connection loss triggers fail-closed on both sides. No partial state persists after disconnect.

### Rollback
- **Userspace/guest (`ramshared-wsl2d`):** Revert to file-based heartbeat fallback (DT-7). Origin manifest stays sealed; cascade continues without vsock telemetry.
- **Userspace/host (`ramshared-winsvc`):** Disable AF_HYPERV listener. Existing Named Pipe IPC (`ramshared-winbroker`) continues unaffected.
- **Host/persistent:** No persistent state modified. `/proc/swaps`, NBD devices, VHDX files remain intact. Lease is in-memory only.
- **Forward-only:** None — all changes are reversible.

---

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 (vsock connect) | #15 | Can a hung Hyper-V socket `connect()` block the daemon indefinitely? | `cargo test -p ramshared-ipc vsock_connect_timeout_falls_back` | Connect exceeds 5s without fallback |
| ITEM-3 (lease revocation) | #13 | If the lease expires mid-I/O, does the guest refuse origin authority without corrupting state? | `cargo test -p ramshared-wsl2d lease_expiry_revokes_origin_authority` | Guest serves origin write after lease expiry |
| ITEM-4 (gate absorption) | #17 | Does the Rust gate logic produce identical decisions to `ramshared-host-gate.sh` on the same inputs? | `cargo test -p ramshared-wsl2d host_gate_shadow_comparison` | Any mismatch on 1000+ fixture inputs |
| ITEM-5 (VHDX attach) | #16 | Can a hung `wsl.exe --mount` block the service beyond the 10s deadline? | `cargo test -p ramshared-winsvc vhdx_attach_timeout_is_bounded` | Attach exceeds 10s or leaves orphan mount |

---

## Security checklist (pre-impl)

- [x] Privilege: host service runs as `NT AUTHORITY\SYSTEM`; guest daemon runs as root. vsock GUID allowlisting restricts connections to the paired WSL2 VM. HMAC secret never transmitted in plaintext.
- [x] User/host copy: all frames bounded to `MAX_PAYLOAD_LEN` (1MB). Manifest payloads capped at `ORIGIN_MANIFEST_MAX_BYTES` (64KB). Operate on owned copies after `read_exact`.
- [x] Flags/IOCTL codes: unknown message types (flags lower 8 bits > 21) rejected with `IpcDeserializeError::UnsupportedVersion`. Unknown protocol versions rejected.
- [x] Info-leak: no kernel addresses, no HMAC secrets, no host paths in default logs. Guest logs use `tracing` with redacted fields. Host ETW events use integer codes.
- [x] IRQ/atomic: N/A — pure userspace.
- [x] Lifetime: vsock socket ownership tracked by `ConnectionGuard` (Drop closes socket). Lease state cleaned on disconnect. VHDX detach on shutdown.
- [x] Hot-unplug: vsock disconnect triggers fail-closed (DT-7). VHDX detach is idempotent. No UAF — all state behind `Arc<Mutex<>>`.
- [x] Host safety: VHDX attach/detach bounded to 10s (DT-4). No GPU/VRAM pressure from control plane. Heartbeat RTT monitored (NFR-1).
- [x] Shared-hardware cushion: N/A — control plane does not allocate shared VRAM/RAM.
- [x] Bounded DMA: N/A — no DMA in control plane.
- [x] Cooperative cascade spillover: N/A — control plane does not serve block I/O.
- [x] Replayable ops: `VHDXAttach`/`VHDXDetach` idempotent (check PartUUID before act). `Disable`/cleanup idempotent (#17). Handshake re-runnable after reconnect.

---

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-ipc/src/lib.rs`**
- Purpose: Shared protocol types, framing, message definitions, version negotiation.
- RF / DT: RF-2, DT-2, NFR-3.
- Types / fns:
  ```rust
  pub const IPC_VERSION_3: u32 = 3;
  pub const MAX_PAYLOAD_LEN: u32 = 1024 * 1024;
  pub const MSG_HANDSHAKE: u8 = 1;
  pub const MSG_HANDSHAKE_ACK: u8 = 2;
  pub const MSG_HEARTBEAT: u8 = 3;
  pub const MSG_HEARTBEAT_ACK: u8 = 4;
  pub const MSG_LEASE_REQUEST: u8 = 5;
  pub const MSG_LEASE_GRANTED: u8 = 6;
  pub const MSG_LEASE_DENIED: u8 = 7;
  pub const MSG_LEASE_RELEASE: u8 = 8;
  pub const MSG_ORIGIN_MANIFEST: u8 = 9;
  pub const MSG_ORIGIN_MANIFEST_ACK: u8 = 10;
  pub const MSG_SAFE_MODE_GATE: u8 = 11;
  pub const MSG_SAFE_MODE_GATE_ACK: u8 = 12;
  pub const MSG_GUARDIAN_HEALTH: u8 = 13;
  pub const MSG_GUARDIAN_HEALTH_ACK: u8 = 14;
  pub const MSG_VHDX_ATTACH: u8 = 15;
  pub const MSG_VHDX_ATTACH_ACK: u8 = 16;
  pub const MSG_VHDX_DETACH: u8 = 17;
  pub const MSG_VHDX_DETACH_ACK: u8 = 18;
  pub const MSG_TELEMETRY: u8 = 19;
  pub const MSG_SHUTDOWN: u8 = 20;
  pub const MSG_SHUTDOWN_ACK: u8 = 21;

  pub struct VsockFrameHeader { /* magic, version, payload_len, flags, correlation_id */ }
  pub struct Handshake { pub min_version: u32, pub max_version: u32, pub boot_id: String, pub distro_id: String, pub hmac: Vec<u8> }
  pub struct HandshakeAck { pub accepted_version: u32, pub heartbeat_secs: u64, pub lease_timeout_secs: u64 }
  pub struct Heartbeat { pub timestamp_ms: u64 }
  pub struct LeaseRequest { pub nonce: Vec<u8> }
  pub struct LeaseGranted { pub lease_id: u32, pub deadline_ms: u64 }
  pub struct OriginManifestPayload { pub sha256: String, pub data: Vec<u8> }
  pub struct VhdxAttachRequest { pub path: String, pub partuuid: String }
  // ... remaining message types per PRD §7
  ```
- Reference pattern: `crates/ramshared-winsvc/src/ipc.rs` (framing), `crates/ramshared-winsvc/src/proto.rs` (constants).
- Required tests: `ramshared-ipc/src/lib.rs` :: `frame_round_trip`, `frame_rejects_bad_magic`, `frame_rejects_oversized_payload`, `version_negotiation_selects_highest_mutual`, `handshake_hmac_validates`.
- Cover target: ≥ 80%

**`crates/ramshared-ipc/src/vsock.rs`**
- Purpose: vsock transport abstraction (connect, listen, accept) with bounded timeouts.
- RF / DT: RF-1, DT-1, NFR-1.
- Types / fns:
  ```rust
  pub struct VsockEndpoint { /* platform socket */ }
  pub fn connect_vsock(cid: u32, port: u32, timeout: Duration) -> Result<VsockStream, VsockError>;
  pub fn listen_hyperv(guid: [u8; 16], timeout: Duration) -> Result<VsockListener, VsockError>;
  pub struct VsockStream { /* Read + Write + set_read_timeout */ }
  ```
- Reference pattern: `crates/ramshared-wsl2d/src/main.rs` `UnixStream` usage; `crates/ramshared-winbroker/src/pipe.rs` `PipeServer` pattern.
- Required tests: `ramshared-ipc/src/vsock.rs` :: `vsock_connect_timeout_falls_back`, `vsock_stream_read_timeout_is_bounded`, `vsock_disconnect_detected_within_interval`.
- `VsockEndpoint` wraps platform socket with `cid`, `port`, `guid` metadata for diagnostics.
- Cover target: ≥ 80%
- Kahneman: #15 (bounded connect)

**`crates/ramshared-wsl2d/src/host_gate.rs`**
- Purpose: Absorbed gate logic from `ramshared-host-gate.sh` — origin manifest validation, guardian health check, safe-mode gate, lease minting.
- RF / DT: RF-5, DT-6.
- Types / fns:
  ```rust
  pub fn validate_origin_manifest(data: &[u8], expected_sha256: &str) -> Result<SealedOrigin, GateError>;
  pub fn check_guardian_health(health_json: &[u8], max_age_sec: u64) -> Result<(), GateError>;
  pub fn evaluate_safe_mode(gate_json: &[u8], boot_id: &str) -> Result<SafeModeDecision, GateError>;
  pub fn mint_lease(manifest: &SealedOrigin, guardian_ok: bool) -> Result<LeaseToken, GateError>;
  ```
- Reference pattern: `scripts/safety/ramshared-host-gate.sh` (logic to absorb), `crates/ramshared-wsl2d/src/main.rs` `read_sealed_origin_manifest`, `validate_host_origin_manifest_bytes`.
- Required tests: `ramshared-wsl2d/src/host_gate.rs` :: `validate_origin_manifest_matches_script`, `check_guardian_health_rejects_stale`, `evaluate_safe_mode_refuses_foreign_boot_id`, `mint_lease_requires_all_gates`, `host_gate_shadow_comparison`.
- Cover target: ≥ 80%
- Kahneman: #17 (shadow comparison), #13 (lease revocation)

### MODIFY

**`crates/ramshared-wsl2d/src/main.rs`**
- What: Add AF_VSOCK client startup path, lease heartbeat loop, fail-closed on disconnect. Replace `HOST_ORIGIN_MANIFEST_PATH` file reads with vsock `OriginManifest` message.
- RF / DT: RF-1, RF-3, RF-6, DT-5, DT-7.
- Symbols: add `VsockControlPlane` struct; modify `run_nbd_with_startup` to accept `ControlPlane` trait (vsock or file fallback); add `HeartbeatLoop`.
- Tests: `lease_expiry_revokes_origin_authority`, `vsock_disconnect_triggers_safe_mode`, `file_fallback_activated_when_vsock_unavailable`.
- Cover: ≥ 80%
- Kahneman: #13

**`crates/ramshared-winsvc/src/product_online.rs`**
- What: Add AF_HYPERV listener, VHDX lifecycle (`wsl.exe --mount`), heartbeat deadline tracking. Absorb `Watch-RamSharedWsl.ps1` monitoring.
- RF / DT: RF-1, RF-3, RF-4, DT-4, DT-5.
- Symbols: add `HypervListener`, `VhdxLifecycle`, `HeartbeatTracker`; modify `HostGates` to receive vsock messages instead of file reads.
- Tests: `vhdx_attach_timeout_is_bounded`, `vhdx_attach_is_idempotent`, `heartbeat_deadline_revokes_lease`, `hyperv_listener_accepts_guest_connection`.
- Cover: ≥ 80%
- Kahneman: #16 (bounded attach)

**`crates/ramshared-winsvc/src/config.rs`**
- What: Add `vsock_guid: String`, `vsock_port: u32`, `lease_timeout_secs: u64`, `hmac_secret: String` fields.
- RF / DT: DT-1, DT-3, DT-5.
- Symbols: extend `WinsvcConfig`.
- Tests: `config_parses_vsock_fields`, `config_defaults_are_safe`.
- Cover: ≥ 80%

**`crates/ramshared-winsvc/src/ipc.rs`**
- What: Re-export framing types into `ramshared-ipc` (or delegate). Add `correlation_id` to `IpcMessageHeader` for v3.
- RF / DT: DT-2.
- Symbols: `IpcMessageHeader` → `VsockFrameHeader` (v3 adds `correlation_id`).
- Tests: `header_v3_round_trip`, `header_v2_backward_compat`.
- Cover: ≥ 80%

### DELETE

- `scripts/safety/ramshared-host-gate.sh` — absorbed into `ramshared-wsl2d/src/host_gate.rs`. Deprecate at Phase 4, remove at N+2.
- `scripts/windows/Manage-RamSharedOrigin.ps1` — absorbed into `ramshared-winsvc/src/product_online.rs`. Deprecate at Phase 4, remove at N+2.
- `scripts/windows/Watch-RamSharedWsl.ps1` — absorbed into `ramshared-winsvc/src/product_online.rs`. Deprecate at Phase 4, remove at N+2.

---

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| `control_plane_state` | `ramshared status --json` / ETW | Enum (`vsock`, `file_fallback`, `safe_mode`) |
| `heartbeat_rtt_us` | `ramshared status --json` / ETW | Histogram (u64, microseconds) |
| `lease_remaining_ms` | `ramshared status --json` | Gauge (u64, milliseconds) |
| `vsock_disconnect_count` | ETW / journald | Counter (u64) |
| `vhdx_attach_result` | ETW | Event (ok/fail + error code) |
| `gate_decision` | journald | Event (pass/fail + reason code) |

---

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter — add vsock control plane diagram |
| `docs/reliability/GAP-REGISTER.md` | Update upon qualification |
| `validation.md` | Append on close |
| `trovaldo.md` | Update host-guest communication status |
| `README.md` | Alter — architecture section, remove script references |

---

## Implementation order

1. **ITEM-1:** Create `crates/ramshared-ipc` — framing, message types, version negotiation, HMAC helper. Unit tests.
2. **ITEM-2:** Create `crates/ramshared-ipc/src/vsock.rs` — transport abstraction with bounded timeouts. Unit tests with mock sockets.
3. **ITEM-3:** Extend `ramshared-wsl2d` — AF_VSOCK client, lease heartbeat loop, fail-closed disconnect. Absorb gate logic (`host_gate.rs`). Unit tests + shadow comparison.
4. **ITEM-4:** Extend `ramshared-winsvc` — AF_HYPERV listener, heartbeat deadline tracking, lease management. Unit tests.
5. **ITEM-5:** Extend `ramshared-winsvc` — VHDX lifecycle (`wsl.exe --mount`), absorb `Manage-RamSharedOrigin.ps1`. Unit tests.
6. **ITEM-6:** Observability — `tracing`/`EventWrite` integration, status JSON fields. Integration tests.

---

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| `crates/ramshared-ipc/src/lib.rs` | `tests::frame_round_trip` | unit | #9 | ≥ 80% |
| `crates/ramshared-ipc/src/lib.rs` | `tests::frame_rejects_bad_magic` | unit | #13 | ≥ 80% |
| `crates/ramshared-ipc/src/lib.rs` | `tests::frame_rejects_oversized_payload` | unit | #13 | ≥ 80% |
| `crates/ramshared-ipc/src/lib.rs` | `tests::version_negotiation_selects_highest_mutual` | unit | #9 | ≥ 80% |
| `crates/ramshared-ipc/src/lib.rs` | `tests::handshake_hmac_validates` | unit | #13 | ≥ 80% |
| `crates/ramshared-ipc/src/vsock.rs` | `tests::vsock_connect_timeout_falls_back` | unit | #15 | ≥ 80% |
| `crates/ramshared-ipc/src/vsock.rs` | `tests::vsock_stream_read_timeout_is_bounded` | unit | #16 | ≥ 80% |
| `crates/ramshared-ipc/src/vsock.rs` | `tests::vsock_disconnect_detected_within_interval` | unit | #15 | ≥ 80% |
| `crates/ramshared-wsl2d/src/host_gate.rs` | `tests::validate_origin_manifest_matches_script` | unit | #17 | ≥ 80% |
| `crates/ramshared-wsl2d/src/host_gate.rs` | `tests::check_guardian_health_rejects_stale` | unit | #13 | ≥ 80% |
| `crates/ramshared-wsl2d/src/host_gate.rs` | `tests::evaluate_safe_mode_refuses_foreign_boot_id` | unit | #13 | ≥ 80% |
| `crates/ramshared-wsl2d/src/host_gate.rs` | `tests::mint_lease_requires_all_gates` | unit | #17 | ≥ 80% |
| `crates/ramshared-wsl2d/src/host_gate.rs` | `tests::host_gate_shadow_comparison` | integration | #17 | ≥ 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::lease_expiry_revokes_origin_authority` | integration | #13 | ≥ 80% |
| `crates/ramshared-wsl2d/src/main.rs` | `tests::vsock_disconnect_triggers_safe_mode` | integration | #13 | ≥ 80% |
| `crates/ramshared-winsvc/src/product_online.rs` | `tests::vhdx_attach_timeout_is_bounded` | unit | #16 | ≥ 80% |
| `crates/ramshared-winsvc/src/product_online.rs` | `tests::vhdx_attach_is_idempotent` | unit | #17 | ≥ 80% |
| `crates/ramshared-winsvc/src/product_online.rs` | `tests::heartbeat_deadline_revokes_lease` | unit | #13 | ≥ 80% |

---

## Validation checklist

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p ramshared-ipc -p ramshared-wsl2d -p ramshared-winsvc`
- [ ] Slice coverage: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-ipc,ramshared-wsl2d,ramshared-winsvc --files crates/ramshared-ipc/src/lib.rs,crates/ramshared-ipc/src/vsock.rs,crates/ramshared-wsl2d/src/host_gate.rs,crates/ramshared-winsvc/src/product_online.rs --min 80`
- [ ] Live path: host-guest vsock connect → handshake → heartbeat RTT ≤ 1ms → `kill -STOP` → disconnect detection within 15s → resume → recovery
- [ ] Every matrix row has a real test name
- [ ] Kahneman critical rows have executable evidence
