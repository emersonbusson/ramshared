---
slug: windows-autonomous-broker-service
title: Autonomous Windows broker service packaging and supervision
milestone: —
issues:
  - 156
---

# PRD — Autonomous Windows broker service packaging and supervision

## 1. Summary

**Change:** Package and supervise the supported RamShared broker/control-plane dependency as part of the machine-scoped Windows product, instead of relying on a temporary PowerShell lease broker started by a test harness.

**Outcome:** After one supported installation and a cold boot, an administrator can start the RamShared Windows product through SCM without an interactive session or lab script. The broker becomes ready before `RamSharedWinSvc` provisions a LUN, both services expose actionable health, and stop, crash recovery, upgrade, rollback, and uninstall preserve the established safe teardown contract.

**Layers:**

- [x] userspace crates
- [x] `ramsharedd` / cascade CLI
- [ ] LKM / Linux kernel
- [ ] uAPI / sysfs / ioctl
- [x] Windows lab / WDK / winsvc
- [x] safety or P0 script
- [x] ADR / runbook
- [ ] P0 benchmark gate

This PRD is SSDV3 Step 1 only. It defines the product requirement and its proof boundary; the SPEC must decide the exact executable composition and SCM topology. Windows production signing and Secure Boot compatibility are separate release gates and are not silently claimed by this feature.

## 2. Technical context

### Confirmed state

| Fact | Classification | Consequence |
| --- | --- | --- |
| `ramshared-winsvc` reads a broker socket address from `winsvc.toml`; the example is `127.0.0.1:7700`. | Confirmed in codebase | A live broker is a startup dependency, but the current loopback socket is not yet a hardened local product boundary. |
| The Rust service installer and `ramshared-winsvc install` register only `RamSharedWinSvc`, as demand-start, with no SCM dependencies. | Confirmed in codebase | Installing the current service does not create an autonomous product. |
| Physical Test Mode campaigns have passed Online CUDA I/O, three checksum rounds, graceful stop, lease release, CUDA restoration, and residue checks. | Confirmed in docs | The storage path is an input to this work, not a substitute for broker lifecycle proof. |
| Guest and physical campaign harnesses create a minimal PowerShell JSONL lease broker for the duration of a run. | Confirmed in codebase | Existing greens prove protocol use but not product broker packaging or supervision. |
| The supported broker implementation and protocol live in Rust crates, and the daemon currently exposes the arbiter with `--slices` plus `--arbiter-listen`. | Confirmed in codebase | Reuse is required; the lab broker must not become the shipped service. |
| The gap register marks the unattended Windows daily service as `PARTIAL` until the broker is packaged and three cold-boot lifecycles pass. | Confirmed in docs | This PRD owns that gap and must not mark it closed from unit tests alone. |
| The current driver path requires Test Mode until a production-signing route is completed. | Confirmed in docs | Autonomous service support can be validated in Test Mode, but normal Secure Boot support remains external to this PRD. |
| SCM dependency ordering alone may not prove application-level broker readiness. | Inference | A bounded health/readiness handshake is required in addition to process ordering. |
| A fixed loopback TCP port does not by itself authenticate the peer and can be occupied by an unrelated local process. | Inference | The Windows product requires OS-authenticated local IPC; loopback TCP alone is insufficient for Day-0 daily mode. |

### Paths in scope

- `crates/ramshared-broker/`: protocol, lease model, state, and reusable broker behavior.
- `crates/ramshared-wsl2d/` or a new Windows product composition crate selected by the SPEC: supported broker runtime reuse.
- `crates/ramshared-winsvc/`: readiness, dependency health, fail-closed startup, and coordinated lifecycle.
- `scripts/windows/Install-RamSharedService.ps1` and the Rust installer: atomic product installation, upgrade, rollback, and uninstall.
- `scripts/windows/`: static checks and VM/physical lifecycle campaigns.
- `docs/reliability/GAP-REGISTER.md`, Windows runbooks, release documentation, and the degradation matrix.

### Exists, extend, create

- **Exists:** broker protocol/model, Windows broker client, CUDA-backed product runtime, SCM service, safe storage teardown, BINARY_MATCH and residue evidence.
- **Extend:** machine-scoped packaging, SCM registration, readiness/health, ordered stop, failure recovery, diagnostics, and lifecycle harnesses.
- **Create if required by the SPEC:** a Windows-native broker service entry point that reuses existing broker code without importing Linux NBD/ublk lifecycle.

