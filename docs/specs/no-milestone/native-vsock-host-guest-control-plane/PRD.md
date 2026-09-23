---
slug: native-vsock-host-guest-control-plane
title: Native vsock host-guest control plane (zero scripts)
milestone: —
issues: []
---

# PRD — Native vsock host-guest control plane (zero scripts)

## 1. Summary

Replace the script-based host-guest control plane (PowerShell + Task Scheduler on Windows, Bash + embedded Python on Linux, JSON file heartbeats over 9P/Drvfs) with a native dual-binary architecture communicating over Hyper-V Sockets (AF_HYPERV on Windows, AF_VSOCK on Linux). The existing `ramshared-winsvc` (Windows NT Service) and `ramshared-wsl2d` (Linux daemon) absorb all runtime logic currently scattered across scripts, eliminating `ramshared-host-gate.sh`, `Manage-RamSharedOrigin.ps1`, `Watch-RamSharedWsl.ps1`, Task Scheduler entries, and file-based heartbeat exchanges. VHDX origin attachment becomes a service-managed operation. Result: zero scripts at runtime, sub-millisecond heartbeat latency, fail-closed on socket disconnect, and full ETW/journald observability.

## Current boundary — staged design

This PRD defines the transport, protocol, and lifecycle for the native control plane. It does NOT cover VMBus ring-buffer zero-copy (tracked in `vmbus-ring-buffer-upstream-v2`) or GPU cache worker changes (covered in `wsl2-isolated-gpu-cache-worker`). Physical 3-tier qualification after migration requires an attended validation session.

## 2. Technical context

- **Confirmed in codebase:** `crates/ramshared-winsvc/` implements a full Windows NT Service (`RamSharedWinSvc`) with Named Pipe IPC (`pipe.rs`), binary framed protocol (`ipc.rs` — magic `0x52414D53`, versioned headers, `MAX_PAYLOAD_LEN` 1MB), heartbeat/lease system (`broker_tenant.rs`), ETW/EventLog integration, and TOML config (`config.rs`).
- **Confirmed in codebase:** `crates/ramshared-winsvc/src/product_online.rs` runs a heartbeat loop (`last_heartbeat.elapsed() >= Duration::from_secs(1)`) and `HostGates` for identity validation.
- **Confirmed in codebase:** `crates/ramshared-wsl2d/src/main.rs` implements the Linux daemon (`ramsharedd`) with Unix socket IPC, sealed origin manifest at `/etc/ramshared/origin.conf`, and host manifest path `/mnt/c/ProgramData/RamShared/ramshared-origin-manifest.json`.
- **Confirmed in codebase:** `scripts/safety/ramshared-host-gate.sh` (Bash + embedded Python) validates origin manifest, guardian health, safe-mode gates, and leases — reading from `/mnt/c/ProgramData/RamShared/` (9P/Drvfs) and writing to `/run/ramshared/` and `/var/lib/ramshared/`.
- **Confirmed in codebase:** `scripts/windows/Manage-RamSharedOrigin.ps1` handles VHDX origin attachment via `wsl.exe --mount`. `scripts/windows/` contains 23 PowerShell scripts. `scripts/safety/` contains 50+ Bash scripts (runtime + CI).
- **Confirmed in codebase:** No AF_HYPERV, AF_VSOCK, or vsock code exists anywhere in the workspace. All host-guest communication uses file-based JSON over 9P/Drvfs.
- **Confirmed in codebase:** `crates/ramshared-winbroker/src/pipe.rs` implements Named Pipe with `CreateNamedPipeW`, `ConnectNamedPipe`, `ImpersonateNamedPipeClient` for Windows-local IPC.
- **Confirmed in docs:** `docs/specs/no-milestone/windows-autonomous-broker-service/` exists with PRD/SPEC/IMPL/AUDIT. `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/` exists (kernel-level, future).
- **Inference:** AF_HYPERV/AF_VSOCK provides sub-millisecond latency vs ~10-50ms for file-based JSON over 9P/Drvfs, with instant disconnect detection on VM suspend/crash.

## 3. Recommended option

Extend the existing `ramshared-winsvc` and `ramshared-wsl2d` binaries with a new vsock transport module, backed by a shared `ramshared-ipc` protocol crate. No new binaries.

### Architecture

