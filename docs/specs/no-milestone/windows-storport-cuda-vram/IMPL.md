# IMPL — Windows StorPort I/O backed by CUDA VRAM

> SSDV3 Step 3 · SPEC: `docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md`

## Status

**PASS (storage-only product on isolated GPU-PV guest)** — campaign
`guest-product-online-20260716-220848` on `win11-drill` completed three fresh lifecycle rounds with
real RTX 2060 CUDA, DriverStore/package `BINARY_MATCH` on signed miniport
`97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F`, one SHA write/read per
lifecycle, graceful stop, correlated lease release, CUDA free restoration, no new dumps, VM Off, and
host GPU OK. Evidence: `evidence/guest-product-online-20260716-220848.md`.

Isolated guest ITEM-3 with **exact VPD serial + size** and Driver Verifier is also **PASS** for the
same signed miniport (`guest-exhaustive-20260716-224913`). Evidence:
`evidence/guest-exhaustive-20260716-224913.md`.

The earlier campaign `guest-product-online-20260716-174238` is retained only as a historical
successful run; its promotion claim is invalidated by the later audit (CREATE-only stop identity,
registry-only pagefile authority, unbounded lock worker, and an incomplete harness verdict).

Earlier aggregate PASS values that used size/name or PnP-presence VPD fallbacks stay **invalidated**.
The corrected campaign requires unique vendor/product, exact serial `ABCDEF0123456789`, and exact
CREATE size (capacity via `IOCTL_DISK_GET_LENGTH_INFO`, not CHS `Win32_DiskDrive.Size`).

**Physical Online preflight (2026-07-16): RED — Online not executed.**

| Gate | Result |
| --- | --- |
| Guest ITEM-3 + Verifier | **PASS** (`guest-exhaustive-20260716-224913`; all required verdicts = 1 including `VPD_SERIAL_MATCH`) |
| Package miniport SHA | `97FD7B37…` (lifecycle + exact VPD image; BINARY_MATCH package↔guest) |
| Installed daily-host miniport SHA | `E690306F…` (**≠ package**; BINARY_MATCH fail) |
| Installed backup `.bak-host` | **MISSING** (prior replace attempt access-denied) |
| `RamSharedCtl` CreateFile | OK err=0 |
| PnP adapter/disk | OK/OK but Get-Disk RAMSHARE count=0 (orphan risk) |
| winsvc userspace binary | `AAD45668…` for `C:\ramshared\bin\ramshared-winsvc.exe` in the passing GPU-PV guest campaign |
| README daily-host policy | Windows kernel driver = **lab VM only** |
| Online limited (64 MiB / S:) | **SKIPPED** (preflight RED) |

Promotion beyond this storage-only gate still excludes: SDV (tool unavailable locally; WDK
headers/targets present but `sdv.exe` not installed) and the isolated WSL2 freeze-elimination
campaign. **StartIo READ-copy race is claimed** (2026-07-17; Verifier `0x2093B` on
`win11-drill` — see StartIo section below). WDK Code Analysis is project-clean for
`drivers/windows/ramshared/*.c`; evidence: `evidence/code-analysis-project-clean-20260716.md`.
The daily Windows host remains under lab-only kernel-driver policy and was not mutated for
promotion.

**Physical Online (2026-07-16 read-only recapture):** still **RED/SKIPPED** — package
`CD7E315D…` ≠ installed `E690306F…`, and README forbids loading the Windows miniport on the daily
host. Evidence: `evidence/physical-preflight-readonly-20260716T172150Z.txt`.

**GPU-PV lab (win11-drill, later 2026-07-16):** after offline UMD copy, guest `nvidia-smi`,
bounded `probe-cuda`, and product Online are **PASS** on real RTX 2060. The closing campaign
`guest-product-online-20260716-220848` supersedes the earlier smoke by proving three fresh
Online/CUDA/I/O/stop lifecycles on the current signed miniport. Evidence:
`evidence/gpupv-probe-cuda-pass-20260716T173812Z.md` and
`evidence/guest-product-online-20260716-220848.md`.
Earlier partial/timeout closeout remains historical: `evidence/gpupv-safe-close-20260716T025812Z.txt`.