### Abuse and catastrophic cases

- An unprivileged user replaces a service binary/configuration or redirects the broker connection.
- An unrelated local process impersonates either peer, pre-creates the endpoint, or sends oversized/malformed protocol frames.
- SCM starts `RamSharedWinSvc` while the broker process exists but is not ready.
- The broker exits while the LUN is Online or while I/O/teardown is in progress.
- Failure actions create a restart storm or create two broker/service owners.
- Upgrade/uninstall removes the broker before the LUN, queue, lease, or pagefile state is safe.
- A package accidentally installs or executes `Lab-LeaseBroker.ps1`.
- A stale service, binary, lease, LUN, PnP node, or configuration survives rollback.

## 3. Recommended option

Ship one machine-scoped Windows product unit containing the signed-or-test-signed driver package, `ramshared-winsvc`, one supported Rust broker runtime, configuration, and lifecycle tooling. Register the broker as a dedicated SCM service and express `RamSharedWinSvc` as its dependent consumer. Use an OS-authenticated, machine-local IPC transport—preferably a Windows named pipe protected by service-SID ACLs—and require an application-level readiness probe before any lease request or `CREATE_DISK`.

The broker service owns only broker/control-plane state. `RamSharedWinSvc` remains the sole owner of CUDA allocation, the StorPort queue, LUN exposure, I/O completion, and storage teardown. Product stop and uninstall therefore proceed consumer-first: safely stop `RamSharedWinSvc`, verify lease/LUN/queue cleanup, and only then stop or remove the broker.

The default installed mode remains **demand-start** until the three cold-boot acceptance lifecycles pass. An unattended automatic-start mode may be promoted only by the release gate defined here; installation must never bring a new LUN Online without an explicit operator choice.

### Why this option

- It reuses the accepted Rust protocol and ownership boundaries.
- SCM plus Windows local IPC provides machine boot integration, peer identity, access control, dependency metadata, failure actions, and event reporting.
- A distinct broker service can remain healthy and diagnosable without transferring storage ownership away from `ramshared-winsvc`.
- Demand-start by default avoids surprising disk/GPU allocation during installation while still allowing an explicit daily-use policy.

### Discarded alternatives

| Alternative | Reason discarded |
| --- | --- |
| Ship the PowerShell lab broker | It is a protocol test double, has no product lifecycle or stable state model, and can create false-green evidence. |
| Let `ramshared-winsvc` spawn an unsupervised child broker | SCM cannot independently observe, order, recover, or uninstall the dependency; child lifetime can outlive partial failures. |
| Depend on an interactive WSL2 broker | SCM startup must work before user login and must not depend on a mutable WSL distribution session. |
| Keep a fixed unauthenticated `127.0.0.1:7700` socket as the daily Windows boundary | Loopback prevents remote access but does not prove which local process owns either end; endpoint squatting or unintended local clients remain possible. |
| Add automatic RAM/file fallback when the broker is unavailable | It hides a product dependency failure and violates the CUDA-only Day-0 path. |
| Enable unconditional automatic restart of both services | Restarting storage ownership without proving the LUN/pagefile state can corrupt data or create ghost devices. |

### Trade-offs

A second SCM service adds packaging and lifecycle surface. Readiness checking delays start and the safest broker-loss behavior may leave the storage service in `FailedSafe` pending an operator-directed stop. These costs are preferable to hidden fallback, automatic destructive recovery, or a false autonomous-product claim.

## 4. RF-N

### RF-1 — Package one supported broker runtime

The Windows product package must install a Rust broker runtime built from the same source tree and release commit as `ramshared-winsvc`. It must not copy, generate, or execute the PowerShell lab broker.

**Acceptance:** the installed manifest records package version and SHA-256 for the broker, service, driver catalog/package, and configuration template. Each installed binary matches the release manifest, and a static test fails on references to `Lab-LeaseBroker.ps1` in product service paths.

**Abuse note:** binaries and mutable configuration are writable only by `SYSTEM` and Builtin Administrators; ordinary users receive at most read/execute access.

### RF-2 — Register explicit SCM ownership and ordering