```
┌──────────────────────────────────┐         ┌──────────────────────────────────┐
│          WINDOWS HOST            │         │           WSL2 GUEST             │
│                                  │  vsock  │                                  │
│  ramshared-winsvc (NT Service)   │◄───────►│  ramshared-wsl2d (daemon)        │
│  + AF_HYPERV listener            │ AF_HYPERV│  + AF_VSOCK client              │
│  + VHDX lifecycle (wsl.exe mount)│  AF_VSOCK│  + origin manifest validation    │
│  + ETW telemetry                 │         │  + lease/heartbeat over vsock     │
│  + Guardian monitoring           │         │  + journald telemetry            │
│                                  │         │                                  │
│  [Eliminated: Watch-RamShared,   │         │  [Eliminated: ramshared-host-    │
│   Task Scheduler, JSON heartbeats│         │   gate.sh, Python one-liners,    │
│   in C:\wsl-forensics\,          │         │   file-based leases in /run/     │
│   Manage-RamSharedOrigin.ps1]    │         │   and /mnt/c/...]                │
└──────────────────────────────────┘         └──────────────────────────────────┘
              │                                          │
              └────────── ramshared-ipc (shared) ────────┘
                    binary framed protocol, RAMS magic,
                    versioned, bounded payloads
```

### Discarded alternatives

1. *Keep script-based file heartbeats (9P/Drvfs JSON):* Rejected. Latency ~10-50ms per heartbeat, disk I/O on every exchange, no instant disconnect detection on VM suspend, fragile file locking, and deadlocks when `wsl.exe` is called from inside WSL. This is the current state being replaced.
2. *TCP sockets over WSL2 virtual network:* Rejected. Adds full TCP/IP stack overhead (~100-500µs), requires port management and firewall rules, and does not provide the instant-disconnect semantics of AF_HYPERV when the VM suspends or crashes.
3. *Shared memory (IVSHMEM) / virtio-vsock custom driver:* Rejected for Day-1. Requires kernel module or custom driver in the WSL2 guest kernel, which is outside the current upstreaming scope. AF_HYPERV/AF_VSOCK is natively available without custom drivers.
4. *New separate `ramshared-guest-bridge` binary:* Rejected. Adds deployment complexity and version skew risk. Extending the existing daemon (`ramshared-wsl2d`) keeps a single guest binary with atomic updates.
5. *Named Pipes over 9P (existing `ramshared-winbroker` transport):* Rejected for host-guest. Named Pipes are Windows-local IPC; the current file-based JSON over 9P is the slow path being eliminated. AF_HYPERV is the native cross-boundary transport.

## 4. Functional requirements (RF)

- **RF-1 (Native Transport):** Host-guest communication must use AF_HYPERV (Windows) / AF_VSOCK (Linux) stream sockets with a well-known Hyper-V socket GUID. No file-based JSON, no 9P/Drvfs dependency for runtime control plane.
- **RF-2 (Shared Protocol):** Both sides must use a shared Rust protocol crate (`ramshared-ipc`) with binary framing (magic `0x52414D53`, versioned headers, bounded payloads ≤ 1MB). Protocol messages cover: Handshake, Heartbeat, LeaseRequest/Granted/Denied/Release, OriginManifest, SafeModeGate, GuardianHealth, VHDXAttach/Detach, Telemetry, Shutdown.
- **RF-3 (Heartbeat & Lease):** The guest daemon sends heartbeat frames at a configurable interval (default 5s). The host service tracks liveness and revokes the lease if no heartbeat arrives within 3× the interval. Lease revocation triggers fail-closed origin-only mode in the guest.
- **RF-4 (VHDX Lifecycle):** The host service manages VHDX origin attachment via `wsl.exe --mount --vhd <path> --bare` (or equivalent WSL2 API) invoked programmatically. The guest daemon validates PartUUID and seals the origin manifest upon attach notification. No user PowerShell required.
- **RF-5 (Script Absorption):** All runtime logic in `ramshared-host-gate.sh` (origin manifest validation, guardian health check, safe-mode gate, lease minting) must be absorbed into `ramshared-wsl2d`. All runtime logic in `Manage-RamSharedOrigin.ps1` and `Watch-RamSharedWsl.ps1` must be absorbed into `ramshared-winsvc`. After migration, zero runtime scripts execute in the control plane path.
- **RF-6 (Fail-Closed Disconnect):** On vsock disconnect (VM suspend, crash, network partition), both sides must transition to fail-closed state within 1 heartbeat interval. The guest must revoke origin authority and fall back to safe mode. The host must trigger guardian isolation.
- **RF-7 (Observability):** Host service emits ETW events for all state transitions. Guest daemon emits journald structured logs. Both sides expose a `status` JSON endpoint for CLI inspection.

## 5. Non-functional requirements (NFR)

- **NFR-1 (Latency):** Heartbeat round-trip must complete in ≤ 1ms (vs ~10-50ms for file-based JSON over 9P). Disconnect detection must occur within 3× heartbeat interval (default 15s).
- **NFR-2 (Zero Scripts at Runtime):** After migration, no `.ps1`, `.sh`, or Python process may execute in the host-guest control plane path. CI/build scripts are exempt.
- **NFR-3 (Atomic Updates):** Guest daemon binary updates must be atomic (rename-based). Protocol version negotiation must support rolling upgrades (N and N-1).
- **NFR-4 (Host Safety):** VHDX attach/detach operations must never corrupt existing host disk state. Operations are idempotent and bounded (≤ 10s timeout).
- **NFR-5 (Security):** vsock connections must be authenticated via shared secret or GUID-based allowlisting. No unauthenticated origin authority minting. All frames bounded to 1MB.