**Guest product Online (2026-07-16, campaign 145248):** **PARTIAL** — Online + exact 64 MiB
RAMSHARE LUN + 3-round SHA **PASS**; graceful stop force-killed after 60s. BINARY_MATCH guest
package `CD7E315D…`. Evidence: `evidence/guest-product-online-20260716-145248.md`.


## Senior audit correction

The previous implementation and report contained unsafe or unsupported conclusions:

- It dismounted configured/candidate drive letters before proving exact LUN identity.
- A volume-lock failure was treated as a soft condition before destructive teardown.
- A refused code-7 stop could drop live owners while SCM continued to report `Running`.
- Synchronous CUDA operations had no independent 5-second observation watchdog.
- Startup could replay `DESTROY` from diagnostic evidence and lease-error paths could leak ownership.
- The Windows `OVERLAPPED` object could leave scope while cancelled I/O was still outstanding.
- Run/event identifiers were reusable, I/O metrics were placeholders, and config reading reopened the
  file after checking it.
- The IOCTL harness accepted size-based VPD identity and declared `STATUS=PASS` while four mandatory
  verdicts remained zero.
- Event 41/6008 and historical Event 51 data did not prove the asserted freeze root cause. That earlier
  causal statement is retracted; it remains a risk hypothesis pending disk-number correlation and a
  captured trace from the incident.

## Implemented correction

| Area | Result |
| --- | --- |
| Teardown identity | Require exactly one `RAMSHARE` / `VRAMDISK` / 16-hex serial / configured-size target before Gate A or any mutation. `Get-Disk`'s standard trailing `SCSI Disk Device` suffix is parsed without weakening the vendor/product pair. Candidate-letter dismount was removed. |
| Destructive gates | Identity → pagefile Gate A → drain → exclusive volume lock → pagefile Gate B → flush/dismount → unregister/destroy. Query, identity, or lock ambiguity refuses stop. |
| Re-audited identity | A bounded observation binds configured letter → unique disk → exact vendor/product/VPD serial → configured `Get-Disk.Size`. The product stop path does not require `PhysicalDriveN` length IOCTL because that handle can deny access during teardown; the external VPD harness remains the IOCTL capacity oracle. |
| Re-audited pagefile authority | Gate A/B require the fail-closed union of configured Registry `PagingFiles` and actual `Win32_PageFileUsage`; either source error, timeout, malformed entry, or matching path refuses. |
| Lock deadline | The I/O pump remains active while locking. At 30 seconds an in-flight mutating worker enters FailedSafe and continues I/O/lease heartbeat until supervised reboot; a completed late result is released and never crosses the destructive frontier. |
| Campaign verdict | Three fresh lifecycle rounds; each requires Online, Driver-Store BINARY_MATCH, one SHA write/read, exit 0, no force-kill, correlated lease release, CUDA restoration, no new dump, and teardown <=30 seconds. Any failed round aborts without retry. |
| INF isolation | DIRID 13 + `ServiceBinary=%13%\ramshared.sys` + build 16299 model floor. WDK 10.0.26100.0 `InfVerif /w` exits 0; canonical harnesses no longer create a parallel System32 RamShared service. |
| Allocation/release order | After destroy, the backend is consumed so `DeviceMem` drops explicitly; `mem_info` must restore free bytes within 64 MiB before `LeaseRelease`. A release write/flush failure retains the lease, becomes ambiguous, and is never replayed. |
| Refused stop | Code 7 retains driver/CUDA/lease owners, returns to Online I/O, and permits a later supervised stop attempt. |
| CUDA watchdog | Independent observer marks an operation failed-safe after 5 seconds. It never destroys a context, LUN, allocation, or lease while synchronous CUDA may still be outstanding. |
| Startup/unwind | No evidence-driven replay. Partial startup unwinds in reverse; release failures are surfaced. If safe destruction cannot be established, ownership is deliberately preserved. |
| Windows I/O lifetime | Partial queue allocations unwind. `CancelIoEx` is followed by a blocking `GetOverlappedResult` drain before stack `OVERLAPPED` storage can leave scope. |
| Config boundary | One no-follow Windows handle performs the check and bounded read; there is no close/reopen race. PowerShell identity/pagefile helpers are time-bounded. |
| Evidence/metrics | Collision-resistant process run IDs, fresh event IDs/timestamps, requested bytes, real operation counters, bounded latency samples, and release/error reporting. |
| VM harness | Every guest command has a real timeout; readiness uses elapsed wall time; errors stop the isolated VM. Verifier must be active and the driver `RUNNING` before pass 2. |
| IOCTL verdict | Exact unique identity is mandatory and every SPEC ITEM-3 verdict is required for `STATUS=PASS`; the corrected harness rejects size/name and PnP-presence VPD fallbacks. |
| Queue rundown | `QSubmit` and `QCommitAndFetch` acquire `IoRundown` for mapped ring/data access; release before long-lived pend; outer hold covers READ copy (no nested acquire); `QTeardownOnCrash` waits before unmap. Failed/Closing refuse new work. Reserved CQE fails closed. |
| Concurrent injectors | Guest harness implements reserved-CQE, completion re-entry, and UNREGISTER-vs-pended-COMMIT probes with dual-handle UNREGISTER. Static RED/GREEN: `Test-WinDriveIoctlValidationStatic.ps1`. |
| Virtual miniport | `HW_INITIALIZATION_DATA` + `STOR_FEATURE_VIRTUAL_MINIPORT` + `HwAdapterControl` + `HwFreeAdapterResources`; FindAdapter does not clear Storport Master/SG/NeedPhysical; HwStartIo dispatches PnP/Power SRBs. Before CREATE, REPORT LUNS is empty and INQUIRY/capacity return NO_DEVICE so no placeholder PDO identity is cached. CREATE/DESTROY use `BusChangeDetected` for the real absent↔present transition. VPD 0x80 copies only a validated uppercase 16-hex serial; INQUIRY honors short allocation; READ CAPACITY(10/16) is implemented. |
| Guest deploy | INF + SetupAPI root-enum, targeted stale DriverStore purge for published INFs whose original name is `ramshared.inf`, mandatory reboot after package install, and post-reboot SetupAPI recreate if `ROOT\RAMSHARED\0000` is absent before IOCTL. The harness must prove both root PnP and `SCSIAdapter` are `OK|problem=0` before any CREATE. |