Installation must register a dedicated broker service and `RamSharedWinSvc` with stable names, absolute quoted image paths, LocalSystem identity, documented start types, and an explicit dependency from the consumer to the broker.

**Acceptance:** an SCM query and registry inspection prove exact image paths, accounts, start modes, dependency ordering, failure actions, and binary hashes. Starting `RamSharedWinSvc` from a cold `Stopped` state starts the broker first without an interactive login.

**Abuse note:** service names and paths are constants or validated installer inputs; no user-writable directory may appear in `ImagePath` or DLL search state.

### RF-3 — Gate provisioning on broker readiness

The broker must expose a bounded local readiness operation over OS-authenticated local IPC that proves protocol version compatibility, server identity, and the ability to register the configured tenant. The broker must admit only the configured consumer service identity. `ramshared-winsvc` must complete that operation before lease acquisition and before `CREATE_DISK`.

**Acceptance:** healthy startup reaches broker-ready within 30 seconds. Missing process, refused connection, protocol mismatch, malformed reply, or timeout leaves no lease, CUDA allocation, queue registration, or LUN and reports one stable failure class.

**Abuse note:** daily mode uses a machine-local endpoint with an explicit ACL for `SYSTEM`, Administrators, and the configured service SID as narrowly required. It caps each protocol frame at 64 KiB and rejects an unauthorized peer, unknown version, or unknown message before state mutation. TCP listening is disabled in daily mode.

### RF-4 — Coordinate safe start and stop

Start must follow broker-ready → lease → CUDA allocation → disk/queue exposure. Stop, upgrade, rollback, and uninstall must follow stop new work → pagefile safety gate → drain → unregister → destroy → wipe/free CUDA → release lease → stop broker.

**Acceptance:** a clean SCM stop completes within 30 seconds without force-kill, releases the lease, restores CUDA capacity within the existing product tolerance, and leaves no RamShared LUN, volume, `Win32_DiskDrive`, or PnP residue. Repeated start/stop requests are idempotent.

**Abuse note:** if a RamShared pagefile is active or safe teardown is not proven, the operation refuses before unregister/destroy/broker stop and preserves recoverable state.

### RF-5 — Contain broker and consumer failures

The supervisor policy must distinguish pre-exposure broker failure from broker loss after the LUN is Online. Pre-exposure failure is fail-closed. Post-exposure loss must make health visibly degraded, reject unsafe new lifecycle mutations, and preserve `ramshared-winsvc` ownership until a bounded safe teardown or documented operator recovery succeeds.

**Acceptance:** VM drills cover broker exit before readiness, broker exit after Online, service exit, simultaneous stop requests, and reboot from a clean stopped state. None produces a BugCheck, checksum mismatch, duplicate owner, forced removal, or hidden RAM/file fallback.

**Abuse note:** deterministic failures are not retried. SCM performs at most three restarts in 60 seconds, only for SPEC-enumerated transient pre-exposure failures, and then remains stopped/failed for diagnosis.

### RF-6 — Provide machine-local health and diagnostics

Operators must be able to distinguish `BrokerStopped`, `BrokerStarting`, `BrokerReady`, `ConsumerStarting`, `Online`, `Degraded`, `Stopping`, `FailedSafe`, and `Stopped`, including the last stable failure code and ownership state.

**Acceptance:** a supported status command plus Windows Event Log/JSONL evidence reports service identities, package hashes, broker endpoint/protocol, tenant/lease identity, LUN identity, CUDA allocation, last transition, and residue summary without requiring PowerShell process inference.

**Abuse note:** logs omit payload data, secrets, raw kernel/user addresses, and reusable privileged handles.

### RF-7 — Make install, upgrade, rollback, and uninstall transactional

Install and upgrade must stage and verify the complete product unit before changing SCM registrations. Rollback must restore the previous complete manifest, not mix broker/service/driver versions. Uninstall must safely stop the consumer, confirm cleanup, stop the broker, remove both services, and then remove only manifest-owned files.

**Acceptance:** fresh install, same-version repair, one-version upgrade, failed-upgrade rollback, and uninstall are repeatable. Any failure returns a non-zero code with the last completed phase and leaves either the old complete version operational or both services safely stopped; no mixed manifest is accepted.

**Abuse note:** uninstall refuses destructive cleanup when an active pagefile, lease, LUN, queue, or unresolved ownership state remains.

### RF-8 — Separate daily-use policy from installation

