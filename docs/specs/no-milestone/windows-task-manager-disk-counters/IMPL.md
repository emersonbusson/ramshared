# IMPL — Windows virtual disk identity, counters, and performance matrix

> SSDV3 Step 3 · SPEC:
> `docs/specs/no-milestone/windows-task-manager-disk-counters/SPEC.md`

## Status

partial · cover ✓ · E2E VM ✓ / physical env-bound · BINARY_MATCH VM ✓ / physical ✗

The source/manufactured gates and the disposable-VM `.8` Driver Verifier slice
are implemented and live-green. The corrected physical campaign has not run,
so the historical July audit remains discovery evidence only and does not
qualify the current five-cell matrix or physical final active state.

## Files

| Path | ITEM/RF | Change |
| --- | --- | --- |
| `drivers/windows/ramshared/driver.c` | ITEM-4/7 · RF-2/3 | Advertise the bounded virtual adapter transfer contract. |
| `drivers/windows/ramshared/queue.c` | ITEM-5/7 · RF-2/3 | Preserve queue-slot lifetime through request completion. |
| `drivers/windows/ramshared/virtdisk.c` | ITEM-5/7 · RF-2/3 | Apply registered per-LUN queue depth and return StorPort BUSY for temporary exhaustion. |
| `drivers/windows/ramshared/ramshared.inf` | ITEM-4/7 · RF-2/3 | Publish Virtual bus fallback and distinct `.8` DriverVer. |
| `crates/ramshared-winsvc/src/service.rs` | ITEM-5/7 · RF-2/3 | Keep exact RAW and mounted teardown gates fail-closed. |
| `crates/ramshared-winsvc/src/product_online.rs` | ITEM-5/7 · RF-2/3 | Persist current-run lifecycle evidence used by readiness binding. |
| `crates/ramshared-winsvc/src/windows_host.rs` | ITEM-5/7 · RF-2/3 | Require bounded, exact singleton LUN discovery. |
| `scripts/windows/Measure-RamSharedDiskIo.ps1` | ITEM-2/3 · RF-3/6 | Bind serial and size, hash intended bytes, require uncached/counter activity, and emit BOM-free semantic JSONL. |
| `scripts/windows/Invoke-WindowsStorageMatrix.ps1` | ITEM-6/7 · RF-1..7 | Add bounded five-cell orchestration, exact identity, recovery journal, pagefile refusal, evidence manifests, regression policy, and terminal cleanup. |
| `scripts/windows/Copy-RamSharedProtectedEvidence.ps1` | ITEM-7 · RF-6/7 | Add canonical containment, reparse/bounds refusal, PowerShell 5.1-safe staged copy/cleanup, and paired source/destination hash inventory. |
| `scripts/windows/Invoke-WindowsDiskCounterAudit.ps1` | ITEM-2/7 · RF-3/6 | Retire the legacy live wrapper so it cannot select an unrelated artifact or qualify the current matrix. |
| `scripts/windows/Run-GuestAutonomousLifecycle.ps1` | ITEM-5/7 · RF-2/3/6 | Bind current PID/run Online identity and fail closed on Event 153 or recovery-volume query failure. |
| `scripts/windows/Test-RamSharedDiskIoStatic.ps1` | ITEM-2/3 · RF-3/6 | Add executable manufactured false-green tests. |
| `scripts/windows/Test-WindowsStorageMatrixStatic.ps1` | ITEM-6/7 · RF-1..7 | Add static and executable manufactured matrix/refusal tests. |
| `scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1` | ITEM-2/4/7 · RF-2/3/6 | Assert driver, counter, queue, and identity contracts. |
| `scripts/windows/New-Win11LabAutounattend.ps1` | ITEM-5 · RF-2/6 | Bind one local administrator to supported OOBE AutoLogon with exact `LogonCount=1`. |
| `scripts/windows/Win11LabMediaContract.ps1` | ITEM-5 · RF-2/6 | Validate the sealed OOBE account binding, preserve caller parameters, and bound media workers plus external tools. |
| `scripts/windows/New-WindowsNoPromptIso.ps1` | ITEM-5 · RF-2/6 | Separate probe/copy budgets and run `oscdimg` through a bounded process-tree guard. |
| `scripts/windows/New-Win11Wsl2LabVm.ps1` | ITEM-5 · RF-2/6 | Refuse unsupported Windows 11 setup memory below 4 GiB before mutation. |
| `scripts/windows/Test-Win11LabMediaContractStatic.ps1` | ITEM-5 · RF-2/6 | Add executable OOBE, dot-source, slow-media, tool-timeout, and setup-memory refusals. |
| `scripts/windows/New-Win11LabReadyClone.ps1`<br>`scripts/windows/Test-Win11LabReadyCloneStatic.ps1` | ITEM-5 · RF-2/6 | Create independently identified differencing-VHD clones from the immutable ready base; bind vTPM, sealed switch, full integration-service IDs, and the correct offline contact state. |
| `scripts/windows/Run-GuestExhaustive.ps1`<br>`scripts/windows/Test-GuestExhaustiveStatic.ps1` | ITEM-5 · RF-2/3/6 | Run the signed normal/Verifier I/O passes and phase-bound exact ROOT/service/OEM/retired-node teardown with typed reboot boundaries. |
| `scripts/windows/Recover-GuestVerifierExactRun.ps1`<br>`scripts/windows/Test-RecoverGuestVerifierExactRunStatic.ps1` | ITEM-5 · RF-2/3/6 | Seal read-only recovery plans and execute the same split teardown transaction without broad repair or deliberate failed intermediate actions. |
| `scripts/windows/Set-Win11LabDriverTestFirmware.ps1`<br>`scripts/windows/Wait-Win11LabReady.ps1` | ITEM-5 · RF-2/6 | Bound guest-only Secure Boot transitions and independently prove exact host/guest readiness plus zero residue before and after the campaign. |

