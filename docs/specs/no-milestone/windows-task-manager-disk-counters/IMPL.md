# IMPL - Windows virtual disk counter audit

## Status

**PARTIAL.** The audit harness, static gates, and one physical-host live audit are
green. The remaining open claim is Task Manager UI parity, which is deliberately
not closed by CIM/direct-I/O evidence.

## Implemented

- `scripts/windows/Invoke-WindowsDiskCounterAudit.ps1`
- `scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1`

## Validation

- Static: `pwsh.exe -NoProfile -ExecutionPolicy Bypass -File scripts/windows/Test-WindowsDiskCounterAuditStatic.ps1`
- Live: 2026-07-18 `C:\ramshared\artifacts\disk-counter-audit-20260718-005325`
  passed with delegated artifact `C:\ramshared\artifacts\exhaustive-20260718-005327`.
  Summary: `PASS=true`, `DISK_IO_MEASURE_OK=true`, `DIRECT_LOAD_MATCH=true`,
  `DIRECT_PROBE_MATCH=true`, `PERFDISK_MATCH=true`, `NONZERO_ACTIVITY=true`,
  `LUN_GONE=true`, `WIN32_GONE=true`, and `PNP_GONE=true`.
- Delegated disk evidence: direct sampling load wrote 400 MiB and read 400 MiB
  with `match=True`; PerfDisk matched `5 S:` and reported non-zero busy/write/queue
  counters; direct 8 MiB checksum probe matched.
- Docs: `./scripts/docs-check.sh`