## Verification

### Green gates

```text
cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets
  ramshared-block: 41 passed
  ramshared-cuda: 5 passed, 1 ignored (live GPU)
  ramshared-winsvc: 78 passed, 1 ignored (live CUDA)

cargo clippy -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets -- -D warnings
  PASS

cargo clippy -p ramshared-winsvc --target x86_64-pc-windows-msvc --all-targets -- -D warnings
  PASS

cargo fmt --package ramshared-winsvc --package ramshared-cuda -- --check
  PASS
```

Rust slice coverage:

| Slice | Coverage |
| --- | ---: |
| broker tenant | 85.9% |
| config | 95.5% |
| driver link | 87.7% |
| evidence | 91.9% |
| runtime | 86.8% |
| service | 84.9% |
| CUDA probe | 80.0% |

PowerShell parser validation passes for `Run-GuestExhaustive.ps1` and
`Invoke-WinDriveIoctlValidation.ps1` under Windows PowerShell 5.1.

The canonical WDK build uses `/W4 /WX /wd4324 /Z7`; the x64 build returned
`BUILD_DRIVERS_OK`. WDK Code Analysis is project-clean for repository driver files; WDK header
analyzer warnings remain toolchain-scope noise. Static Driver Verifier and Linux `checkpatch.pl`
were not available in this local tool image, so no SDV/checkpatch PASS is claimed. The Windows MSVC
toolchain build of `ramshared-winsvc` produced SHA `AAD45668…`.