The installer must expose a documented, explicit choice between demand-start and unattended automatic start. Automatic start is unavailable for promotion until this PRD's cold-boot gate is satisfied on the supported matrix.

**Acceptance:** installation performs no implicit disk creation. The selected policy is machine-readable and preserved across repair/upgrade. Switching policy does not start or stop an Online device outside the normal safe lifecycle.

## 5. NFR-N

### NFR-1 — Host safety

- No pressure generator, pagefile activation, surprise removal, GPU reset, or forced process kill is authorized by this feature.
- Live-host lifecycle tests use the Windows watchdog harness, finite waits, telemetry, and cleanup artifacts.
- Broker readiness budget is 30 seconds; clean service teardown budget is 30 seconds; complete product stop budget is 45 seconds.
- Any new BugCheck/dump, checksum mismatch, active pagefile teardown attempt, forced kill with an Online LUN, or unexplained residue is an immediate rollback trigger.

### NFR-2 — Security

- Each service receives a stable Windows service SID. The SPEC must minimize privileges and document whether the broker can run under a virtual service account instead of LocalSystem.
- Broker daily mode uses a protected machine-local IPC namespace and creates no TCP listener or firewall opening.
- The broker authenticates the connecting service identity through Windows access checks; filesystem ACLs and a protocol tenant string are not substitutes for peer authentication.
- Product directories use protected ACLs; binary/config hashes are verified at install and evidence time.
- Protocol parsing is bounded and fail-closed. No secret is stored in command lines, logs, or the repository.
- Production signing and Secure Boot trust remain required before describing this as normal-Windows support.

### NFR-3 — Reliability and resilience

- One command issued twice must have the same safe outcome as once for install repair, start, stop, status, and uninstall refusal.
- Restart is bounded as specified by RF-5 and may not cross a storage-ownership ambiguity.
- Service or broker exit must never be reported as healthy merely because the SCM process is running.
- The final acceptance set requires three consecutive cold-boot lifecycles with no cleanup between runs other than the supported product stop.

### NFR-4 — Observability

Each lifecycle emits a run ID and structured records with timestamp, package/commit/binary hashes, OS build, GPU/driver identity, SCM state, broker state, endpoint/protocol version, tenant/lease, LUN serial/size, CUDA bytes, transition duration in milliseconds, retry count, stable error class, stop phase, and residue result.

Required counters are starts, ready successes/failures, bounded retries, lease grants/releases, Online transitions, graceful stops, teardown refusals, forced kills, broker disconnects, checksum mismatches, and residual identities. Raw artifacts remain auditable and contain no payload or virtual addresses.

### NFR-5 — Compatibility

Day-0 support is limited to the already allow-listed x64 Windows 11/WDK/NVIDIA/Test Mode matrix until separate compatibility and production-signing gates pass. Broker protocol compatibility is exact and versioned; no older protocol shim or alternate storage backend is introduced.

### NFR-6 — Performance

This slice claims lifecycle latency, not storage throughput. Record three samples for broker readiness and full start/stop using milliseconds, with median, p99, and deviation. Regression rollback triggers are readiness over 30 seconds, clean consumer stop over 30 seconds, or complete product stop over 45 seconds on the fixed acceptance host.

## 6. Flows

### Happy flow

1. **Installer:** verify elevation, package manifest, hashes, supported platform, paths, and ACLs.
2. **Installer:** atomically stage broker/service/config and register both SCM services with the selected demand/automatic policy.
3. **SCM:** start the broker service without a user session.
4. **Broker:** create the protected local IPC endpoint, initialize state, and report protocol-ready.
5. **`RamSharedWinSvc`:** pass readiness, acquire the lease, allocate CUDA VRAM, and expose the LUN.
6. **Operator/workload:** use the volume while status reports matching broker, lease, LUN, CUDA, and package identities.
7. **SCM stop:** stop `RamSharedWinSvc`; it drains, destroys, wipes/frees, and releases.
8. **SCM/product control:** after cleanup proof, stop the broker and report zero residue.

### Alternate flows

- **Demand-start:** the product remains inert after boot until the operator starts the consumer; SCM starts its broker dependency.
- **Automatic-start:** after release promotion, SCM starts the broker and consumer at boot with no login.
- **Broker already ready:** readiness is idempotent and reuses the single configured instance.
- **Repair/upgrade:** safely stop, stage a complete manifest, switch registrations, validate, and retain a rollback manifest.
- **Unsafe stop:** keep ownership intact and report the exact pagefile/teardown blocker.