## Validation (numbers)

- Rust build/lint/tests: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` → exit 0. The workspace suite passed; privileged CUDA/Vulkan/ublk cases remained explicitly ignored.
- Slice tests: coverage invocation ran 122 `ramshared-winsvc` library tests → 121 passed, 1 CUDA-only ignored, 0 failed.
- Cover: `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winsvc --files crates/ramshared-winsvc/src/service.rs --min 80 --report-json tmp/windows-task-manager-disk-counters-cov.json` → exit 0; `service.rs` 85.4% lines (`603/706`).
- WDK build and Code Analysis: `Build-Drivers.ps1 -KitVersion 10.0.26100.0 -CodeAnalysis` with `/W4 /WX` → exit 0; `ramshared.sys` 33,280 bytes, SHA-256 `816dc594aacf4b97e1c7f3130a546f8038b226f09494a40d87a61cad8678f1dd`; `poolstress.sys` 8,192 bytes, SHA-256 `8e498eda524ecc5b13fb5fc913457713a279395a28f5b5d4b16023b3accc3cf1`.
- InfVerif: WDK 10.0.26100.0 x64 `InfVerif.exe /w drivers/windows/ramshared/ramshared.inf` → exit 0, no findings.
- Native Windows Rust: MSVC release build of `ramshared-winbroker` and `ramshared-winsvc` → exit 0 in 64 seconds; `ramshared-winsvc.exe` 1,744,384 bytes/SHA-256 `e42f74ee1ad214e5e8ab828218841b524fbf0d72bae118bd38886cae4fbca8fd`, `ramshared-winbroker.exe` 961,024 bytes/`0ec4bb8332329cb7b4d4fdbeabaf6de5de2abe8c410c9bdb4fa83cae25c83ab2`, SID probe 612,864 bytes/`d3729400aa729fc9b66fa6a78caeb43bb4fa9fe9cbd9f4eeffb487521efdc5ef`.
- Native Windows clippy: the first UNC run exposed an incremental-lock incompatibility; rerun with `CARGO_INCREMENTAL=0` and an isolated target directory passed `--all-targets -- -D warnings` in 54 seconds. No deployed binary was overwritten.
- Test-signed VM package: Inf2Cat reported zero errors/warnings; SignTool sign and `/pa` verification passed for the `.sys` and catalog. Staged package hashes: `ramshared.sys` 34,704 bytes/`5e4ff79148274ec1a029a057714f0066389b6e103f5425e5c3c2aab1adb07a55`, `ramshared.inf` 1,839 bytes/`6ce0ee05ead79ed096ba70155e6d24cc7ca573f99204c97f5b1c3642d6853ebf`, `ramshared.cat` 2,396 bytes/`56fa048229bfd8f4a9735ca5c084e4cf9598172025957b1b225cf8d61b19c230`.
- PowerShell 5.1 parser: matrix/probe production and static files → `PS51_PARSE_OK count=4`, exit 0.
- Disk-I/O manufactured suite: `powershell.exe ... -File scripts/windows/Test-RamSharedDiskIoStatic.ps1` → exit 0; exact identity, intended-payload corruption, uncached failure, zero activity, expected size, raw fields, and BOM-free JSONL contracts passed.
- Matrix manufactured suite: `powershell.exe ... -File scripts/windows/Test-WindowsStorageMatrixStatic.ps1` → exit 0; final marker `PASS Test-WindowsStorageMatrixStatic`.
- Windows disk/static suite: `Test-WindowsDiskCounterAuditStatic.ps1` → 30/30 PASS lines; this includes legacy retirement, provider failure, current-run identity, immutable guest binary staging, traversal/reparse, bounded copy, paired hashes, and PowerShell 5.1-safe cleanup contracts.
- Protected evidence live copy: the first legitimate run exposed a PowerShell 5.1 parent-path failure and orphan empty staging; after TDD correction, the same operator command published 90 files/945,121 bytes. Independent verification recomputed 90/90 source and destination hashes; inventory SHA-256 `03c714cc7a262bdcb5c82eae1fb4fca2ff84c6b41ab436863997bcc32dd19cd8`. Seven validated empty staging directories were removed and zero remain.
- Windows static aggregate: `Test-WindowsCiStatic.ps1 -RepoRoot <repo>` → exit 0 and `PASS windows_static_suite_runs_named_static_harnesses`; all six fixed source-only harnesses passed.
- Corrected OOBE/media TDD: five genuine REDs were observed for missing `LogonCount`, caller `StagingRoot` clobbering, a shared 90-second bulk-copy deadline, unbounded `oscdimg`, and unsupported 2 GiB Windows 11 setup. The focused media suite then passed all 30 named markers; the full Windows static aggregate exited 0.
- Sealed media: output ISO 8,454,309,888 bytes/SHA-256 `EE07B0766773105C22E952658FBDED018A1894846123DFF23991D6281E34A785`; external and embedded XML both SHA-256 `8C22438E54B7E4319D2AB454627E7DB6014AAF6B7DE16BABEC03818F368CF61C`; EFI no-prompt image present.
- Live VM before/action/after: `clean-5` before remained `IMAGE_STATE_UNDEPLOYABLE`, `OOBEInProgress=1`, `SetupPhase=4`, `SetupType=2`; corrected `clean-6` creation bound the new ISO and exact VHD. A 2 GiB diagnostic start produced the legitimate Windows 11 minimum-memory refusal; the supported 4 GiB retry then failed safe with Hyper-V `0x800705AA`. After: both RamShared guests Off, exact `clean-6` DVD retained, zero physical-host/WSL reboot, and no mutation to the unrelated 12,288 MiB `gha-ubuntu-2404` VM. Evidence: `tmp/windows-task-manager-disk-counters-e2e/20260810-oobe-media-validation.json`.
- Immutable clone and memory boundary: the 20,505,952,256-byte base remained SHA-256 `1F17888E525553810881E835FB2E3B8F7C74B9A4EAEC3481F7BCE8A118B63EC2`; `clean-9` received a new VM ID, differencing VHD, vTPM, four vCPUs, Private switch, and zero checkpoints. Hyper-V legitimately refused its first 4 GiB start with `0x800705AA` while host-free memory was 4,336,263,168 bytes, WSL used 12,280,619,008 bytes, and WSL swap used 4,284,153,856/4,294,967,296 bytes. Gracefully shutting down only completed RamShared `clean-7`/`clean-8` guests raised host-free memory to 10,003,623,936 bytes; no foreign process, WSL, or physical host was restarted.
- Disposable-VM E2E: readiness passed first complete attempt in 110,401 ms; Secure Boot On→Off passed; the signed `.8` driver loaded with SHA-256 `5E4FF79148274EC1A029A057714F0066389B6E103F5425E5C3C2AAB1ADB07A55` and BINARY_MATCH true. Normal I/O passed in 87,690 ms and Verifier I/O in 25,347 ms. Each proved the legitimate queue, six refusals, three race/rundown guards, exact 128 MiB Virtual/SSD/4 KiB identity and VPD serial, Event 153=0, and new dumps=0. Verifier reset, exact ROOT/service/OEM/retired-node teardown, certificate removal, and TestSigning disable all completed with exit 0; two independent final observations reported package/service/ROOT/disk/PnP/Verifier/certificate/TestSigning counts zero. Secure Boot Off→On and final readiness passed in 92,229 ms; the clone was gracefully left Off. Evidence: `tmp/windows-task-manager-disk-counters-e2e/20260810-vm-verifier-final.json` and `C:\ramshared\artifacts\guest-verifier-d0f9a571-c7bb-4f78-9e40-aa7233ed85e6`.
- Exact failed-run recovery: the prior run `3b13195c-4e40-4467-b079-9ac228aa8bf1` exposed the retired-node classifier and PowerShell scalar-unwrapping defects. The sealed READY plan `guest-verifier-recovery-0f834fc6-694b-4202-a949-ad413ad78db9` bound the exact package/ROOT/service/hashes; action `guest-verifier-recovery-156cd553-535f-4fe6-8383-f35ba823345f` removed only those identities and proved all final counts zero without reinstalling Windows.
- SPEC matrix: source/static names are present and the executable manufactured guards passed. The disposable-VM Driver Verifier, refusal, recovery, firmware, BINARY_MATCH, and final-readiness rows now have live proof. Physical-only five-cell matrix rows remain open and are not represented as VM or unit proof.
- E2E before: read-only host observation on 2026-08-09 found the `ramshared` driver Running/Manual, both product services Stopped/Manual, zero RamShared disks, zero RamShared scheduled tasks, and zero armed watchdog markers.
- E2E action/after: disposable-VM package install, service/ROOT start, normal and Verifier I/O, guest-only reboot boundaries, exact teardown, firmware restoration, and zero-residue readiness are live-green. No physical-host install, storage mutation, reboot, or 75-sample campaign was performed in this validation window.
- BINARY_MATCH: VM loaded DriverStore image matched the immutable `.8` SYS SHA-256 `5e4ff79148274ec1a029a057714f0066389b6e103f5425e5c3c2aab1adb07a55`. The physical host remains an explicit mismatch: loaded image SHA-256 `033ce3f8be36b0dc01d731ea29c6af8259a11d398f7e1f863bf01aa1786dd1c9`; therefore physical BINARY_MATCH and overall DONE still fail.

## Gaps

- open: the successful WDK/Code Analysis/InfVerif outputs must be sealed with the final immutable package before deployment; they are build proof, not loaded-image proof.
- closed: disposable-VM identity, signed Driver Verifier, six refusal classes, BINARY_MATCH, Event 153=0, dump=0, exact failed-run recovery, teardown, Secure Boot restoration, and final zero readiness are live-proven on a fresh differencing clone.
- env-bound: supervised physical five-cell campaign must produce exactly 75 unique samples, 15 summaries, per-cell context/evidence manifests, zero Event 153, integrity, counter activity, regression verdicts, and complete cleanup.
- open: deploy the sealed `.8` package under supervision, then prove installed and loaded driver/broker/winsvc hashes match its immutable manifest before any active claim. The current loaded-driver mismatch requires a later approved reboot; it must not be repaired in place.
- open: Task Manager remains secondary human observation; Windows Storage APIs and direct unbuffered I/O are the canonical truth.

## Rollback trigger

Rollback to the last independently verified immutable manifest if any loaded
hash differs, any cell reports Event 153, checksum mismatch, timeout,
configured/active pagefile, ambiguous identity, more than 20% throughput loss,
more than 2× p99 latency, stop over 30 seconds, nonzero residue, or incomplete
artifact inventory. Preserve the failed campaign evidence and leave the product
stopped when a safe verified final active state cannot be established.

## Traceability

| RF | ITEM | Commit |
| --- | --- | --- |
| RF-1, RF-2, NFR-1 | ITEM-1/3/7 | pending supervised commit |
| RF-3, RF-7 | ITEM-2/3/5/7 | pending supervised commit |
| RF-4 | ITEM-4/5 | pending supervised commit |
| RF-5, RF-6, NFR-3 | ITEM-6/7/8 | pending physical evidence and supervised commit |
| NFR-2 | ITEM-3/5/8 | pending VM/physical evidence and supervised commit |