### Isolated VM evidence (current signed miniport; VPD result invalidated)

Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260715-214831`
Binary: `ramshared.sys` SHA256 `1E57690EA63E6287D4790A134544DC9F46253BB356D1C2B3B1D65FC812F30CFF`
Repo evidence: `evidence/ioctl-guest-verdict-injectors.json`, `evidence/ioctl-guest-verdict-verifier.json`,
`evidence/ioctl-guest-summary-verifier.json`

```text
IOCTL_PASS1=PASS
IOCTL_VERIFIER=PASS
VERIFIER_RAN=true
GUEST_EXIT=0
```

Both historical passes reported every ITEM-3 verdict = 1, but `VPD_SERIAL_MATCH` is no longer
accepted because the old harness admitted non-serial fallbacks. The three concurrent probes,
remaining refusal verdicts, and `NO_NEW_DUMP` remain recorded. Pass 2 ran with Driver Verifier
**active** on `ramshared.sys`
(flags `0x2093B`: special pool, force IRQL, pool tracking, I/O, deadlock, security, misc, DDI;
DMA checking omitted for virtual StorPort). `verifier /query` showed
`MODULE: ramshared.sys (load: 1 / unload: 0)`. No new minidumps. VM stopped; verifier reset
best-effort.

Harness notes that made Verifier boot reliable:

- Guest OS reboot via `shutdown /r` (not only `Restart-VM -Force`) so Verifier settings apply.
- PSD wait after Verifier boot up to 600s.
- Avoid `sc delete`/`delete-driver` thrash on every deploy.

Concurrent probe honesty: ring/IOCTL concurrency, not full StartIo READ-copy race. The exact VPD
identity path must be fixed and the corrected campaign rerun before ITEM-3 can return to PASS.

### Corrected exact-VPD rerun

Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260716-104650`

```text
IOCTL_PASS1=FAIL
IOCTL_VERIFIER=FAIL
VERIFIER_RAN=true
VPD_SERIAL_MATCH=0 (both passes)
all other required verdicts=1 (both passes)
NO_NEW_DUMP=1 (both passes)
GUEST_EXIT=2
```

Verifier flags were `0x2093B`; `ramshared.sys` was loaded once and actively verified. The corrected
harness SHA was `6D7B2DC1…`, and the guest driver SHA was `1E57690E…`. The guest exposed multiple
RAMSHARE PnP identities but no authoritative candidate with vendor/product + serial
`ABCDEF0123456789` + 128 MiB size. This is an honest VPD product gap, not a harness timeout.

Terminal state: `win11-drill` Off, verifier reset best-effort, one bare GPU partition adapter with
empty partition values, zero assignable devices, and host RTX 2060 `OK`.

Repo evidence: `evidence/ioctl-guest-summary-exact-vpd.json`,
`evidence/ioctl-guest-verdict-exact-vpd.json`,
`evidence/ioctl-guest-verdict-exact-vpd-verifier.json`, and
`evidence/ioctl-guest-exact-vpd-console.txt`.