## 6. Execution flows

### 6.1 Happy Path — Boot, Handshake, Active Operation
1. Windows boots; `ramshared-winsvc` starts and opens AF_HYPERV listener on well-known GUID.
2. WSL2 starts; `ramshared-wsl2d` starts and connects via AF_VSOCK to host GUID.
3. Guest sends `Handshake` with protocol version, boot_id, and distro identity.
4. Host validates identity, sends `HandshakeAck` with lease parameters and origin manifest.
5. Guest validates origin manifest (replacing `ramshared-host-gate.sh` logic), seals `/etc/ramshared/origin.conf`.
6. Host sends `VHDXAttach` with origin VHDX path; guest validates PartUUID and confirms.
7. Guest begins `Heartbeat` at configured interval. Host rewrites lease deadline on each heartbeat.
8. Cascade activates: ZRAM + VRAM + SSD swap tiers come online.
9. On shutdown: guest sends `Shutdown`; host sends `VHDXDetach`; both sides clean up.

### 6.2 Error Path — VM Suspend or Crash
1. WSL2 VM suspends or crashes.
2. vsock connection breaks (kernel detects Hyper-V socket disconnect).
3. Host detects disconnect within 1 heartbeat interval → emits ETW event → triggers guardian isolation.
4. Guest (on resume/restart) finds lease expired → revokes origin authority → enters safe mode.
5. On next boot: guest reconnects, revalidates, reacquires lease.

### 6.3 Error Path — Host Service Crash
1. `ramshared-winsvc` crashes or is terminated.
2. vsock connection breaks.
3. Guest detects disconnect within 1 heartbeat interval → revokes lease → enters safe mode.
4. Host service restarts → reopens AF_HYPERV listener → guest reconnects.

## 7. Data and state model

```text
┌─────────────────────────────────────────────┐
│           Control Plane State Machine        │
└──────────────────────┬──────────────────────┘
                       │
              [vsock connected]
                       ▼
                 ┌───────────┐
                 │ HANDSHAKE │
                 └─────┬─────┘
                       │ [HandshakeAck received]
                       ▼
                 ┌───────────┐
                 │   LEASED  │◄────┐
                 └─────┬─────┘     │ [heartbeat within deadline]
                       │           │
                       ├───────────┘
                       │
         [disconnect / lease expired]
                       ▼
                 ┌───────────┐
                 │ SAFE_MODE │ (fail-closed, origin-only)
                 └─────┬─────┘
                       │ [reconnect + revalidate]
                       ▼
                 ┌───────────┐
                 │  RECOVER  │
                 └───────────┘
```

- vsock frame header (extends existing `IpcMessageHeader`):
  - `magic`: `u32` (0x52414D53)
  - `version`: `u32` (3 for vsock transport)
  - `payload_len`: `u32` (bounded to 1MB)
  - `flags`: `u32` (message type + control bits)
  - `correlation_id`: `u64` (request-response matching)

- Message types (flags lower 8 bits):
  - 1 = Handshake, 2 = HandshakeAck
  - 3 = Heartbeat, 4 = HeartbeatAck
  - 5 = LeaseRequest, 6 = LeaseGranted, 7 = LeaseDenied, 8 = LeaseRelease
  - 9 = OriginManifest, 10 = OriginManifestAck
  - 11 = SafeModeGate, 12 = SafeModeGateAck
  - 13 = GuardianHealth, 14 = GuardianHealthAck
  - 15 = VHDXAttach, 16 = VHDXAttachAck
  - 17 = VHDXDetach, 18 = VHDXDetachAck
  - 19 = Telemetry
  - 20 = Shutdown, 21 = ShutdownAck

## 8. Interfaces

- **Host listener:** AF_HYPERV stream socket on RamShared well-known service GUID (redacted in public docs; stored in `winsvc.toml`).
- **Guest client:** AF_VSOCK stream socket to VMADDR_CID_HOST (CID 2) on the service port.
- **Protocol:** `ramshared-ipc` crate — shared between `ramshared-winsvc` and `ramshared-wsl2d`.
- **Config:** `winsvc.toml` (host) and `/etc/ramshared/config.toml` (guest) — `vsock_guid`, `heartbeat_secs`, `lease_timeout_secs`.
- **Telemetry:** ETW provider `RamShared-ControlPlane` (host), journald structured logs (guest).
- **CLI:** `ramshared status --json` reads guest daemon status over Unix socket (unchanged).

## 9. Dependencies and risks

