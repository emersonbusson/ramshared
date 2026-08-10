# SPEC — Autonomous Windows broker service packaging and supervision

> Revised in place after the 2026-07-24 SSDV3 Step 2.5 initial `no-go`.
> Corrections close cancellable named-pipe I/O, sender-bound lease release,
> exact SCM failure-action semantics, effective-config integrity, and stale
> status/evidence ambiguity. The re-audit verdict is recorded in
> [`AUDIT-2.5.md`](AUDIT-2.5.md).

## Closed scope

### In now

- Add one native Windows broker executable and SCM service, `RamSharedBroker`, built from this workspace.
- Replace the installed Windows daily profile's TCP broker client with a Windows named-pipe client at the fixed endpoint `\\.\pipe\RamSharedBroker.v1`.
- Reuse protocol-v1 message semantics and the bounded JSONL codec from
  `crates/ramshared-broker/src/protocol.rs`; `Register → Registered` is the readiness/version handshake and must not allocate a lease.
- Extract one transport-independent logical lease state machine into
  `crates/ramshared-broker` and use it from both the Linux broker core and the Windows broker.
- Register stable SCM service SIDs, a consumer dependency on `RamSharedBroker`, bounded service failure actions, and demand-start by default.
- Add a manifest-owned, transactional installer/control path for install, repair, status, start, stop, upgrade/rollback, and uninstall.
- Preserve the existing `ramshared-winsvc` CUDA/StorPort ownership and teardown order.
- Prove the lifecycle VM-first, then with three consecutive physical Test Mode cold boots through the approved watchdog harness.

### Out now

- Driver/IOCTL/queue changes, pagefile enablement, production signing, Secure Boot and anti-cheat claims.
- A TCP listener in the installed Windows daily profile.
- Remote tenants, NBD/ublk, WSL-dependent startup, automatic update, UI, fleet telemetry, or fallback storage.
- Holder force-revoke: protocol v1 remains holder-cooperative and disconnect remains an ambiguous release signal.
- Automatic-start promotion before the physical cold-boot gate.

### Assumed-ready dependencies

- `crates/ramshared-winsvc/src/product_online.rs::run_product_online` owns the proven
  lease → CUDA → CREATE → REGISTER → Online and consumer-first teardown path.
- `crates/ramshared-winsvc/src/broker_tenant.rs::BrokerTenant` enforces exact grant bytes and release-at-most-once.
- `crates/ramshared-broker/src/protocol.rs::{Msg, read_msg, write_msg, PROTO_VERSION, MAX_LINE_BYTES}` is the canonical message/codec boundary.
- `crates/ramshared-wsl2d/src/broker_srv.rs::BrokerCore` is the current authoritative broker session integration.
- `crates/ramshared-winsvc/src/windows_host.rs::WindowsHostState` provides owned config reads, SHA-256, elevation, host observations, and Event Log precedent.
- The current physical Test Mode storage evidence remains valid only as a storage-path prerequisite; it does not close this lifecycle slice.

## Traceability

| PRD | SPEC |
| --- | --- |
| RF-1 | ITEM-3, ITEM-6, ITEM-7 — DT-2, DT-8 |
| RF-2 | ITEM-4, ITEM-6 — DT-1, DT-6, DT-9 |
| RF-3 | ITEM-3, ITEM-4, ITEM-5 — DT-3, DT-4, DT-5 |
| RF-4 | ITEM-5, ITEM-6, ITEM-8 — DT-7, DT-10 |
| RF-5 | ITEM-2, ITEM-4, ITEM-5, ITEM-8 — DT-4, DT-7, DT-11, DT-16 |
| RF-6 | ITEM-4, ITEM-5, ITEM-6 — DT-10, DT-12 |
| RF-7 | ITEM-6, ITEM-8 — DT-8, DT-9, DT-13 |
| RF-8 | ITEM-6, ITEM-8 — DT-6, DT-14 |
| NFR-1 | ITEM-5, ITEM-8, ITEM-9 — DT-7, DT-11 |
| NFR-2 | ITEM-2, ITEM-3, ITEM-4, ITEM-6 — DT-1, DT-3, DT-5, DT-9, DT-16 |
| NFR-3 | ITEM-2, ITEM-4, ITEM-5, ITEM-6, ITEM-8 — DT-4, DT-7, DT-11, DT-13 |
| NFR-4 | ITEM-4, ITEM-5, ITEM-8 — DT-10, DT-12 |
| NFR-5 | ITEM-2, ITEM-3, ITEM-6 — DT-2, DT-5, DT-8 |
| NFR-6 | ITEM-5, ITEM-8 — DT-4, DT-10 |

## Technical decisions

