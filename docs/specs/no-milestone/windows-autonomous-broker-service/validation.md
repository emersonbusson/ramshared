# Validation — autonomous Windows broker service

## What

SSDV3 Step 3 validation of shared sender-bound leases, the native Windows
broker, authenticated named pipes, SCM supervision, transactional two-service
packaging, autonomous storage lifecycle, broker-loss containment, VM cold
boots and the supervised physical-host campaign.

## Measured data

| Gate | Result |
| --- | --- |
| fmt / workspace clippy | PASS / PASS |
| touched tests | broker 40; winbroker 19; winsvc 120 pass + 1 CUDA ignore; wsl2d suites PASS |
| native MSVC release | PASS; broker and winsvc staged |
| static PowerShell harness | PASS: 13 assertions |
| final broker VM matrices | Peer / RetryBudget / Boundary PASS; Event Log PASS |
| `lease.rs` cover | 98.7% (149/151) |
| winbroker `lib.rs` cover | 97.3% (286/294) |
| winsvc config / IPC / package / runtime / evidence | 96.7% / 83.3% / 89.0% / 88.9% / 97.2% |
| package transaction | 5/5 PASS for VM and physical package shapes |
| VM healthy lifecycle | 3/3 PASS; 9/9 SHA; zero residue |
| VM BrokerLossOnline | PASS; stop 4,838 ms; no reconnect; zero residue |
| physical cold boots | 3/3 PASS; 9/9 SHA; zero residue; zero forced kills |

Coverage reports:

- `tmp/windows-autonomous-broker-service-broker-cov.json`
- `tmp/windows-autonomous-broker-service-winbroker-cov.json`
- `tmp/windows-autonomous-broker-service-winsvc-cov.json`

## VM before → action → after

Before: `win11-drill`, Windows 26200.8037, Test Mode, both demand services
stopped, zero RamShared disks.

Action:

- package `R:` through the complete transaction matrix;
- three independent cold-boot lifecycle runs;
- one `BrokerLossOnline` run;
- three random 8 MiB write/read/SHA rounds per run;
- broker peer, retry, oversized, partial-frame, deny-only SID and blocked-read
  cancellation drills.

After:

- broker/winsvc/driver BINARY_MATCH in every lifecycle run;
- healthy readiness 9,908/19,784/11,556 ms: median 11,556 ms, nearest-rank
  p99 19,784 ms;
- healthy product stop 4,949/5,060/4,417 ms: median 4,949 ms, p99 5,060 ms;
- `BrokerLossOnline` safe consumer stop 4,838 ms;
- services stopped, lease released where confirmable, zero disks and no
  automatic consumer reconnect.

Evidence: `evidence/vm-final/` and `evidence/package-final/`.

The broker matrices were rerun after the final Event Log change. The loaded
native broker matched SHA-256
`EE7C102F620B5F21947321EE93F16E9C6D174A406E7426165EA64B9A0D746911`.
Peer, RetryBudget and Boundary all observed Event ID 1000 from
`RamSharedBroker` with `transition=process_ready`. Readiness was 506/476/671
ms, blocked accept/read cancellation was 254–266 ms, and the partial-frame
deadline fired at 10,021 ms. The complete evidence map is
`evidence/README.md`; the post-change result is
`evidence/vm-final/broker-final-matrices.json`.

## Physical before → action → after

Before: Windows 11 build 26200, Test Mode, loaded driver hash matched the
package, services stopped, zero RamShared disks, pagefile only on `C:`, `S:`
free. The real 466 GiB `R:` data volume remained mounted and untouched.

Action: `Run-HostAutonomousLifecycle.ps1` as a SYSTEM startup task, one
immutable `S:` manifest across three cold boots, watchdog armed per boot,
three random 8 MiB SHA rounds, supported consumer-first stop and zero-residue
proof before each reboot.

After:

- manifest SHA `0F6DFDB3327EEDAF1143C5742B4E0CD3A00F16FDD8FF4FF3799230902AAC1F1A`
  on boots 1–3;
- readiness 1,164/1,165/1,156 ms: median 1,164 ms, p99 1,165 ms;
- consumer stop 2,796/2,557/2,552 ms: median 2,557 ms, p99 2,796 ms;
- product stop 3,049/2,810/2,805 ms: median 2,810 ms, p99 3,049 ms;
- 9/9 SHA matches, residue 0, forced kills 0;
- final services stopped, disk count 0, `S:` absent, task and watchdog absent.

Evidence: `evidence/physical-final/`.

## Failures found and corrected

- WinDrive PSI heartbeat was treated as an unexpected message and closed the
  authoritative lease session; a regression test now keeps it open.
- the Windows build consumed a stale mirror; the release was rebuilt from the
  synchronized source and its hash changed as expected;
- format orchestration lacked a bound; VM/physical init-format now has a
  60-second deadline;
- the first physical manifest used `R:`, already a 466 GiB data volume. The
  attempt refused before formatting. SPEC DT-17 now requires the immutable
  config letter and a pre-install collision refusal; the final package uses
  free `S:`;
- immutable version identity correctly rejected changing `R:` to `S:` under
  the same version/commit; physical package identities are now distinct;
- physical `Stop-Service` raised despite a completed safe stop. The harness
  now records that request error but still requires `Stopped` within 30
  seconds and zero residue.

Rejected attempts are retained as evidence, not counted as PASS:
`evidence/physical-failed-r-collision/` and
`evidence/physical-failed-stop-cmdlet/`.

## Verdict

✅ PASS — implemented, canonical cover passed, VM and physical live E2E
passed, BINARY_MATCH passed, and no environment-bound gap remains for this
surface.