- **Prerequisites:** Hyper-V socket support in WSL2 kernel (standard since WSL2 kernel 5.10+). `windows-rs` crate for AF_HYPERV. `vsock` crate (or raw `libc`) for AF_VSOCK.
- **Risks:**
  - WSL2 kernel config may lack `CONFIG_VSOCKETS` / `CONFIG_VSOCKETS_DIAG`. *Mitigation:* runtime detection with fallback to file-based heartbeat (deprecated path).
  - AF_HYPERV GUID registration may conflict with other Hyper-V services. *Mitigation:* use RamShared-specific GUID, validate at startup.
  - Script absorption may miss edge cases in `ramshared-host-gate.sh` (Python manifest validation). *Mitigation:* port all validation logic to Rust with equivalent tests; run shadow comparison before cutover.
- **Rollback trigger:** Any heartbeat RTT > 10ms sustained for > 60s, or any data loss on vsock stream, or any failure to detect disconnect within 3× heartbeat interval.

## 10. Implementation strategy

1. **Slice 1:** Create `crates/ramshared-ipc` with vsock transport abstraction and extended framed protocol. Unit tests with socketpair mock.
2. **Slice 2:** Extend `ramshared-wsl2d` with AF_VSOCK client, absorbing `ramshared-host-gate.sh` logic (origin manifest validation, guardian health, safe-mode gate, lease). Shadow mode: run alongside script, compare results.
3. **Slice 3:** Extend `ramshared-winsvc` with AF_HYPERV listener, VHDX lifecycle via `wsl.exe --mount`, and heartbeat monitoring. Absorb `Manage-RamSharedOrigin.ps1` and `Watch-RamSharedWsl.ps1`.
4. **Slice 4:** Cut over: disable script-based path, enable vsock-only. Remove Task Scheduler entries and file-based heartbeat code.
5. **Slice 5:** Cleanup: deprecate `ramshared-host-gate.sh`, `Manage-RamSharedOrigin.ps1`, `Watch-RamSharedWsl.ps1`, and file-based heartbeat JSON. Update packaging scripts.

## 11. Documents to update

- `docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md`
- `docs/specs/no-milestone/native-vsock-host-guest-control-plane/AUDIT-2.5.md`
- `docs/specs/no-milestone/native-vsock-host-guest-control-plane/IMPL.md`
- `docs/reliability/GAP-REGISTER.md`
- `validation.md`
- `trovaldo.md`
- `scripts/package/build-deb-package.sh` / `build-linux-bundle.sh` (remove absorbed scripts)
- `README.md` (architecture section)

## 12. Out of scope

- VMBus ring-buffer zero-copy (tracked in `vmbus-ring-buffer-upstream-v2`).
- GPU cache worker changes (tracked in `wsl2-isolated-gpu-cache-worker`).
- Windows driver (WDF/StorPort) development (tracked in `windows-storport-cuda-vram`).
- CI/build scripts (`.sh` in `scripts/package/`, `.ps1` in `scripts/p0/` for benchmarks).
- Multi-distro WSL2 support (single distro `Ubuntu-24.04` is the Day-0 target).

## 13. Acceptance criteria

- Unit test coverage ≥ 80% on `ramshared-ipc`, vsock transport modules, and absorbed gate logic.
- Heartbeat RTT measured ≤ 1ms (p99) over vsock, vs baseline file-based measurement.
- Disconnect detection within 3× heartbeat interval on `kill -STOP` of WSL2 VM.
- `ramshared-host-gate.sh` shadow comparison: 100% match on 1000+ origin manifest validation runs.
- Zero runtime scripts executing in control plane path (verified via `strace`/`dtrace` audit).
- ETW events emitted for all state transitions (handshake, lease grant/revoke, VHDX attach/detach, disconnect).
- `ramshared status --json` shows `control_plane: vsock` and `heartbeat_rtt_us` metric.

## 14. Validation plan

- **Unit:** `cargo test -p ramshared-ipc` — protocol framing, message types, version negotiation.
- **Unit:** `cargo test -p ramshared-wsl2d` — absorbed gate logic (origin manifest, guardian health, safe-mode).
- **Unit:** `cargo test -p ramshared-winsvc` — AF_HYPERV listener, VHDX lifecycle, heartbeat monitoring.
- **Integration:** Socketpair end-to-end handshake → heartbeat → lease → disconnect → fail-closed.
- **Live path:** Host-guest boot → vsock connect → cascade up → heartbeat RTT measurement → `kill -STOP` → disconnect detection → resume → recovery.
- **Shadow:** Run `ramshared-host-gate.sh` alongside absorbed Rust logic for 1000+ iterations, compare outputs.
- **Env-bound gaps:** Real AF_HYPERV between Windows host and WSL2 guest requires physical host — marked as env-bound in IMPL.
