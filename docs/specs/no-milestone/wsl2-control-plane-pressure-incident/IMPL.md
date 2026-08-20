# IMPL — WSL2 control-plane pressure incident hardening

Status: **source implementation; live qualification pending**.

## Implemented

- Host-budget refusal now prevents CUDA allocation.
- Product NBD starts with guaranteed preallocation and deterministic 4→2→1 GiB
  admission; sparse mode requires an explicit experimental environment flag.
- Status schema v3 distinguishes topology from `READY`, `AT_RISK`, and
  `BLOCKED`, including activation identity, guaranteed capacity, and disk
  growth above the activation baseline.
- The read-only Ratatui monitor provides RAM/PSI, physical GPU, swap-tier, and
  control-plane panels. Its typed JSONL path owns bounded GPU collection,
  rotation, and atomic heartbeat publication; the shell sampler is now a thin
  compatibility wrapper.
- The Windows status viewer reads only the host heartbeat, and the btop helper
  is plan-first with an exact backup/rollback path.
- The safe workload launcher calculates a control-plane reserve and invokes a
  dedicated systemd slice.
- The sealed installer atomically deploys the health sampler unit and workload
  slice without enabling them; a conflict rolls back the new release and
  preserves the existing systemd definition.
- The Windows dead-man reads the external heartbeat and signals only the
  managed slice with bounded WSL client calls.
- Persistent cascade and checkout-based health autostarts on the daily host
  were disabled after a successful zero-use, swapoff-first deactivation.
- The obsolete persistent lifecycle approval was moved out of systemd's
  active drop-in path without deleting it.
- The sealed uninstaller proves unit identity before removal and leaves foreign
  definitions and managed workloads untouched.
- The Windows incident snapshot records bounded post-incident host state,
  sanitized `.wslconfig` kernel/swap state, and a SHA-256 inventory. It
  delegates the official WSL trace collection to a manual, reviewed, elevated
  operator action.

## Source verification

- Rust tests: all selected package unit and integration tests passed; unsafe
  GPU/ublk live tests remained gated and ignored.
- Clippy: selected packages and all targets passed with warnings denied.
- SSDV3 line coverage: sparse VRAM 92.6%, CLI changed-file slice 82.6% or
  higher after the workload file reached 90.3%, and daemon main 81.7%.
- Sealed NBD packaging: 37/37 shell gates passed, including auxiliary-unit
  conflict, rollback, and foreign-unit preservation coverage.
- Health delegation, schema v3 attribution, monitor parser, rotation, atomic
  heartbeat, and safe `/bin/true` managed-scope tests passed.
- Full Windows static CI, including the bounded watchdog, passed.
- The incident snapshot PowerShell parser and static contract passed. One
  explicitly authorized local post-incident capture completed without WSL
  lifecycle changes or workload pressure; its machine-local artifact is not
  part of this repository.
- Documentation governance, lifecycle, links, evidence, and public hygiene
  passed.

## Open evidence

- Disposable WSL version/kernel A/B with official Microsoft traces.
- Approved GPU-backed 512/1024 zram transition matrix.
- Reviewed upstream comment; nothing has been posted automatically.

No live pressure or new readiness claim is produced by this implementation
record.