### Error contract

| Trigger | Installer/CLI exit or SCM result | Required log | Resulting state |
| --- | --- | --- | --- |
| Invalid manifest, hash, path, ACL, or config | 2 | `package_refused`, field/artifact, expected/observed | Existing complete version unchanged or `Stopped` |
| Broker cannot create its IPC endpoint or become ready in 30 s | 3 / service-specific failure | `broker_not_ready`, endpoint, protocol, elapsed, retry count | Both services stopped; no lease/LUN |
| Unauthorized or unexpected local peer | 4 | `broker_peer_refused`, authenticated identity, endpoint; no secrets | No protocol state mutation; offending connection closed |
| Protocol mismatch or malformed frame | 4 | `broker_protocol_refused`, expected/observed version, bounded detail | `FailedSafe`; no provisioning |
| Lease/CUDA/driver-link failure after readiness | Existing product code, surfaced unchanged | Existing stable product error plus broker identity | Reverse unwind; broker may remain ready, no Online claim |
| Broker disconnect after Online | 5 | `broker_lost_online`, lease/LUN/ownership, last heartbeat | `Degraded` or `FailedSafe`; consumer retains ownership pending safe stop |
| Active pagefile or teardown ambiguity | 7 | existing teardown refusal plus broker state | Online/recoverable state preserved; broker not stopped |
| Restart budget exhausted | 8 | `restart_budget_exhausted`, count/window/root error | Failed service remains stopped |
| Clean operation | 0 | lifecycle summary and zero-residue proof | `Online` while running, then `Stopped` |

## 7. Data / state model

```text
InstalledProduct
├─ ProductManifest
│  ├─ version, commit
│  ├─ broker_binary { path, sha256 }
│  ├─ winsvc_binary { path, sha256 }
│  ├─ driver_package { identity, sha256 }
│  └─ config { path, sha256, start_policy }
├─ BrokerService
│  ├─ scm_state
│  ├─ ipc_endpoint, protocol_version, allowed_service_sid
│  └─ BrokerState: Stopped → Starting → Ready → Stopping
└─ ConsumerService
   ├─ scm_state
   ├─ tenant, lease
   ├─ cuda_allocation
   ├─ queue, lun
   └─ ProductState:
      Stopped → WaitingForBroker → Provisioning → Online
      Online → Degraded | Stopping
      Stopping → Stopped
      any bounded failure → FailedSafe
```

Ownership invariants:

1. `Online` implies broker identity known, a lease held, CUDA allocation live, queue registered, and one LUN owner.
2. Broker readiness does not imply consumer health.
3. The broker may stop only when the consumer is `Stopped` and lease/LUN/queue residue is zero.
4. A mixed-version manifest is never runnable.
5. Failure state is persistent and observable until an explicit safe recovery or successful clean start.

No kernel ABI, IOCTL, queue structure, or disk identity changes are authorized by this PRD.

## 8. Interfaces

### SCM

- Stable broker service name selected by the SPEC; stable consumer name remains `RamSharedWinSvc`.
- Consumer depends on broker.
- Absolute quoted image paths; no shell, script host, relative path, or user profile.
- Demand-start is the installation default; delayed automatic start is an explicit post-gate policy.
- Failure actions are bounded and apply only to SPEC-enumerated pre-exposure transient failures.

### Product control

- `install`: stage and register a complete manifest; idempotent repair for the same version.
- `status`: return manifest identity, both SCM states, broker readiness, lease/LUN/CUDA ownership, and residue.
- `start`: request the ordered product lifecycle.
- `stop`: request safe consumer-first teardown.
- `uninstall`: refuse unless safe cleanup succeeds; remove manifest-owned artifacts only.
- Every command has a finite timeout and stable non-zero errors from the flow contract.

### Broker

- Preferred Day-0 transport: Windows named pipe in a machine-local namespace, with an explicit security descriptor and service-SID peer restriction.
- Existing versioned message semantics and codec are reused over a transport abstraction unless the SPEC proves a blocking contract defect.
- `127.0.0.1:7700` may remain a lab or cross-platform test transport, but it is disabled by the installed daily Windows profile.
- Readiness must be side-effect-free or use an idempotent capability exchange; it must not allocate a lease.
- Frame size is capped at 64 KiB; tenant and capacity bounds remain enforced.

