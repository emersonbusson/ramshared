#Requires -Version 5.1
<#
.SYNOPSIS
  ITEM-9 / DT-13 — pagefile-VRAM vs disk latency (capacity-first gate).

.DESCRIPTION
  Side-by-side in the SAME window, >=3 runs, median+p99+deviation, tags idle|loaded.
  Dual output: docs/benchmarks/results.jsonl + docs/BENCHMARKS.md (append-only).
  Gate: pagefile-VRAM usage > 0 under pressure AND p99 <= Kx disk (K from first real run).
#>
[CmdletBinding()]
param(
    [ValidateSet('idle', 'loaded')]
    [string]$Condition = 'idle',
    [int]$Runs = 3
)

Write-Host @"
Measure-PagefileVram.ps1 — STUB (ITEM-9 not implemented yet)

Planned:
  - Condition tag: $Condition
  - Runs: $Runs
  - Capture context (build, GPU, RAM, open apps) automatically
  - Compare page-in on V: pagefile vs C: pagefile in the same load snapshot
  - Append machine line to docs/benchmarks/results.jsonl
  - Append human block to docs/BENCHMARKS.md

Exit 2 until implemented.
"@
exit 2