### Signed absent→present lifecycle + exact VPD PASS

Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260716-224913`

The current signed image (`97FD7B37…`, `/W4 /WX /wd4324`, test-signed) was deployed only to the
isolated guest with targeted DriverStore purge, post-deploy reboot, SetupAPI root-device recreate
when needed, and exact-identity harness using `IOCTL_DISK_GET_LENGTH_INFO` for capacity.

| Field | Value |
| --- | --- |
| IOCTL_PASS1 | **PASS** |
| IOCTL_VERIFIER | **PASS** |
| `VPD_SERIAL_MATCH` | **1** (both passes) |
| Serial / size | `ABCDEF0123456789` / `134217728` |
| Verifier | `0x2093B`, `ramshared.sys` load 1 / unload 0 |
| Dumps | none new |
| Terminal | VM Off; bare GPU-PV; host RTX 2060 OK |

Prior intermediate campaigns (`104650`, `111439`, `114124`) correctly failed until ghost cleanup and
the capacity surface fix landed. The historical `120459` pass remains valid for `CD7E315D…`, but
the current package proof is `224913`. Evidence:
`evidence/guest-exhaustive-20260716-224913.md`, `evidence/vpd-exact-pass-20260716.md`,
`evidence/terminal-state-vpd-pass-20260716T170631Z.md`,
`evidence/wdk-build-audit-20260716T171026Z.md`,
`evidence/vpd-lifecycle-package-20260716-111336.json`, and `evidence/ioctl-guest-*-vpd-pass*`.


### StartIo READ-copy race (2026-07-17 **CLAIMED**)

Harness fix: early post-CREATE StartIo with **live** `StartQueuePump` until after
PhysicalDrive READ + second-handle UNREGISTER. MSFT_Disk appears only while
enumeration READs are pumped (product Online pattern); CreateFile without the
pump hangs the class stack.

| Campaign | Verifier | STARTIO | Key line |
| --- | --- | --- | --- |
| `startio-probe-20260717-092819` | no | 1 | `readOk=1 drained=4 sq=4/4 unregOk=1` |
| `startio-verifier-20260717-092950` | `0x2093B` | 1 | `readOk=1 drained=5 sq=5/5 unregOk=1`; load 1/unload 0 |

Package SHA `97FD7B37…`. Evidence: `evidence/startio-claim-20260717.md`,
`evidence/startio-probe-20260717-092819/`, `evidence/startio-verifier-20260717-092950/`.

### Manufactured pagefile Gate A refusal (2026-07-17)

| Layer | Evidence |
| --- | --- |
| Unit (product teardown path) | `manufactured_pagefile_on_product_volume_refuses_gate_a` — Gate A refuse, code 7, no UNREGISTER/DESTROY, Online retained |
| Static | `scripts/windows/Test-PagefileRefusalManufacturedStatic.ps1` |
| Guest lab inject | `Invoke-PagefileRefusalManufactured.ps1` on win11-drill: registry PagingFiles inject for letter `S:`, restore OK (`pagefile-refusal-20260717-095826`) |

Live product Online + stop with mid-lifecycle inject is **PASS**
(`guest-product-online-20260717-102614` / `pagefile-online-refuse-20260717-102614`):
`gate_a_active: S:\pagefile.sys`, Online resumed, then clean stop exit 0, lease release,
CUDA restore, no dump. Flag: `Run-GuestProductOnline.ps1 -ManufacturedPagefileRefuse`.

### SDV probe (2026-07-17)

`Invoke-SdvProbe.ps1`: `sdv.exe` not on PATH; MSBuild `/t:sdv` → MSB4057 (target missing).
WDK has `WindowsDriver.Sdv.targets` but SDV tool package is not installed.
**SDV_CLAIM=NOT_CLAIMED.** Evidence: `evidence/sdv-probe-20260717/`.

## Remaining promotion gates

1. ~~Manufactured active-pagefile refusal~~ — unit + guest inject + **live Online+stop** PASS.
2. Keep physical Online blocked by the lab-only policy; do not reinterpret guest proof as daily-host
   authorization.
3. ~~StartIo READ-copy race~~ — **claimed** on win11-drill under Verifier (see above).
4. Run the isolated WSL2 freeze campaign before any freeze-elimination claim.
   Scaffold: `scripts/safety/wsl2-freeze-campaign.sh` — dry-run baseline + gate refuse on
   daily host; full 2× before→action→after only with `--allow-isolated-lab`,
   `RAMSHARED_ISOLATED_LAB=1`, and `--run-isolated` (cgroup-bounded pressure + watchdog).
   Static: `scripts/safety/Test-Wsl2FreezeCampaignStatic.sh`.
5. SDV PASS — install Static Driver Verifier component for WDK 10.0.26100, re-run
   `Invoke-SdvProbe.ps1` / MSBuild `/t:sdv`, record defects=0.

The WSL2 freeze claim is **BLOCKED, not PASS**. Promotion requires a dedicated isolated
before→action→after campaign with watchdog/timeout, swapoff-first, ghost/deleted-plus-used-kB checks,
`BINARY_MATCH`, D-state/hung-task capture, two idempotent repetitions, and cleanup. That campaign was
not run on the daily WSL2 host because repository policy forbids live thrash pressure there.

## Rollback triggers

- BugCheck, new dump, checksum mismatch, or exact-identity ambiguity.
- Pagefile observation/query ambiguity or exclusive-lock failure.
- Any CUDA operation observed above 5 seconds or teardown above 30 seconds.
- CUDA capacity not restored within 64 MiB after a clean teardown.
- Loss of guest control under Driver Verifier.

No automatic commit, merge, host reboot, driver replacement, or physical-host destructive campaign was
performed.