### Configuration

Machine-scoped configuration records the IPC endpoint identity, expected service SID, tenant, service start policy, readiness/stop budgets, evidence directory, and existing CUDA/disk settings. Installer validation rejects arbitrary endpoint namespaces, unknown privileged path substitutions, and contradictory service policies.

## 9. Dependencies and risks

| Risk / dependency | Mitigation and rollout gate |
| --- | --- |
| Existing broker daemon composition includes Linux transport concerns. | SPEC discovery must choose a minimal Windows entry point that reuses broker core without pretending Linux ublk/NBD exists on Windows. |
| SCM process state can be healthy while the protocol is unavailable. | Require RF-3 application readiness and heartbeat evidence. |
| Named-pipe ACL or service-SID configuration can lock out the real consumer or admit an unintended principal. | Generate the security descriptor from stable service identities and test both legitimate access and explicit refusal. |
| Broker loss after Online creates lease-state ambiguity. | Preserve storage ownership, enter `Degraded`/`FailedSafe`, and allow only safe teardown/recovery operations. |
| Restart actions can race teardown or duplicate ownership. | Limit restarts to pre-exposure transient failures; enforce single-instance/manifest locks and three-in-60-second budget. |
| Upgrade can mix incompatible broker/service binaries. | Atomic complete-manifest staging, BINARY_MATCH, version handshake, rollback manifest. |
| Automatic boot allocation surprises the operator or conflicts with anti-cheat/Secure Boot. | Demand-start default, explicit opt-in, and no normal-Windows claim until production signing passes separately. |
| Physical-host lifecycle testing can disrupt daily workloads. | VM-first validation; physical runs use watchdog, finite budgets, explicit supervision, and clean terminal proof. |

### Numeric rollback triggers

Roll back the candidate and retain the last complete manifest on any of:

- one new BugCheck or dump;
- one checksum mismatch or BINARY_MATCH failure;
- one forced kill/removal while a LUN, queue, lease, or pagefile is active;
- any LUN/volume/`Win32_DiskDrive`/PnP/lease residue 30 seconds after a reported clean stop;
- broker readiness over 30 seconds, consumer clean stop over 30 seconds, or product stop over 45 seconds;
- more than three restart attempts in 60 seconds;
- one TCP listener in the installed daily profile, unauthorized IPC admission, or writable-by-users product binary/config;
- one mixed broker/service/driver manifest.

## 10. Implementation strategy

1. **Contract discovery:** inventory broker-core portability, current protocol readiness semantics, SCM library capabilities, installer ownership, and exact failure codes. Close choices in `SPEC.md`.
2. **Broker service seam:** add the minimal Windows service entry point and local-IPC transport around existing broker core; prove service-SID admission/refusal, readiness, shutdown, malformed-frame refusal, and single-instance ownership without a driver.
3. **Product packaging:** build a complete manifest, protected directories, atomic staging, exact SCM registrations, BINARY_MATCH, repair, and rollback.
4. **Consumer coordination:** add readiness gating, state/health reporting, consumer-first stop, and bounded failure policy while preserving the existing storage teardown.
5. **VM lifecycle proof:** exercise install, cold boot, start, I/O, broker loss, safe stop, upgrade/rollback, and uninstall in `win11-drill`.
6. **Physical Test Mode proof:** only after VM gates pass, run three supervised cold-boot lifecycles on the allow-listed host and consolidate evidence.
7. **Promotion:** change the gap register from `PARTIAL` only when every acceptance criterion is backed by one auditable evidence set. Automatic start and normal-Windows support are separate promotion decisions.

Early validation is deliberately broker-only and VM-first so packaging/lifecycle defects are found before CUDA LUN exposure.

## 11. Documents to update

- `docs/specs/no-milestone/windows-autonomous-broker-service/SPEC.md` and later `IMPL.md`.
- `docs/reliability/GAP-REGISTER.md`.
- `docs/reliability/DEGRADATION-MATRIX.md`.
- Windows installation, recovery, upgrade, rollback, and daily-use runbooks.
- `README.md` support/status table and limitations.
- Release manifest/signing documentation, without claiming production signing completion.
- `MEMORY.md` with local append-only session evidence.