| # | Decision | Why |
| --- | --- | --- |
| DT-1 | Create a second own-process SCM service named `RamSharedBroker`, display name `RamShared Local Broker Service`. Its account is the virtual service account `NT SERVICE\RamSharedBroker`; `RamSharedWinSvc` remains LocalSystem because it owns the driver/CUDA path. Set both services to unrestricted service-SID mode. `RamSharedWinSvc` declares `RamSharedBroker` as an SCM dependency. | Separates broker supervision from storage ownership, gives both processes stable OS identities, and avoids granting the broker LocalSystem without evidence that it needs it. |
| DT-2 | Add workspace crate/binary `crates/ramshared-winbroker` rather than compiling the Linux `ramshared-wsl2d` daemon for Windows or embedding an unsupervised child in `ramshared-winsvc`. The new crate depends on `ramshared-broker`, `serde`, `serde_json`, `toml` for the required immutable `broker.toml`, `windows-service`, and the minimum `windows-sys` features only. It has no CUDA, StorPort, NBD, ublk, WSL, or GPU dependency. | The Windows service is a control-plane process. Reusing the Linux daemon would import false platform lifecycle; embedding it would defeat SCM supervision. The explicit TOML dependency keeps the DT-15 configuration format parseable without an undocumented codec. |
| DT-3 | The installed daily transport is a byte-stream Windows named pipe with the exact canonical name `\\.\pipe\RamSharedBroker.v1`, maximum four instances, 64 KiB input/output buffers, and protocol line cap `MAX_LINE_BYTES`. Every server/client handle uses `FILE_FLAG_OVERLAPPED`; accept, read, and write have owned `OVERLAPPED`+event state, a 10 s per-frame deadline, and service-stop cancellation through `CancelIoEx` followed by `GetOverlappedResult`. `ERROR_OPERATION_ABORTED` during stop is normal shutdown. No worker is joined until its outstanding operation has completed/cancelled; failure to quiesce within 10 s leaves SCM `StopPending`/failure and must not close a live handle underneath I/O. The pipe name is not configurable in the daily profile. TCP remains only in existing lab/cross-platform code and is absent from the `ramshared-winbroker` dependency graph. | Removes local port squatting/firewall surface without introducing an uninterruptible `ConnectNamedPipe`/`ReadFile` hang that would violate SCM stop bounds or close handles during in-flight I/O. |
| DT-4 | Readiness is the first authenticated session's existing `Msg::Register { proto: PROTO_VERSION, tenant, transport: WinDrive }` followed by `Msg::Registered`. The same pipe instance then remains the authoritative lease session. No new `Ready`, `Ping`, or compatibility message is added. Client connection uses a 30 s overall deadline with 250 ms reconnect intervals only for `ERROR_FILE_NOT_FOUND` and `ERROR_PIPE_BUSY`; all other errors fail immediately. | Protocol v1 already proves codec/version/core readiness and tenant admission without allocating a lease. Reusing the session avoids a readiness/session TOCTOU. |
| DT-5 | The product-pipe security descriptor is built from the runtime-resolved SID of `NT SERVICE\RamSharedWinSvc`. Because that service is LocalSystem with an unrestricted service SID, the DACL grants SYSTEM data access for the normal-token pass and the resolved service SID data access for the restricted-token pass; the broker SID retains control and Builtin Administrators receive no product-protocol ACE. At server startup, resolve the expected service SID with two-call `LookupAccountNameW`, require `SidTypeWellKnownGroup`, copy the SID into owned memory, and fail on ambiguity/absence. After `ConnectNamedPipe`, perform one bounded overlapped read into the owned 64 KiB-capped frame buffer (Windows returns `ERROR_CANNOT_IMPERSONATE` before a pipe read), then call `ImpersonateNamedPipeClient`, open the thread token, require the exact service SID as an enabled non-deny-only group, close the token, and execute `RevertToSelf` through an RAII guard before parsing those bytes. Thus another SYSTEM process can satisfy the DACL's normal pass but is still refused before parsing, session ID, or broker mutation. The status pipe independently authenticates Administrators or the consumer service SID with the same bounded read-then-impersonate pattern. | Windows restricted service tokens must pass the access check against both normal and restricted SID sets, and named-pipe impersonation requires a preceding server read. Explicit token verification remains the authoritative peer boundary and prevents a SYSTEM peer, deny-only SID, or inherited-token assumption from becoming protocol authentication. |
| DT-6 | Installation default is demand-start for both services. `RamSharedWinSvc` depends on `RamSharedBroker`, so starting the consumer starts the broker. The only supported opt-in setting is `start_policy = "automatic"` in the signed/hashed product manifest; promotion would use delayed-auto for the consumer and automatic for the broker. ITEM-9 passed, but the Step 3 promotion decision is to retain demand-start: the physical proof exercised explicit starts and the package remains test-signed. Installation, repair, or policy change never calls start. | Preserves explicit disk creation and avoids silently broadening a supervised Test Mode acceptance into an automatic-start or public-release claim. |
| DT-7 | Broker loss **before** lease/CUDA/disk exposure returns broker error code 3 and unwinds to `Stopped`. Broker loss **after** `Online` sets `RuntimePhase::FailedSafe`, logs `broker_lost_online`, keeps the CUDA/queue/LUN I/O pump alive, and requests the existing safe consumer teardown. It never force-destroys, re-registers, reconnects, reacquires, or treats disconnect as proof of lease release. Pagefile/identity/lock refusal leaves the consumer running in `FailedSafe` for operator recovery; SCM reports a service-specific failure only after the safe runtime returns. | The miniport depends on the process I/O pump, not on the broker. Killing/restarting the consumer to recover a logical lease would turn a control-plane loss into storage loss. |
| DT-8 | Introduce `ProductManifestV1`, capped at 64 KiB and parsed with `deny_unknown_fields`, containing schema, semantic package version, commit, architecture, start policy, fixed service identities, and `{relative_path, sha256}` entries for broker EXE/config, winsvc EXE/config, and driver INF/CAT/SYS. Paths must be normalized relative children with no prefix/root/`..`; hashes are uppercase 64-hex SHA-256. Both effective configs are immutable version artifacts, not freely mutable ProgramData copies. The manifest is immutable inside each version directory. | One manifest is the atomic compatibility/BINARY_MATCH unit and prevents mixed broker/service/driver/config versions, manual configuration drift, or manifest path escape. |
| DT-9 | Install under `C:\Program Files\RamShared\versions\<version>-<commit12>\`; only evidence, rollback journal, and the active pointer live under `C:\ProgramData\RamShared\`. Stage to a sibling `.staging-<run_id>` directory, single-open hash every owned file, apply the final protected ACL, reopen/hash after ACL application, rename the complete directory to its final immutable name, then update SCM image paths/config arguments. Replace `active-manifest.json` through same-volume `ReplaceFileW` with write-through and retain `active-manifest.bak`. On missing/corrupt pointer, no service is started automatically: recovery must match both stopped SCM image paths to exactly one complete manifest before restoring the pointer. The installer mutex is `Global\RamSharedProductInstall.v1`, created with an Administrators+SYSTEM DACL rather than the caller's default DACL. | Directory rename defines a local staging frontier; post-ACL revalidation closes staging substitution, and the backup/recovery rule avoids treating a temp rename as a crash-proof transaction. Immutable effective configs make the active manifest the complete executable configuration unit. |
| DT-10 | Add a machine-readable `status --json` command to `ramshared-winsvc`. It performs read-only SCM queries, reads the active manifest, re-hashes installed artifacts, performs current host/LUN residue observations, and queries broker status only through a separately authenticated, side-effect-free broker status pipe `\\.\pipe\RamSharedBrokerStatus.v1`. This pipe uses a local 4 KiB-capped `BrokerStatusRequestV1`/`BrokerStatusV1` codec, not `protocol::Msg`; it admits Builtin Administrators and `RamSharedWinSvc` and has no mutation command. JSONL rows are labelled `last_evidence` with timestamp/run ID and are never converted into current health. Human `status` renders the same owned snapshot. Any failed live source yields `Unknown`/non-zero, never a healthy inference from stale evidence. | `Msg::StatusReply` describes tenants/slices and has no broker-process identity. A separate ACL and local status schema avoids changing protocol v1 or turning administrator diagnostics into a lease client; explicit freshness prevents stale JSONL from masquerading as current health. |
| DT-11 | Configure SCM automatic restart only for abnormal termination of `RamSharedBroker`, with four explicit actions: restart after 5 s, restart after 15 s, restart after 40 s, then `SC_ACTION_NONE`; failure-count reset is 60 s. Keep `SERVICE_FAILURE_ACTIONS_FLAG.fFailureActionsOnNonCrashFailures = FALSE`. Deterministic config/ACL/protocol/startup errors report a service-specific error and exit normally, so SCM does not apply recovery actions. Transient pipe-not-ready handling remains the bounded client retry in DT-4, not an exit-code-sensitive SCM policy. `RamSharedWinSvc` has no SCM automatic restart. After Online, pipe EOF triggers DT-7 and no automatic consumer reconnect/replay occurs. The broker persists no lease across process restart and reports a new `broker_instance_id` only through evidence/status, not protocol v1. | SCM recovery does not select actions by service exit code. Crash-only recovery plus an explicit fourth `NONE` matches Windows semantics, bounds actual crashes, and keeps deterministic failures out of a misleading restart loop. |
| DT-12 | Lifecycle evidence is append-only JSONL under `C:\ProgramData\RamShared\evidence\service-lifecycle.jsonl`, one record capped at 16 KiB. Both services also emit concise Windows Event Log transitions. `broker_instance_id` is a per-process random 128-bit hex value from `BCryptGenRandom`; no address, token, payload, or secret is logged. | Correlates both SCM processes and makes process-running versus protocol-ready observable without unsafe data. |
| DT-13 | Upgrade/rollback is stop-first and never in-place. The controller safely stops `RamSharedWinSvc`, requires zero lease/LUN/queue residue, then stops `RamSharedBroker`; only then may it switch SCM image paths and the active manifest. If validation/registration fails before either new service starts, restore old SCM definitions and active pointer. Once a new consumer reaches Online, rollback is forward-only through its normal safe stop; no file-level reversal while Online is allowed. Uninstall follows the same stop gate and deletes only files named by retained manifests. | Persistent SCM and storage ownership cannot participate in one filesystem transaction. Explicitly splitting pre-start rollback from post-Online forward-only recovery avoids mixed state. |
| DT-14 | The physical cold-boot gate uses one immutable manifest for all three boots. Each boot starts from services stopped or the selected explicit auto policy, runs three SHA rounds, performs supported stop, and reboots only after zero-residue proof. Automatic-start remains documentation-disabled until all three runs and final preflight pass. | Prevents changing binaries/config between samples and prevents a previous cleanup from masking a boot-lifecycle defect. |
| DT-15 | Add protected immutable `broker.toml` with `[local_broker] schema = 1`, `capacity_bytes`, `allowed_tenant`, and absolute `evidence_path`. The installer single-open parses both version-owned broker and winsvc configs and requires `allowed_tenant == win_drive.tenant`, `capacity_bytes == win_drive.size_bytes`, and `capacity_bytes` to be non-zero and a multiple of `win_drive.block_size`. The Windows session core grants only when `LeaseRequest.bytes == capacity_bytes`; smaller/larger requests are denied. Each service independently re-hashes its own config against the active manifest before creating IPC/CUDA/storage effects. | A standalone control-plane service has no Linux slice pool from which to infer capacity. Exact request/cross-config equality and runtime hash checks make manual or torn configuration divergence fail closed without adding fields to protocol v1. |
| DT-16 | `LeaseRelease` authorization is sender-bound. `BrokerCore::on_msg` passes the session ID into `on_lease_release`; both broker shells resolve the authenticated registered holder and call `LeaseBook::release(holder, lease)`. An unregistered session or wrong holder receives `Msg::Error`, is closed, and does not mutate the lease. Unknown/duplicate release from the correct holder is logged as an idempotent no-op. Disconnect calls `LeaseBook::disconnect(holder)` exactly once. | The current Linux handler accepts only a lease ID, so a different registered tenant that learns/guesses the ID could release another holder's lease. The shared extraction must fix, not preserve, that authorization defect. |
| DT-17 | The physical-host drill takes its target volume letter from the immutable packaged `winsvc_config`; before product install it proves that the letter is absent from both the host drive map and pagefile set. A collision is a pre-exposure refusal: the harness never removes, remaps, formats, or reletters the existing volume. VM and physical evidence may use different manifests when their safe free letters differ, but all boots in one physical campaign use one manifest hash. | Drive letters are host-global operator state. The lab convention `R:` can name a real data volume on the official host, so a hard-coded drill letter is unsafe and invalidates identity evidence. |
| DT-18 | Every physical integrity round hashes the intended in-memory payload before the write and compares it with the exact read-back bytes. Before any partition mutation, the harness binds the immutable manifest and effective `winsvc_config` size/sector/letter to the current winsvc PID/run `Online` serial, then requires exactly one `RAMSHARE VRAMDISK` with that serial and size, Virtual bus, matching sectors, `PartitionStyle=RAW`, zero partitions, and non-boot/non-system flags. A non-RAW or contradictory observation is a refusal, never a reusable prior volume. | Hashing the file after the write lets consistent corruption pass. Friendly-name plus a hard-coded size can select a foreign or stale device, while the product serial is deliberately generated per Online run and therefore must come from current-run evidence rather than be invented from the package. |
| DT-19 | Physical preflight refuses the packaged volume letter when it appears in either active `Win32_PageFileUsage` or configured `PagingFiles`; query or parse failure is also a refusal. A stop command exception is RED even if SCM later reports `Stopped`; the harness never records a supported-stop PASS after a failed operator surface. | A configured next-boot pagefile remains host-persistent state even when it is not active yet. SCM terminal state alone does not prove that the supported stop command returned cleanly. |
| DT-20 | Every potentially blocking physical child (CIM/storage discovery, format, and controller invocation) uses redirected asynchronous output draining, a numeric deadline, and verified `taskkill /T /F` process-tree termination on timeout. The parent never calls unbounded `Stop-Job`/`Remove-Job` on a worker stuck in device I/O. Timeout or incomplete tree termination is RED and retains bounded evidence. | A watchdog can stop services but cannot release a controller blocked in a synchronous cmdlet or a PowerShell job waiting in device I/O. Draining redirected pipes only after waiting can itself deadlock on a full pipe. |
| DT-21 | Each physical reboot requires a fresh explicit operator invocation with `-ApprovePhysicalHost -ApproveReboot`. That invocation creates one cryptographically random, single-use resume token valid for at most 30 minutes, persists only its SHA-256 and expiry in campaign state, and schedules a one-shot startup command containing the token but never `-ApprovePhysicalHost`. Startup atomically consumes the exact unexpired token before mutation and unregisters the task. After one boot drill it stops in `awaiting_approval`; it never schedules or performs the next reboot itself. Watchdog shutdown requires the separate `-AllowWatchdogShutdown` approval for that exact boot, and its delayed worker is bound to a unique marker nonce so an older worker cannot act on a later boot's marker. Every boot-action catch/finally path disarms the marker and unregisters the task; no failure leaves a delayed shutdown or replayable approval behind. | A persisted approval switch is not fresh authorization. Automatic chained reboots conflict with the operator requirement to approve each reboot and make a failed scheduled task replayable on later boots. A distinct short-lived one-use token preserves no-login cold-boot execution without granting the startup task general physical-host authority or surviving a cancelled reboot indefinitely. Generation-binding also prevents an earlier sleeping watchdog from mistaking a newly armed campaign for its own failed boot. |
| DT-22 | The VM autonomous lifecycle treats runtime evidence and Disk Event ID 153 as safety sources, never as best-effort diagnostics. Before disk discovery it requires exactly one schema-1 `Online` row from the current `RamSharedWinSvc` PID: the row PID, `run_id`, evidence filename, timestamp, vendor/product, 16-hex `lun_serial`, and configured size must agree, and any evidence directory/list/read/JSON failure is RED. It binds that serial to one exact `RAMSHARE VRAMDISK` before format or I/O. The Event Log query filters the `disk` provider over the current I/O interval and uses terminating errors; only `NoMatchingEventsFound` is a legitimate zero-event observation, while every provider/query failure is RED rather than an empty retry count. Format-timeout recovery likewise uses terminating volume observation, so an unreadable or ambiguous volume state refuses `Clear-Disk`. | A missing evidence directory, malformed row, unavailable Event provider, or failed volume query previously collapsed into an empty collection. That can turn unknown ownership, unobserved retry errors, or a published volume into a false PASS. |
| DT-23 | The VM autonomous lifecycle accepts an explicit immutable `HostBinDir` and forwards it unchanged to `Run-GuestProductPackage.ps1`. The default remains the documented host staging directory, but qualification commands name the freshly hashed candidate directory; the harness never requires overwriting a physical-host service image merely to test a guest VM. | Reusing `C:\ramshared\bin` couples guest qualification to the physical service staging area and can make a VM proof silently consume stale binaries. Explicit staging preserves package identity and keeps the stopped physical installation untouched. |
| DT-24 | Every host-side PowerShell Direct connect, remote invocation, and `Copy-Item -ToSession`/`-FromSession` in the VM product-package and autonomous-lifecycle harnesses runs in one disposable child `powershell.exe`. The parent starts concurrent stdout/stderr drains before waiting, applies a numeric outer deadline that exceeds the 180-second bounded readiness retry window, and on timeout targets only that exact child PID with `taskkill /T /F`; failure to observe child-tree exit is RED. Each worker opens one session, performs one operation, and removes that session in `finally`. Nonzero child exit, timeout, malformed/missing typed result, or cleanup failure is a fail-closed harness error. The password is passed only through the worker environment, never through its command line, payload file, or diagnostics. This deadline discipline does not authorize a physical-host restart; VM cold-boot behavior remains an explicitly invoked VM-only E2E action. | A PS Direct call can block before an in-process timeout or shared session cleanup owns it. Redirected pipes can also deadlock a parent that waits before draining. A killed child must not leave a reusable host session or a false successful package/lifecycle result. |
| DT-25 | The VM cold-boot action is a deferred guest-only shutdown, never `shutdown.exe /s /t 0` inside the typed PowerShell Direct worker. The lifecycle harness validates a 15–30 second requested delay, asks the guest to schedule that delay, requires a typed receipt with Boolean `shutdown_scheduled = true`, Int32 `delay_seconds` equal to the request, and a zero `shutdown.exe` exit code, and only then begins bounded `Off` polling. The delay gives the remote `Invoke-Command` return, result export, and worker `Remove-PSSession` cleanup a completion window; failure to schedule or validate the receipt is RED. This is not a physical-host reboot authorization. | Immediate guest shutdown can tear down PowerShell Direct before its worker exports the required typed result, falsely reporting a failed lifecycle even when the guest correctly accepted the shutdown request. |
| DT-26 | The manufactured PowerShell Direct deadline seam resolves the exact executable of its current PowerShell host, requires that path to be an existing file, and passes it explicitly to the synthetic grandchild. It never assumes `$PSHOME\powershell.exe`: Windows PowerShell 5.1 normally hosts `powershell.exe`, while PowerShell 7 normally hosts `pwsh.exe`. The chosen path is used only for the static timeout/process-tree fixture and does not change the product worker interpreter or authorize a guest/host action. | A source-only gate that works only under Windows PowerShell 5.1 can fail on the GitHub Windows runner after every product test has passed. Binding the already-running interpreter makes the manufactured process-tree proof portable without weakening identity or timeout checks. |

## Atomicity and rollback

### Atomicity frontier

- **Userspace/daemon:** a broker session mutates logical lease state only after peer authentication and a valid `Register`. One `LeaseRequest` either yields one exact `LeaseGranted` or no grant. `LeaseRelease` remains at-most-once from the consumer.
- **Filesystem/package:** the immutable version directory becomes eligible only after complete hash validation and directory rename. `active-manifest.json` selects exactly one complete version.
- **SCM:** the two service definitions are not transactionally updated by Windows. The controller records old definitions, changes the broker first and consumer second while both are stopped, re-queries exact values, then advances the active manifest pointer.
- **Windows driver:** unchanged. Existing CREATE/REGISTER/UNREGISTER/DESTROY atomicity and ambiguous-state rules remain authoritative.
- **Host/persistent:** lease, CUDA allocation, queue, LUN, volume, pagefile observations, service definitions, and active manifest cross separate frontiers. A reported clean stop requires every applicable observation to agree.

### Rollback

- **Userspace/daemon:** before Online, close the pipe, undo only confirmed effects in reverse order, and return the stable error. After Online, DT-7/DT-13 are forward-only through safe stop.
- **Windows driver:** no package rollback or reinstall while a RamShared queue/LUN/pagefile state is active or ambiguous. Driver package switching remains outside this slice unless zero residue is independently proven.
- **Host/persistent:** before starting the new consumer, restore the captured old SCM definitions and old active-manifest pointer. Retain both immutable version directories until validation finishes.
- **Forward-only:** once the new consumer reaches Online, any rollback must first complete the current version's safe storage teardown. Pagefile refusal, lock refusal, broker-instance mismatch, or ambiguous crash forbids automatic continuation.

## Kahneman map (critical only)

| ITEM / stage | # | Question | Min evidence | Abort |
| --- | --- | --- | --- | --- |
| ITEM-2 lease extraction | #13/#17 — refusal and 2× = 1× | Does the legitimate holder release once while a second registered tenant is refused without mutation, and does duplicate release/disconnect preserve one transition? | `cargo test -p ramshared-broker wrong_holder_cannot_release && cargo test -p ramshared-wsl2d foreign_tenant_cannot_release_lease && cargo test -p ramshared-broker lease_book_release_twice_is_one_transition` | Foreign release, duplicate grant/release, or unrelated Linux broker behavior change |
| ITEM-3 named-pipe boundary | #13 — meaningful refusal | Does the real service SID succeed while SYSTEM/admin/unrelated service identities are refused before mutation? | `scripts/windows/Run-GuestBrokerService.ps1 -Case PeerMatrix` | One unauthorized admission or legitimate-client lockout |
| ITEM-4 restart policy | #15 — retry only transient | Are only missing/busy pipe cases retried within 30 s, and is the fourth broker failure left stopped? | `scripts/windows/Run-GuestBrokerService.ps1 -Case RetryBudget` | Retry of access/protocol/config error or >3 SCM restarts/60 s |
| ITEM-5 Online broker loss | #16 — from exhaustion | When the broker disappears after Online, does the I/O pump remain alive through safe stop/refusal? | `scripts/windows/Run-GuestBrokerService.ps1 -Case BrokerLossOnline` | BugCheck, I/O corruption, force-kill, reconnect/replay, or destructive teardown on refusal |
| ITEM-6 package switch | #2 — counterfactual | If the new registration fails halfway, is the old complete version still selected and runnable? | `scripts/windows/Run-GuestProductPackage.ps1 -Case ManufacturedRollback` | Mixed manifest, orphan service, or in-place binary replacement |
| ITEM-8 lifecycle | #9 — number | Are readiness/stop bounds and three SHA rounds recorded with units and exact hashes? | `scripts/windows/Run-GuestAutonomousLifecycle.ps1` | Readiness >30 s, consumer stop >30 s, product stop >45 s, or SHA mismatch |
| ITEM-9 physical gate | #5 — worst case | Does the same immutable package survive three cold boots on the supported host without residue? | `scripts/windows/Run-HostAutonomousLifecycle.ps1 -ColdBoots 3` | New dump, force-kill, residue, BINARY_MATCH failure, or host terminal state not clean |

## Security checklist (pre-impl)

- [x] **Privilege:** `RamSharedBroker` uses `NT SERVICE\RamSharedBroker`; `RamSharedWinSvc` retains LocalSystem. Both receive stable service SIDs. Named-pipe DACL and explicit token inspection admit the consumer service SID only.
- [x] **User/host copy:** config, manifest, JSONL protocol line, status frame, and file paths are copied into owned buffers with 64 KiB/16 KiB caps before validation/use.
- [x] **Flags/IOCTL codes:** no new IOCTL. Manifest enums use `deny_unknown_fields`; status pipe rejects every message except `Status`; protocol pipe rejects broker-to-client or pre-Register messages through the existing core contract.
- [x] **Info-leak:** no kernel/user virtual address, handle value, access token, payload, or security descriptor is logged.
- [x] **IRQ/atomic or IRQL:** N/A — userspace. Existing driver IOCTL/IRQL behavior is unchanged.
- [x] **Lifetime:** every pipe handle, `OVERLAPPED` event, impersonation token, service handle, file handle, mutex, and event source has one RAII owner; `CancelIoEx` plus completion precedes handle close, and `RevertToSelf` is mandatory on every impersonation exit. Disconnect notification reaches the lease core once.
- [x] **Hot-unplug/device-gone:** broker/service exit maps to stable `broker_lost_*` state; existing CUDA/driver device-lost path remains authoritative.
- [x] **Host safety:** destructive failure cases run in `win11-drill`; physical runs use the approved Windows watchdog, bounded waits, and no pressure/pagefile-hot removal.
- [x] **Replayable ops:** install repair, status, start request, stop request, release, and uninstall refusal have named idempotency tests. CREATE/REGISTER and ambiguous release are explicitly not replayed.

## Files to CREATE / MODIFY / DELETE

### CREATE

**`crates/ramshared-broker/src/lease.rs`**

- Purpose: single logical lease state machine shared by Linux and Windows broker shells.
- RF / DT: RF-1, RF-5; DT-2, DT-7, DT-11.
- Types / fns:
  - `pub struct LeaseBook { capacity_bytes: u64, next_id: u32, pending: Option<PendingLease>, active: Option<LogicalLease> }`
  - `pub struct PendingLease { pub holder: TenantId, pub requested_bytes: u64 }`
  - `pub struct LogicalLease { pub id: u32, pub holder: TenantId, pub bytes: u64 }`
  - `pub enum LeaseDecision { Pending(PendingLease), Denied(LeaseDeny) }`
  - `pub enum LeaseDeny { ZeroBytes, OverCapacity, AlreadyHeld, WrongHolder, WrongLease }`
  - `pub fn begin_request(&mut self, holder: TenantId, bytes: u64) -> LeaseDecision`
  - `pub fn grant_pending(&mut self, granted_bytes: u64) -> Result<LogicalLease, LeaseDeny>`
  - `pub fn cancel_pending(&mut self, holder: TenantId) -> bool`
  - `pub fn release(&mut self, holder: TenantId, lease: u32) -> Result<bool, LeaseDeny>`
  - `pub fn disconnect(&mut self, holder: TenantId) -> LeaseDisconnect`
- Reference pattern: `crates/ramshared-broker/src/slices.rs` single-owner state machine.
- Required tests: `crates/ramshared-broker/src/lease.rs` :: `zero_and_over_capacity_are_denied`, `request_stays_pending_until_explicit_grant`, `grant_may_round_to_slice_capacity`, `second_holder_is_denied`, `wrong_holder_cannot_release`, `lease_book_release_twice_is_one_transition`, `disconnect_cancels_or_releases_only_holder`, `lease_id_wrap_is_refused`.
- Cover target: ≥80%.
- Kahneman: #13/#17.

**`crates/ramshared-winbroker/Cargo.toml`**

- Purpose: Windows-native local broker binary definition.
- RF / DT: RF-1; DT-1, DT-2.
- Types / fns: binary `ramshared-winbroker`.
- Reference pattern: Windows target dependencies in `crates/ramshared-winsvc/Cargo.toml`.
- Required tests: `cargo check -p ramshared-winbroker --target x86_64-pc-windows-msvc`.
- Cover target: N/A — manifest.

**`crates/ramshared-winbroker/src/lib.rs`**

- Purpose: expose pure config/state/session seams for Linux-host unit coverage.
- RF / DT: RF-1, RF-3, RF-5, RF-6; DT-2, DT-4, DT-11, DT-12.
- Types / fns:
  - `BrokerConfigV1 { capacity_bytes, allowed_tenant, evidence_path }`
  - `BrokerPhase::{Stopped, Starting, Ready, Stopping, Failed}`
  - `BrokerSessionCore`
  - `BrokerSessionCore::on_authenticated_msg(session_id, Msg) -> Vec<BrokerEffect>`; for the Windows logical-budget service, a valid request calls `begin_request` then `grant_pending(requested_bytes)` in the same single-owner event turn
  - `BrokerSessionCore::on_disconnect(session_id) -> Vec<BrokerEffect>`
  - `BrokerEffect::{Reply, Close, Audit, LeaseReleased}`
- Reference pattern: pure-core/effect split in `crates/ramshared-wsl2d/src/broker_srv.rs::{BrokerCore, CoreEvent, Outbound}`.
- Required tests: `crates/ramshared-winbroker/src/lib.rs` :: `register_is_readiness_without_lease`, `message_before_register_is_refused`, `tenant_mismatch_is_refused`, `one_live_session_only`, `exact_lease_grant_and_release`, `disconnect_releases_server_state_and_audits_ambiguous`, `status_has_instance_and_lease_state`.
- Cover target: ≥80%.
- Kahneman: #13/#17.

**`crates/ramshared-winbroker/src/pipe.rs`**

- Purpose: Windows named-pipe creation, peer authentication, bounded session I/O, and status pipe.
- RF / DT: RF-3, RF-5; DT-3, DT-5, DT-10.
- Types / fns:
  - `PipeServer::bind_product(expected_service_sid: &Sid) -> io::Result<Self>`
  - `PipeServer::bind_status() -> io::Result<Self>`
  - `PipeServer::accept_authenticated(&self, stop: &AtomicBool, deadline: Instant) -> Result<AuthenticatedPipe, PipeAuthError>`
  - `AuthenticatedPipe::{read_frame_deadline, write_frame_deadline, cancel_and_quiesce}`
  - `fn resolve_service_sid(account: &str) -> Result<OwnedSid, PipeAuthError>`
  - `fn verify_client_service_sid(pipe: HANDLE, expected: &OwnedSid) -> Result<(), PipeAuthError>`
- Reference pattern: RAII Windows handles in `crates/ramshared-winsvc/src/windows_driver.rs`.
- Required tests: `scripts/windows/Run-GuestBrokerService.ps1` :: `legitimate_service_sid_connects`, `administrator_protocol_connect_is_refused`, `unrelated_service_is_refused`, `deny_only_service_sid_is_refused`, `oversized_line_is_refused`, `partial_frame_times_out`, `stop_cancels_blocked_accept`, `stop_cancels_blocked_read`, `status_pipe_rejects_mutation`.
- Cover target: N/A — Windows API seam; pure descriptor/path validation helpers ≥80%.
- Kahneman: #13.

**`crates/ramshared-winbroker/src/service.rs`**

- Purpose: SCM entry, broker state loop, stop handling, bounded worker ownership, Event Log/JSONL.
- RF / DT: RF-1, RF-2, RF-5, RF-6; DT-1, DT-4, DT-11, DT-12.
- Types / fns:
  - `run_service(config: BrokerConfigV1) -> Result<(), BrokerServiceError>`
  - `run_console(config: BrokerConfigV1, stop: Arc<AtomicBool>)`
  - `service_main(Vec<OsString>)`
  - `emit_transition(BrokerPhase, StableBrokerError)`
- Reference pattern: `crates/ramshared-winsvc/src/main.rs::windows_svc::run_service`.
- Required tests: `scripts/windows/Run-GuestBrokerService.ps1` :: `scm_start_ready_stop`, `stop_with_no_session_is_idempotent`, `stop_cancels_blocked_pipe_io`, `fourth_failure_remains_stopped`, `deterministic_failure_does_not_restart`.
- Cover target: N/A — E2E-only SCM shell.
- Kahneman: #15/#17.

**`crates/ramshared-winbroker/src/main.rs`**

- Purpose: SCM dispatcher and bounded `console --config <absolute>` test entry.
- RF / DT: RF-1; DT-1, DT-2.
- Types / fns: `main`, `entry`, `parse_cli`.
- Reference pattern: `crates/ramshared-winsvc/src/main.rs`.
- Required tests: `crates/ramshared-winbroker/src/main.rs` :: `cli_rejects_relative_config`, `cli_has_no_tcp_listen_option`, `cli_has_no_install_mutation`.
- Cover target: ≥80% for parser; Windows dispatcher N/A — boilerplate.

**`crates/ramshared-winbroker/broker.example.toml`**

- Purpose: canonical protected Windows local-broker configuration template.
- RF / DT: RF-1, RF-3; DT-3, DT-15.
- Shape: `[local_broker] schema = 1`, `capacity_bytes = 536870912`,
  `allowed_tenant = "windrive-host"`, and
  `evidence_path = "C:\\ProgramData\\RamShared\\evidence"`.
- Required tests: `crates/ramshared-winbroker/src/lib.rs` ::
  `example_config_parses`, `capacity_must_be_nonzero_and_aligned`,
  `unknown_config_field_is_refused`.
- Cover target: N/A — example consumed by covered parser.

**`crates/ramshared-winsvc/src/ipc.rs`**

- Purpose: transport abstraction and Windows named-pipe client implementing `Read + BufRead + Write`.
- RF / DT: RF-3, RF-5; DT-3, DT-4, DT-7.
- Types / fns:
  - `pub trait BrokerStream: BufRead + Write {}`
  - `pub fn connect_product_pipe(deadline: Instant) -> Result<NamedPipeBrokerStream, BrokerConnectError>`
- Reference pattern: current `product_online.rs::BrokStream`; RAII handles in `windows_driver.rs`.
- Required tests: `crates/ramshared-winsvc/src/ipc.rs` ::
  `only_not_found_and_busy_retry`, `deadline_stops_retry`;
  `scripts/windows/Run-GuestBrokerService.ps1` ::
  `register_and_lease_over_named_pipe`.
- Cover target: ≥80% for retry/error mapping; N/A — Windows handle seam.
- Kahneman: #15.

**`crates/ramshared-winsvc/src/package.rs`**

- Purpose: owned manifest parsing/validation, staging plan, SCM change plan, rollback record, status snapshot.
- RF / DT: RF-1, RF-2, RF-6, RF-7, RF-8; DT-6, DT-8, DT-9, DT-10, DT-13.
- Types / fns:
  - `ProductManifestV1`, `ManifestArtifact`, `StartPolicy`
  - `InstallPlan`, `ScmDefinition`, `RollbackRecord`, `ProductStatus`
  - `parse_manifest(&[u8])`, `validate_artifact_path`, `verify_staged_files`,
    `validate_cross_config(broker: &BrokerConfigV1, winsvc: &WinDriveConfig)`
  - `plan_install(current, candidate)`, `plan_rollback(record)`, `plan_uninstall(status)`
- Reference pattern: boundary validation in `crates/ramshared-winsvc/src/config.rs`; effect planning in `src/runtime.rs`.
- Required tests: `crates/ramshared-winsvc/src/package.rs` :: `manifest_rejects_unknown_and_over_64k`, `artifact_path_cannot_escape`, `hash_must_be_sha256_hex`, `mixed_commit_is_refused`, `broker_capacity_must_equal_lun_size`, `broker_tenant_must_equal_winsvc_tenant`, `same_version_repair_is_idempotent`, `half_registered_candidate_rolls_back_old_definitions`, `uninstall_refuses_owned_storage`.
- Cover target: ≥80%.
- Kahneman: #2/#13/#17.

**`scripts/windows/Test-AutonomousBrokerStatic.ps1`**

- Purpose: repository/package static contract for service names, pipe-only daily profile, manifest, ACL plan, and no lab broker.
- RF / DT: RF-1, RF-2, RF-3; DT-1, DT-3, DT-5, DT-8.
- Required tests: `scripts/windows/Test-AutonomousBrokerStatic.ps1` ::
  `BROKER_BINARY_MATCH`, `SCM_DEPENDENCY_MATCH`, `SERVICE_SID_MATCH`,
  `DAILY_TCP_LISTENER_ABSENT`, `NO_LAB_BROKER_REFERENCE`.
- Cover target: N/A — harness.

**`scripts/windows/Run-GuestBrokerService.ps1`**

- Purpose: isolated SCM/IPC/security/failure matrix without StorPort exposure where possible.
- RF / DT: RF-2, RF-3, RF-5; DT-1, DT-3, DT-4, DT-5, DT-11.
- Required tests: `scripts/windows/Run-GuestBrokerService.ps1` ::
  `legitimate_service_sid_connects`,
  `administrator_protocol_connect_is_refused`,
  `unrelated_service_is_refused`, `deny_only_service_sid_is_refused`,
  `oversized_line_is_refused`, `partial_frame_times_out`,
  `stop_cancels_blocked_accept`, `stop_cancels_blocked_read`,
  `status_pipe_rejects_mutation`, `scm_start_ready_stop`,
  `fourth_failure_remains_stopped`, and
  `deterministic_failure_does_not_restart`.
- Cover target: N/A — Windows VM E2E.

**`crates/ramshared-winsvc/src/bin/ramshared-service-sid-probe.rs`**

- Purpose: VM-only SCM probe that runs under the real
  `RamSharedWinSvc` LocalSystem+service-SID token, performs
  Register→LeaseRequest→LeaseRelease over the product pipe, records a bounded
  result, and exits without CUDA, driver, LUN, volume, or pagefile effects.
- RF / DT: RF-2, RF-3; DT-3, DT-5.
- Packaging: test artifact only; it is not one of the seven product-manifest
  artifacts and is never installed by the product controller.
- Tests: `Run-GuestBrokerService.ps1 -Case PeerMatrix`.
- Cover target: N/A — Windows SCM/token E2E helper.

**`scripts/windows/Run-GuestProductPackage.ps1`**

- Purpose: install/repair/upgrade/manufactured rollback/uninstall transaction drills.
- RF / DT: RF-1, RF-2, RF-7, RF-8; DT-6, DT-8, DT-9, DT-13.
- Required tests: `scripts/windows/Run-GuestProductPackage.ps1` ::
  `FreshInstall`, `Repair`, `ManufacturedRollback`, `UninstallRefusal`,
  `CleanUninstall`.
- Cover target: N/A — Windows VM E2E.

**`scripts/windows/Run-GuestAutonomousLifecycle.ps1`**

- Purpose: one VM autonomous lifecycle campaign with Online I/O and exact cleanup evidence.
- RF / DT: RF-4, RF-5, RF-6; DT-4, DT-7, DT-10, DT-12, DT-14, DT-22, DT-23, DT-24, DT-25.
- Required tests: `scripts/windows/Run-GuestAutonomousLifecycle.ps1` ::
  `cold_boot_no_login`, `three_round_sha`, `consumer_first_stop`,
  `lease_release`, `zero_residue`,
  `current_online_evidence_failure_is_red`,
  `event153_query_failure_is_red`,
  `recovery_volume_query_failure_is_red`,
  `psdirect_outer_deadline_is_enforced`,
  `psdirect_redirected_streams_are_drained`,
  `psdirect_timeout_terminates_child_tree`,
  `psdirect_nonzero_child_is_red`,
  `psdirect_calls_are_session_finally_cleaned`,
  `deferred_guest_shutdown_preserves_psdirect_result`,
  `psdirect_runner_uses_current_host_executable`.
- Cover target: N/A — Windows VM E2E.

**`scripts/windows/Run-GuestProductPackage.ps1`**

- Purpose: prepare and exercise immutable VM product packages without an
  unbounded host-side PowerShell Direct operation.
- RF / DT: RF-2, RF-4, RF-5; DT-8, DT-9, DT-13, DT-24.
- Required tests: `FreshInstall`, `Repair`, `ManufacturedRollback`,
  `UninstallRefusal`, `CleanUninstall`,
  `guest_product_package_retries_psdirect_readiness`, and every DT-24 test
  named for `Run-GuestAutonomousLifecycle.ps1` above.
- Cover target: N/A — Windows VM E2E/static seam.

**`scripts/windows/Invoke-GuestPsDirectBounded.ps1`**

- Purpose: the shared PowerShell 5.1 outer-deadline, asynchronous-drain,
  typed-result, and session-cleanup boundary used by the two VM harnesses.
- Required tests: every DT-24/DT-25/DT-26 test named above.
- Cover target: N/A — manufactured PowerShell seam.

**`scripts/windows/Test-GuestPsDirectDeadlineStatic.ps1`**

- Purpose: parser/static checks plus a locally manufactured child process tree
  that exercises the production deadline, stream-drain, termination, and
  fail-closed result guards without opening a VM session.
- Required tests: `psdirect_outer_deadline_is_enforced`,
  `psdirect_redirected_streams_are_drained`,
  `psdirect_timeout_terminates_child_tree`, `psdirect_nonzero_child_is_red`,
  `psdirect_calls_are_session_finally_cleaned`,
  `deferred_guest_shutdown_preserves_psdirect_result`,
  `psdirect_runner_uses_current_host_executable`.
- Cover target: N/A — manufactured PowerShell seam.

**`scripts/windows/Run-HostAutonomousLifecycle.ps1`**

- Purpose: supervised physical Test Mode three-cold-boot orchestrator using the existing Windows watchdog discipline.
- RF / DT: all acceptance; DT-14.
- Required tests: `scripts/windows/Run-HostAutonomousLifecycle.ps1` ::
  `three_cold_boots_same_manifest`, `final_preflight_clean`,
  `resume_marker_is_monotonic`, `cleanup_artifacts_complete`,
  `intended_payload_corruption_is_red`,
  `exact_online_identity_required_before_format`,
  `non_raw_lun_refuses_before_mutation`,
  `active_pagefile_refuses_before_install`,
  `configured_pagefile_refuses_before_install`,
  `pagefile_query_failure_refuses_before_install`,
  `stop_request_error_is_red`, `bounded_child_terminates_process_tree`,
  `resume_task_has_one_time_token_without_approval_switch`,
  `stale_or_replayed_resume_token_is_refused`,
  `watchdog_shutdown_requires_separate_approval`,
  `failure_cleanup_disarms_watchdog_and_task`.
- Cover target: N/A — physical E2E.
- Kahneman: #5/#9.

**`scripts/windows/Test-HostAutonomousLifecycleStatic.ps1`**

- Purpose: PowerShell 5.1 manufactured tests for the physical harness's pure
  identity, pagefile, integrity, stop, authorization, cleanup, and bounded-child
  contracts. It may create and terminate only its own temporary PowerShell
  process tree; it never invokes the harness's physical execution path.
- Required tests: every manufactured test named for
  `Run-HostAutonomousLifecycle.ps1` above except the four live three-boot
  evidence names.
- Cover target: N/A — manufactured PowerShell seam.
- Kahneman: #13/#15/#16/#17.

### MODIFY

**`.cargo/config.toml`**

- What: for `x86_64-pc-windows-msvc`, enable `target-feature=+crt-static` so
  the exact seven-artifact product manifest has no undeclared VC Runtime DLL
  dependency.
- RF / DT: RF-1, RF-7; DT-2, DT-8.
- Tests: native Windows release build plus `dumpbin /dependents`/equivalent
  proving `VCRUNTIME140.dll` is absent.
- Cover: N/A — build manifest.

**`Cargo.toml`**

- What: add `crates/ramshared-winbroker` to workspace members.
- RF / DT: RF-1; DT-2.
- Before → after: no Windows broker binary → one independently buildable binary.
- Tests: `cargo metadata --no-deps`; Windows target check.
- Cover: N/A — manifest.

**`crates/ramshared-broker/src/lib.rs`**

- What: export `pub mod lease`.
- RF / DT: RF-1, RF-5; DT-2.
- Symbol: module surface only.
- Tests: `cargo test -p ramshared-broker`.
- Cover: N/A — structural module surface; the exact package suite is bound by the
  `rust-slice-structural-contract-v1` declaration below.

**`crates/ramshared-wsl2d/src/broker_srv.rs`**

- What: replace locally owned pending/active lease identity and holder validation with
  `ramshared_broker::lease::LeaseBook`, while retaining slice draining/reservation and all existing Linux session/telemetry effects. Change `arbiter::Action::GrantLease` to carry holder/slices only; `BrokerCore` calls `LeaseBook::grant_pending(granted_bytes)` to allocate the canonical lease ID after slices become `Leased`.
- RF / DT: RF-1, RF-5; DT-2, DT-7.
- Symbols: `BrokerCore::{pending_lease, lease, on_lease_request, on_lease_release, on_disconnect, on_tick}` and `ramshared_broker::arbiter::{Action::GrantLease, Arbiter::tick}`.
- Before → after: lease identity/holder/capacity rules embedded in Linux core and release keyed only by lease ID → shared sender-bound `LeaseBook`; Linux slice actions remain in `BrokerCore`.
- Callers/docs: existing `spawn_broker` and memory-broker SPEC remain behaviorally unchanged.
- Tests: retain `register_assigns_stable_id_and_acks_psi`, `duplicate_register_is_rejected`, `proto_mismatch_rejected`, `lease_granted_from_free_slices`, `lease_denied_when_in_progress`, `lease_denied_over_capacity`, `lease_release_returns_slices`, `lease_revokes_active_then_grants_after_zero`, `lease_released_when_holder_disconnects`, `windrive_nao_recebe_swap`, and `windrive_pode_lease`; add `foreign_tenant_cannot_release_lease` and `shared_lease_book_preserves_linux_wire_effects`.
- Cover: ≥80% for extracted business logic; existing broker server target unchanged.
- Kahneman: #13/#17; abort on foreign-holder release or any unrelated existing Linux broker regression.

**`crates/ramshared-winsvc/Cargo.toml`**

- What: enable minimum named-pipe, token, ACL, SCM query, and mutex Windows APIs; no networking dependency is added.
- RF / DT: RF-2, RF-3, RF-7; DT-3, DT-5, DT-9.
- Tests: Windows target check and dependency audit proving no TCP server.
- Cover: N/A — manifest.

**`crates/ramshared-winsvc/src/lib.rs`**

- What: export `ipc` on Windows and `package` cross-platform.
- RF / DT: RF-3, RF-7; DT-3, DT-8.
- Tests: crate unit suite.
- Cover: N/A — structural module surface; the exact package suite is bound by the
  `rust-slice-structural-contract-v1` declaration below.

**`crates/ramshared-winsvc/src/config.rs`**

- What: remove `broker: String` and `broker_addr()`. Add fixed-version broker fields
  `broker_pipe: BrokerPipeV1` (deserialize only exact enum value `named_pipe_v1`) and
  `broker_ready_timeout_secs` constrained to `1..=30`; default product config uses 30.
- RF / DT: RF-3; DT-3, DT-4.
- Symbols: `WinDriveConfig`, `validate`, test fixture `GOOD`.
- Before → after: user-configurable socket address → fixed authenticated local transport selector.
- Callers/docs: `product_online.rs`, example TOML, guest/host campaigns.
- Tests: `accept_named_pipe_v1`, `reject_tcp_daily_transport`, `reject_ready_timeout_over_30`, `reject_unknown_broker_fields`.
- Cover: ≥80%.
- Kahneman: #13.

**`crates/ramshared-winsvc/src/product_online.rs`**

- What: replace `TcpStream`/`BrokStream` with `ipc::connect_product_pipe`; treat `Register → Registered` as readiness; keep the same authoritative pipe through release; add DT-7 broker-loss transition without stopping the I/O pump prematurely. Broker process identity is correlated externally through status/evidence, not added to protocol v1.
- RF / DT: RF-3, RF-4, RF-5, RF-6; DT-4, DT-7, DT-12.
- Symbols: `run_product_online`, `BrokStream` (delete local type), heartbeat/watchdog broker-error branches, `error_after_release`.
- Before → after: one-shot five-second TCP connect → authenticated named pipe with 30-second bounded transient retry; broker process identity remains external status/evidence only.
- Callers/docs: SCM and console continue calling the same `run_product_online`.
- Tests: pure/runtime tests `pre_exposure_broker_loss_unwinds`, `online_broker_loss_keeps_pump_until_safe_stop`, `online_pipe_eof_is_not_reconnected`, plus VM drills.
- Cover: N/A — Windows live composition; extracted state/retry helpers ≥80%.
- Kahneman: #15/#16.

**`crates/ramshared-winsvc/src/runtime.rs`**

- What: add `WaitingForBroker`, `Degraded` (diagnostic only before transition to `FailedSafe`), stable broker failure subclasses, product control commands, and pure safe lifecycle/package plans.
- RF / DT: RF-4, RF-5, RF-6, RF-7, RF-8; DT-4, DT-7, DT-10, DT-13.
- Symbols: `RuntimePhase`, `RuntimeErrorClass`, `ProductCommand`, `parse_product_cli`, `run_runtime`.
- Before → after: install/uninstall mutation embedded in `main.rs` and generic broker error → explicit product lifecycle/status/error states.
- Tests: `status_json_parses`, `start_stop_commands_are_explicit`, `online_broker_loss_is_not_replayed`, `unsafe_uninstall_plan_refuses`, existing runtime tests.
- Cover: ≥80%.
- Kahneman: #13/#17.

**`crates/ramshared-winsvc/src/evidence.rs`**

- What: add broker service/instance/pipe/protocol/transition/retry and package manifest/hash fields; enforce 16 KiB lifecycle row cap.
- RF / DT: RF-6; DT-10, DT-12.
- Symbols: `RuntimeEvidence`, `EvidenceWriter::append`, evidence reader/summary.
- Tests: `lifecycle_row_has_broker_identity`, `oversized_lifecycle_row_is_refused`, `status_uses_last_complete_row`.
- Cover: ≥80%.

**`crates/ramshared-winsvc/src/main.rs`**

- What: centralize both-service install/control in package/controller functions; set SCM dependency, service SID/start/failure policies; add `status [--json]`, `start`, `stop`, and transactional `install --manifest <absolute>`/`uninstall`.
- RF / DT: RF-1, RF-2, RF-6, RF-7, RF-8; DT-1, DT-6, DT-8, DT-9, DT-10, DT-11, DT-13.
- Symbols: `windows_svc::{entry, install, uninstall, run_service}`, service constants.
- Before → after: creates/deletes only `RamSharedWinSvc` with empty dependencies → controller for one complete two-service manifest.
- Callers/docs: `Install-RamSharedService.ps1` becomes a thin, compatibility-free wrapper over the product command or is deleted after callers migrate.
- Tests: parser/package unit tests plus guest package/lifecycle drills.
- Cover: parser/planner ≥80%; SCM mutation N/A — Windows E2E.
- Kahneman: #2/#15/#17.

**`crates/ramshared-winsvc/winsvc.example.toml`**

- What: replace `broker = "127.0.0.1:7700"` with fixed
  `broker_pipe = "named_pipe_v1"` and `broker_ready_timeout_secs = 30`.
- RF / DT: RF-3; DT-3, DT-4.
- Tests: config parse and static daily TCP absence.
- Cover: N/A — config.

**`crates/ramshared-winbroker/src/lib.rs` and `crates/ramshared-winsvc/src/package.rs` configuration boundary**

- What: both configs are single-open, 64 KiB capped, UTF-8, `deny_unknown_fields`, and validated into owned values before SCM/pipe/file effects.
- RF / DT: RF-1, RF-3, RF-7; DT-8, DT-9, DT-15.
- Before → after: only winsvc socket config exists → two exact, cross-validated machine configs.
- Tests: broker config parser and package cross-config tests named above.
- Cover: ≥80%.
- Kahneman: #13.

**`scripts/windows/Install-RamSharedService.ps1`**

- What: reduce to a signed product-controller wrapper accepting a manifest path, or delete under ITEM-6 after all callers use `ramshared-winsvc install --manifest`. It must not implement a second SCM transaction.
- RF / DT: RF-1, RF-2, RF-7; DT-8, DT-9, DT-13.
- Before → after: copies one EXE/config and creates one service → delegates one complete manifest install.
- Tests: guest package campaign and static no-second-installer assertion.
- Cover: N/A — wrapper.

**`scripts/windows/Run-GuestProductOnline.ps1`**

- What: remove generation/start/stop of `Lab-LeaseBroker.ps1`; install/start the packaged `RamSharedBroker`, capture its hash/state/evidence, and retain the existing Online/teardown gates.
- RF / DT: RF-1, RF-4; DT-1, DT-3, DT-12.
- Tests: updated guest autonomous lifecycle.
- Cover: N/A — harness.

**`scripts/windows/Run-HostExhaustive.ps1`**

- What: remove the inline PowerShell broker and consume the immutable product manifest; retain watchdog, bounded 64 MiB host policy, SHA, graceful teardown, and terminal cleanup.
- RF / DT: RF-1, RF-4, RF-6; DT-12, DT-14.
- Tests: `Test-HostExhaustiveStatic.ps1` plus new host autonomous orchestrator.
- Cover: N/A — physical harness.

**`scripts/windows/Test-HostExhaustiveStatic.ps1`**

- What: invert `host_broker_required` from requiring the lab broker to forbidding it; require package broker BINARY_MATCH, SCM dependency, named-pipe profile, and consumer-first stop evidence.
- RF / DT: RF-1, RF-2, RF-4; DT-1, DT-3, DT-14.
- Tests: self.
- Cover: N/A — static harness.

### DELETE

No tracked file is deleted at SPEC time. During ITEM-6, delete
`scripts/windows/Install-RamSharedService.ps1` only if repository-wide `rg` proves every caller migrated to the product controller; otherwise keep the thin wrapper with no independent business logic. Generated `Lab-LeaseBroker.ps1` artifacts remain test history but are no longer created by product campaigns.

## Observability

| Signal | Where | Level / type |
| --- | --- | --- |
| `broker_phase` and `broker_instance_id` | broker Event Log + lifecycle JSONL + status JSON | transition / 128-bit correlation |
| authenticated/refused peer | broker Event Log + JSONL | count + stable reason; SID category only, no raw token |
| readiness elapsed | consumer JSONL/status | milliseconds |
| transient connect retry | consumer JSONL/status | count + Win32 class |
| protocol version/tenant registration | both JSONL streams | version + tenant hash/name per existing policy |
| lease grant/release/disconnect | broker and consumer JSONL | lease ID, bytes, explicit vs disconnect |
| product manifest identity | status JSON + campaign artifact | version, commit, SHA-256 |
| SCM service state/start policy/dependency | status JSON + campaign artifact | enum/list |
| `Online`, `Degraded`, `FailedSafe`, `Stopping`, `Stopped` | consumer Event Log + JSONL/status | transition |
| restart budget exhausted | broker Event Log/status | count/60 s + last stable error |
| teardown/residue summary | consumer lifecycle JSONL + harness | duration ms + exact identity counts |

## Living docs

| Document | Action |
| --- | --- |
| `ARCHITECTURE.md` | Alter — add Windows local broker service, named-pipe trust boundary, and ownership split. |
| `docs/decisions/ADR-0006-storport-virtual-miniport.md` | Alter — record named-pipe deployment refinement without changing logical-lease/CUDA ownership decisions. |
| `docs/decisions/ADR-0007-windows-local-broker-ipc.md` | Create — structural decision for two SCM services, named pipe/service SID, and no daily TCP. |
| `docs/reliability/DEGRADATION-MATRIX.md` | Alter — add pre-/post-Online broker loss, restart exhaustion, and package rollback rows. |
| `docs/reliability/GAP-REGISTER.md` | Alter on close only — retain `PARTIAL` until the physical gate; record exact evidence when closed. |
| `docs/specs/no-milestone/windows-autonomous-broker-service/validation.md` | Create/append during ITEM-8/9 closure with VM and physical evidence. |
| `README.md` | Alter — installed topology, demand-start default, Test Mode limitation, and signing gap. |
| Windows install/recovery/rollback runbook | Create or alter under `docs/runbooks/` based on existing Windows runbook inventory in ITEM-1. |
| `docs/BENCHMARKS.md` + `docs/benchmarks/results.jsonl` | Alter — user-requested VM/physical lifecycle comparison only; no throughput/P0 claim. |
| `.claude/rules/*`, `CLAUDE.md`, `AGENTS.md` | N/A — no repository convention change. |

## Implementation order

1. **ITEM-1 — Discovery lock:** record exact Windows SDK APIs/features, current service/package callers, existing runbook target, supported host build, and baseline tests. Add no product behavior.
2. **ITEM-2 — Shared lease state:** add `LeaseBook`, integrate it into Linux `BrokerCore`, and prove wire/state equivalence before creating the Windows broker.
3. **ITEM-3 — Windows broker core and authenticated IPC:** add `ramshared-winbroker`, pure session core, named-pipe/security boundary, config, console gate, and cross-target build.
4. **ITEM-4 — SCM broker lifecycle:** add service entry/status pipe/Event Log/JSONL, exact SID/dependency/failure policies, and isolated VM security/retry drills without exposing a LUN.
5. **ITEM-5 — Consumer integration:** replace TCP with the named-pipe client, use Register as readiness, add externally correlated broker evidence and post-Online broker-loss containment, then run VM storage lifecycle.
6. **ITEM-6 — Transactional product controller:** add manifest/staging/SCM plan/status/start/stop/repair/upgrade/rollback/uninstall and migrate installer callers.
7. **ITEM-7 — Product harness migration:** remove all generated lab brokers from guest/host product campaigns and require package/guest BINARY_MATCH for both Rust services.
8. **ITEM-8 — Full VM gate and docs:** run static/unit/coverage/cross-build plus package, failure, autonomous lifecycle, rollback, and uninstall matrices; update ADR/runbooks/reliability/README; create validation evidence.
9. **ITEM-9 — Physical Test Mode gate:** with explicit supervision and one immutable package, complete three cold boots and final cleanup; only then update the gap register and decide automatic-start promotion.

## Rust slice ownership contracts

<!-- rust-slice-structural-contract-v1
{
  "schema_version": 1,
  "id": "windows-autonomous-structural-rust",
  "kind": "rust-structural-contract",
  "files": [
    "crates/ramshared-broker/src/lib.rs",
    "crates/ramshared-winsvc/src/lib.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-broker/src/lib.rs",
      "package": "ramshared-broker",
      "cargo_test": [
        "cargo",
        "test",
        "-p",
        "ramshared-broker",
        "--lib"
      ]
    },
    {
      "source": "crates/ramshared-winsvc/src/lib.rs",
      "package": "ramshared-winsvc",
      "cargo_test": [
        "cargo",
        "test",
        "-p",
        "ramshared-winsvc",
        "--lib"
      ]
    }
  ]
}
-->

<!-- rust-slice-platform-e2e-v1
{
  "schema_version": 1,
  "id": "windows-autonomous-platform-e2e-rust",
  "kind": "windows-platform-e2e",
  "files": [
    "crates/ramshared-winbroker/src/main.rs",
    "crates/ramshared-winbroker/src/service.rs",
    "crates/ramshared-winsvc/src/bin/ramshared-service-sid-probe.rs",
    "crates/ramshared-winsvc/src/cuda_probe.rs"
  ],
  "verifications": [
    {
      "source": "crates/ramshared-winbroker/src/main.rs",
      "static": {
        "path": "scripts/windows/Test-AutonomousBrokerStatic.ps1",
        "test": "broker_cli_contract"
      },
      "live": {
        "path": "scripts/windows/Run-GuestBrokerService.ps1",
        "test": "scm_start_ready_stop"
      }
    },
    {
      "source": "crates/ramshared-winbroker/src/service.rs",
      "static": {
        "path": "scripts/windows/Test-AutonomousBrokerStatic.ps1",
        "test": "broker_service_contract"
      },
      "live": {
        "path": "scripts/windows/Run-GuestBrokerService.ps1",
        "test": "scm_start_ready_stop"
      }
    },
    {
      "source": "crates/ramshared-winsvc/src/bin/ramshared-service-sid-probe.rs",
      "static": {
        "path": "scripts/windows/Test-AutonomousBrokerStatic.ps1",
        "test": "service_sid_probe_contract"
      },
      "live": {
        "path": "scripts/windows/Run-GuestBrokerService.ps1",
        "test": "legitimate_service_sid_connects"
      }
    },
    {
      "source": "crates/ramshared-winsvc/src/cuda_probe.rs",
      "static": {
        "path": "scripts/windows/Test-AutonomousBrokerStatic.ps1",
        "test": "cuda_probe_uses_local_broker_config"
      },
      "live": {
        "path": "scripts/windows/Run-GuestAutonomousLifecycle.ps1",
        "test": "three_round_sha"
      }
    }
  ]
}
-->

## Required tests matrix

| Production path | Test (`file` :: `name`) | Kind | Kahneman | Cover |
| --- | --- | --- | --- | --- |
| Shared logical lease | `crates/ramshared-broker/src/lease.rs` :: `zero_and_over_capacity_are_denied`, `request_stays_pending_until_explicit_grant`, `grant_may_round_to_slice_capacity`, `second_holder_is_denied`, `wrong_holder_cannot_release`, `lease_book_release_twice_is_one_transition`, `disconnect_cancels_or_releases_only_holder`, `lease_id_wrap_is_refused` | unit | #13/#17 | ≥80% |
| Linux broker equivalence and release authorization | `crates/ramshared-wsl2d/src/broker_srv.rs` :: `register_assigns_stable_id_and_acks_psi`, `duplicate_register_is_rejected`, `proto_mismatch_rejected`, `lease_granted_from_free_slices`, `lease_denied_when_in_progress`, `lease_denied_over_capacity`, `lease_release_returns_slices`, `lease_revokes_active_then_grants_after_zero`, `lease_released_when_holder_disconnects`, `windrive_nao_recebe_swap`, `windrive_pode_lease`, `foreign_tenant_cannot_release_lease`, `shared_lease_book_preserves_linux_wire_effects` | unit/regression | #13/#17 | Touched lease business logic ≥80% |
| Windows broker pure core | `crates/ramshared-winbroker/src/lib.rs` :: `register_is_readiness_without_lease`, `message_before_register_is_refused`, `tenant_mismatch_is_refused`, `one_live_session_only`, `exact_lease_grant_and_release`, `disconnect_releases_server_state_and_audits_ambiguous`, `status_has_instance_and_lease_state` | unit | #13/#17 | ≥80% |
| Broker/config CLI | `crates/ramshared-winbroker/src/main.rs` :: `cli_rejects_relative_config`, `cli_has_no_tcp_listen_option`, `cli_has_no_install_mutation` | unit | #13 | ≥80% parser |
| Product config boundary | `crates/ramshared-winsvc/src/config.rs` :: `accept_named_pipe_v1`, `reject_tcp_daily_transport`, `reject_ready_timeout_over_30`, `reject_unknown_broker_fields` | unit | #13 | ≥80% |
| Pipe retry policy | `crates/ramshared-winsvc/src/ipc.rs` :: `only_not_found_and_busy_retry`, `deadline_stops_retry` | unit | #15 | ≥80% helper |
| Manifest/package planning | `crates/ramshared-winsvc/src/package.rs` :: `manifest_rejects_unknown_and_over_64k`, `artifact_path_cannot_escape`, `hash_must_be_sha256_hex`, `mixed_commit_is_refused`, `broker_capacity_must_equal_lun_size`, `broker_tenant_must_equal_winsvc_tenant`, `same_version_repair_is_idempotent`, `half_registered_candidate_rolls_back_old_definitions`, `uninstall_refuses_owned_storage` | unit | #2/#13/#17 | ≥80% |
| Runtime containment | `crates/ramshared-winsvc/src/runtime.rs` :: `failure_after_lease_releases_once`, `failure_after_cuda_frees_before_release`, `failure_after_create_destroys_before_free`, `failure_after_register_unwinds_reverse`, `deterministic_failure_is_not_retried`, `busy_observation_is_bounded`, `stop_twice_has_one_effect`, `ambiguous_crash_state_is_not_replayed`, `cuda_watchdog_does_not_destroy_stuck_context`, `online_broker_loss_is_not_replayed`, `unsafe_uninstall_plan_refuses` | unit | #13/#16/#17 | ≥80% |
| Evidence/status | `crates/ramshared-winsvc/src/evidence.rs` :: `append_preserves_prior_rows`, `schema_has_no_pointer_or_payload_fields`, `stable_error_redacts_payload`, `each_phase_transition_gets_a_fresh_event_identity_and_timestamp`, `read_all_rows_missing_file_yields_not_found`, `read_all_rows_invalid_json_yields_invalid_data`, `lifecycle_row_has_broker_identity`, `oversized_lifecycle_row_is_refused`, `status_uses_last_complete_row`, `status_never_promotes_stale_evidence_to_current_health` | unit | #9/#13 | ≥80% |
| SCM/IPC peer boundary | `scripts/windows/Run-GuestBrokerService.ps1` :: `legitimate_service_sid_connects`, `administrator_protocol_connect_is_refused`, `unrelated_service_is_refused`, `deny_only_service_sid_is_refused`, `oversized_line_is_refused`, `partial_frame_times_out`, `stop_cancels_blocked_accept`, `stop_cancels_blocked_read`, `status_pipe_rejects_mutation`, `scm_start_ready_stop`, `broker_event_log_transition`, `fourth_failure_remains_stopped`, `deterministic_failure_does_not_restart` | isolated VM | #13/#15/#16 | N/A — Windows E2E |
| Package transactions | `scripts/windows/Run-GuestProductPackage.ps1` :: `FreshInstall`, `Repair`, `ManufacturedRollback`, `UninstallRefusal`, `CleanUninstall` | isolated VM | #2/#17 | N/A — Windows E2E |
| Online broker loss | `scripts/windows/Run-GuestBrokerService.ps1` :: `BrokerLossOnline` | isolated VM | #16 | N/A — Windows E2E |
| Autonomous VM lifecycle | `scripts/windows/Run-GuestAutonomousLifecycle.ps1` :: `cold_boot_no_login`, `three_round_sha`, `consumer_first_stop`, `lease_release`, `zero_residue`, `current_online_evidence_failure_is_red`, `event153_query_failure_is_red`, `recovery_volume_query_failure_is_red`, `guest_lifecycle_forwards_explicit_host_bin_dir`, `psdirect_outer_deadline_is_enforced`, `psdirect_redirected_streams_are_drained`, `psdirect_timeout_terminates_child_tree`, `psdirect_nonzero_child_is_red`, `psdirect_calls_are_session_finally_cleaned`, `deferred_guest_shutdown_preserves_psdirect_result` | isolated VM/static | #9/#13/#15/#16 | N/A — Windows E2E |
| VM PowerShell Direct deadline seam | `scripts/windows/Test-GuestPsDirectDeadlineStatic.ps1` :: `psdirect_outer_deadline_is_enforced`, `psdirect_redirected_streams_are_drained`, `psdirect_timeout_terminates_child_tree`, `psdirect_nonzero_child_is_red`, `psdirect_calls_are_session_finally_cleaned`, `deferred_guest_shutdown_preserves_psdirect_result`, `psdirect_runner_uses_current_host_executable` | manufactured | #13/#15/#16/#17 | N/A — PowerShell seam |
| Static product boundary | `scripts/windows/Test-AutonomousBrokerStatic.ps1` :: `BROKER_BINARY_MATCH`, `SCM_DEPENDENCY_MATCH`, `SERVICE_SID_MATCH`, `DAILY_TCP_LISTENER_ABSENT`, `NO_LAB_BROKER_REFERENCE`; `scripts/windows/Test-HostExhaustiveStatic.ps1` :: `package_broker_required`, `consumer_first_stop_required`, `complete_pass_gate` | static | #13 | N/A — harness |
| Physical immutable package | `scripts/windows/Run-HostAutonomousLifecycle.ps1` :: `three_cold_boots_same_manifest`, `final_preflight_clean`, `resume_marker_is_monotonic`, `cleanup_artifacts_complete`, `intended_payload_corruption_is_red`, `exact_online_identity_required_before_format`, `non_raw_lun_refuses_before_mutation`, `active_pagefile_refuses_before_install`, `configured_pagefile_refuses_before_install`, `pagefile_query_failure_refuses_before_install`, `stop_request_error_is_red`, `bounded_child_terminates_process_tree`, `resume_task_has_one_time_token_without_approval_switch`, `stale_or_replayed_resume_token_is_refused`, `watchdog_shutdown_requires_separate_approval`, `failure_cleanup_disarms_watchdog_and_task` | manufactured + physical supervised | #5/#9/#16/#17 | N/A — environment-bound |
| Physical harness manufactured safety | `scripts/windows/Test-HostAutonomousLifecycleStatic.ps1` :: `intended_payload_corruption_is_red`, `exact_online_identity_required_before_format`, `non_raw_lun_refuses_before_mutation`, `active_pagefile_refuses_before_install`, `configured_pagefile_refuses_before_install`, `pagefile_query_failure_refuses_before_install`, `stop_request_error_is_red`, `bounded_child_terminates_process_tree`, `resume_task_has_one_time_token_without_approval_switch`, `stale_or_replayed_resume_token_is_refused`, `watchdog_shutdown_requires_separate_approval`, `failure_cleanup_disarms_watchdog_and_task` | manufactured | #13/#15/#16/#17 | N/A — PowerShell seam |

Required command gates before ITEM-8 can close:

```text
cargo fmt --all -- --check
cargo test -p ramshared-broker
cargo test -p ramshared-wsl2d
cargo test -p ramshared-winbroker
cargo test -p ramshared-winsvc
cargo clippy -p ramshared-broker -p ramshared-wsl2d -p ramshared-winbroker -p ramshared-winsvc --all-targets -- -D warnings
cargo check -p ramshared-winbroker --target x86_64-pc-windows-msvc
cargo check -p ramshared-winsvc --target x86_64-pc-windows-msvc
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-broker --files crates/ramshared-broker/src/lease.rs --min 80
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winbroker --files crates/ramshared-winbroker/src/lib.rs --min 80
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winsvc --files crates/ramshared-winsvc/src/config.rs,crates/ramshared-winsvc/src/ipc.rs,crates/ramshared-winsvc/src/package.rs,crates/ramshared-winsvc/src/runtime.rs,crates/ramshared-winsvc/src/evidence.rs --min 80
pwsh -NoProfile -File scripts/windows/Test-AutonomousBrokerStatic.ps1
pwsh -NoProfile -File scripts/windows/Test-HostExhaustiveStatic.ps1
./scripts/docs-check.sh
```

The VM and physical commands are environment-bound gates, not substitutes for the unit/static/coverage commands. Missing Windows VM or physical-host evidence leaves this SPEC `PARTIAL`.

## Validation checklist

- [x] `cargo fmt --all -- --check`.
- [x] `cargo clippy` with `-D warnings` for every touched Rust crate.
- [x] Named unit/regression tests pass for `ramshared-broker`,
  `ramshared-wsl2d`, `ramshared-winbroker`, and `ramshared-winsvc`.
- [x] Per-file Rust slice coverage gates are at least 80% for every matrix
  business-logic path.
- [x] Both Windows binaries cross-build for `x86_64-pc-windows-msvc`.
- [x] Static/manufactured Windows physical-lifecycle safety tests emit every required PASS marker.
- [ ] Revalidate legitimate service-SID access and all named refusal/cancellation cases
  pass in the disposable Windows VM.
- [ ] Revalidate package install, repair, manufactured rollback, uninstall refusal, and
  clean uninstall pass in the disposable Windows VM.
- [ ] Revalidate autonomous VM lifecycle before → action → after, BINARY_MATCH,
  three SHA rounds, broker-loss containment, graceful stop, and zero residue.
- [ ] Three corrected physical Test Mode cold boots pass with one immutable
  manifest and one fresh approval per reboot through the approved watchdog harness.
- [ ] README, ADRs, degradation matrix, runbook, gap register, `validation.md`,
  and `IMPL.md` match the evidence without promoting production signing.
- [x] `./scripts/docs-check.sh` and `git diff --check` pass.
