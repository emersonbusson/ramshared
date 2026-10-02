# WSL2 stress freeze and consolidation branch audit — 2026-09-23

## Scope and disposition

Audited all 12 commits on `feat/ramshared-20260921-consolidation` after its
remote tip `fccbb5b9`. This is a source and retained-host-evidence audit. The
candidate has **no physical three-tier qualification**. No new cascade, stress,
host installation, or WSL shutdown was performed during this audit.

## Retained incident timeline (America/Sao_Paulo)

| Time | Evidence | Interpretation |
| --- | --- | --- |
| 18:01:03 | Kernel reported four NBD reads stuck for 30 seconds. Repeated reports reached more than 2,000 seconds. | The NBD path was already unhealthy before the final pressure ramp. |
| 18:37–18:40 | NBD disconnects and read I/O errors; three `MCE: Killing` records for unrelated processes. | Data-path and host-memory fault signatures require separate investigation. Their initiating cause is unproved. |
| 19:00–19:30 | Four more `MCE: Killing` records. | A zero-panic label cannot qualify this boot. |
| 19:36:07–19:37:45 | Four `ramshared stress --cascade --tier3-target-pct 99` sudo sessions opened and closed. | These recorded CLI invocations ended before the terminal pressure rise. Their output was not retained as a qualified envelope. |
| 19:44:19–19:44:36 | Durable health JSONL showed Python RSS rise from 5,246,312 to 8,442,308 KiB, `MemAvailable` fall from 3,027,384 to 614,700 KiB, and PSI full avg10 rise from 8.16% to 17.91%. ZRAM reached 1,048,572 KiB, NBD swap 477,956 KiB, and fallback SSD swap 640,492 KiB. Cache, supervisor, and guardian evidence were stale. | The final pressure source was `python3`; its exact command and relation to the earlier CLI runs are not established. NBD logical occupancy does not establish physical GPU residency. |
| 19:44:39 onward | Journald logged memory pressure; the long boot stopped producing records. WSL restarted at 19:53:39 and underwent several short boots. | Guest unresponsiveness/restart is observed. A kernel BUG/Oops/panic at the terminal instant is not proved. |

The retained September 23 benchmark JSON files were written hours before the
terminal pressure event. They cannot be used as its verdict. The post-restart
swap table contained only the WSL fallback swap device, zero used, with managed units inactive.

## Commit-by-commit review

| Commit | Scope | Audit result |
| --- | --- | --- |
| `dc855e43` | Isolated GPU cache worker | **Critical:** a partial write allocated a full chunk and an unwritten range could be served as a cache hit. **High:** reported cached bytes were inferred from submitted payload bytes rather than confirmed worker allocation. A live free-VRAM buffer was not checked at each allocation. Local regression tests reproduced these failures and source corrections are pending host qualification. |
| `d98e363e` | Worker framing and handshake | The larger handshake timeout and single-frame send are bounded, but a stream write can still be partial; the client correctly fails closed on a partial mutation. No live NBD stall proof exists for this build. |
| `8acba838` | Worker SPEC audit | The SPEC still describes a `DxgProvider` and dedicated control lane while executable worker selection uses CUDA then Vulkan over one stream. The live contract needs reconciliation before DONE. |
| `8dd27ed5` | Shared IPC and vsock | Protocol is hermetically tested. Linux connect uses a blocking connect with a best-effort socket timeout; Windows AF_HYPERV listen and accept remain unimplemented. No production host-guest path is qualified. |
| `a0615d5a` | Host gate and Windows control plane | Host gate is not wired into product activation. Windows VHDX command timeout was a no-op, and successful detach did not clear recorded attachment state. The command timeout and state-clear defects have local fixes; the control plane remains partial. |
| `491a756f` | Control-plane SPEC | IMPL marked all items implemented despite the missing Windows listener, activation wiring, and live E2E. Status must remain PARTIAL. |
| `cb50dd75` | Coverage and stress documentation | Reports hermetic coverage and low-pressure runs; these are not physical three-tier qualification. |
| `c574cc93` | Comment language and generated capability records | Documentation-only correction; no hang-class runtime delta found. |
| `469467a8` | Disable timeout and BOM parsing | Teardown timeout is separated from data-path timeout. No live 5-second drain/reap proof was recorded. |
| `fc12fd5f` | Stress verdict and CLI activation | **High:** `dmesg` failure counted as zero faults; NBD stuck requests, NBD read errors, and MCE were not part of the PASS gate. Physical cache was required only at entry. Local corrections fail closed on missing evidence and require one simultaneous full-tier snapshot. |
| `543158d6` | Cache status freshness | **High:** background thread changed only the JSON timestamp, making stale physical values look fresh; CLI accepted 300-second samples. Local correction removes the timestamp rewrite and limits sample age to 15 seconds. |
| `c02e5d8d` | Monitor priority labels | Display-only correction aligns ZRAM/NBD priorities with the active topology; no new hang path found. |

## Remaining host gates

1. Build and install one exact candidate under the configured global build
   admission contract. Record binary SHA-256 and runtime `BINARY_MATCH`.
2. Confirm a fresh host guardian, supervisor, origin identity, and independent
   Windows watchdog. The daily WSL host must not use an environment variable
   alone as proof that the watchdog is armed.
3. Isolate the earlier NBD stuck-read and MCE signatures before destructive
   pressure. Capture guest kernel log and Windows host events from the same
   bounded run, with a rollback path and swapoff-first cleanup.
4. Qualify three matched runs using simultaneous ZRAM occupancy, NBD logical
   occupancy, worker-reported physical VRAM allocation corroborated by an
   independent GPU free-memory delta, actual SSD swap usage, integrity reads,
   pressure, faults, and exact workload/binary identity.
   Abort on stale cache or blocked guardian/supervisor state.

Source-level tests and coverage do not close these host gates.

## Local correction and verification

The working tree now rejects unwritten GPU cache ranges, reports allocation
from a bounded worker heartbeat, removes synthetic status freshness, and
prevents a safety stop from entering the active page cycler. Kernel fault
evidence is checked before and during a cascade; the stress watchdog now sets
the termination signal after releasing buffers. Stress verdicts require
available kernel evidence, no fault signatures, and one simultaneous
physical three-tier sample. Windows VHDX commands now have a real child-process
deadline, and successful detach clears the local attachment record.

The block library passed 107 tests. The CLI passed 320 unit and 10 integration
tests. Clippy passed for block, CLI, WSL daemon, and Windows service targets.
The documentation check passes all project-owned checks but fails lifecycle
classification on a pre-existing untracked `.mimocode/plans/` document.

At 20:17 local time, the read-only WSL2 campaign gate reported `daily_host=true`,
`shared_windows_desktop=true`, `windows_watchdog=false`,
`guardian_state=BLOCKED`, and `gates_ok=false`. Managed ZRAM and NBD were absent;
only the fallback swap device was present with zero used. No destructive
stress run or physical qualification was attempted in the source-correction
phase because the independent host-safety gates were blocked.