## 12. Out of scope

- Microsoft production/attestation signing, EV certificate procurement, Secure Boot trust, and anti-cheat compatibility.
- Enabling or managing a RamShared pagefile for daily use.
- Changes to the StorPort miniport ABI, queue layout, or CUDA storage backend.
- ublk, NBD data transport, custom kernels, remote tenants, or any Windows daily-profile TCP broker exposure.
- Automatic fallback to RAM, files, or a lab backend.
- Unsupervised pressure, crash, GPU reset, or pagefile-hot removal on the daily host.
- GUI/tray applications, fleet management, remote telemetry, auto-update, or multi-host orchestration.
- Broader Windows/GPU/driver compatibility beyond the existing allow-listed matrix.

## 13. Acceptance criteria

1. One supported package installs exact broker, service, driver, configuration, manifest, ACL, and SCM definitions; BINARY_MATCH is green for every shipped artifact.
2. No product path references or starts the PowerShell lab broker.
3. A cold boot with no interactive login can reach `BrokerReady`; starting the consumer then reaches Online with the expected serial, size, CUDA allocation, and lease.
4. Broker-unavailable or protocol-mismatch startup refuses within 30 seconds and creates no lease, CUDA allocation, queue, or LUN.
5. Three consecutive cold-boot lifecycles each pass Online CUDA I/O, three deterministic SHA-256 rounds, graceful stop without force-kill, lease release, CUDA restoration, no new dump, and zero LUN/volume/Win32/PnP residue.
6. VM broker-loss and consumer-loss drills produce no BugCheck, corruption, duplicate owner, ghost device, unbounded restart, or hidden fallback, and end in the specified observable state.
7. Clean product stop completes within 45 seconds; unsafe pagefile/ownership state refuses before destructive teardown or broker stop.
8. Same-version repair, one-version upgrade, manufactured upgrade failure with rollback, and uninstall leave no mixed manifest or orphan service.
9. Status and evidence distinguish process-running from protocol-ready and Online from Degraded/FailedSafe.
10. The gap register and README continue to label normal Secure Boot Windows support as blocked until the independent signing gate passes.

## 14. Validation plan

### Unit and static

- Named tests for manifest validation, absolute-path/ACL policy, start-policy parsing, state transitions, retry budget, readiness timeout, protocol/version/frame bounds, idempotent commands, and rollback selection.
- Static installer tests proving exact SCM arguments, dependency, service SIDs, IPC ACL, failure actions, no lab-script reference, and no daily-profile TCP listener.
- Existing broker codec/model and `ramshared-winsvc` tests remain green with the repository's required coverage gate for touched Rust slices.

### Windows VM live path

Use disposable `win11-drill` and capture `before → action → after` evidence for:

1. fresh install and demand-start;
2. cold boot without login;
3. normal Online I/O and clean stop;
4. broker absent, slow readiness, protocol mismatch, malformed frame, unauthorized peer, and IPC endpoint collision;
5. broker exit before provisioning and after Online;
6. consumer exit and simultaneous stop requests;
7. same-version repair, upgrade, manufactured failed upgrade/rollback, and uninstall;
8. exact terminal cleanup and dump checks.

The destructive failure drills remain VM-only. Every run records package and guest binary hashes.

### Physical Test Mode live path

After every VM gate is green, use the existing supervised Windows watchdog harness on the allow-listed physical host. Run three consecutive cold boots with the same release package and one consolidated evidence set containing:

- SCM configuration and start order;
- broker readiness and protocol identity;
- package ↔ host `BINARY_MATCH`;
- Online identity, CUDA allocation, and lease;
- three SHA-256 I/O rounds per boot;
- graceful consumer-first stop, lease release, CUDA restoration;
- no force-kill, no new dump, and zero LUN/volume/Win32/PnP residue;
- terminal VM/WSL/GPU state required by the harness.

### Environment-bound gaps

- Production-signed Secure Boot operation remains `PARTIAL`/blocked until the independent signing process and clean normal-Windows installation proof exist.
- Automatic-start promotion remains `PARTIAL` until the three cold-boot physical lifecycles pass.
- Any acceptance item that cannot run in the available VM or physical environment remains explicitly `PARTIAL`; documentation or mocks cannot close it.
