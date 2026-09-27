# AUDIT-2.5 — wsl2-origin-capacity-policy

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Medium | DT-2 | Sub-GiB sizes or unaligned bytes could break fixed-block extent mapping. | Require strict modulo verification `fixed_size_bytes % (1024^3) == 0`. |
| High | DT-2 | Physical container could be sized smaller than partition 1 (MSR) + partition 2 (swap). | Enforce mathematical floor `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`. |
| Low | DT-3 | Approval token could be ambiguous across different physical sizes. | Incorporate container GiB into the token string: `RAMSHARED_ORIGIN_${N}GIB_PARTUUID`. |
| Low | DT-5 | Static tests might fail if `25GB` string was removed from PowerShell script. | Retain `25GB` in parameter `ValidateRange` and fallback defaults. |
| High | DT-6 | Default placement can choose a full distro volume and start a fixed VHDX allocation without preserving host headroom. | Prefer the distro volume only at `OriginSizeBytes + 10 GiB`; check C: next; otherwise refuse before writes and recheck after staging allocation. |
| High | DT-7 | Recomputing the path from the current distro `BasePath` can make a sealed origin at another path inaccessible or direct later operations at a new path. | Reuse the absolute origin path in the sealed manifest; reject explicit path mismatch; never relocate existing data automatically. |
| Medium | DT-8 | `Get-Volume` may return zero/multiple volume records or a non-local destination. | Require one unambiguous local volume record for automatic selection; fail closed without choosing an arbitrary drive. |

## Open questions

- Manufactured selector, reserve-boundary, replay, override-refusal, and rollback-order tests pass. An elevated disposable-host drill also created a 5 GiB fixed VHDX, verified its sealed GPT/PARTUUID identity, and removed it on both C: and the registered distro volume I:.
- A read-only 64 GiB request correctly rejected I: for insufficient reserve and selected C:. Explicit-path planning passed. Cleanup proved the temporary targets/manifests absent and the production manifest and `.wslconfig` unchanged.
- EVD-0067 attaches the disposable VHDX to WSL, passes the live identity-bound host gate, refuses a stale guardian proof, and provisions the 4 GiB swap signature idempotently. The test swap was never activated and the VHDX was detached and uninstalled. The literal one-volume C: host topology was not available; its selection boundary is covered by a manufactured test.

## Verdict

**GO for source review and the isolated Windows storage lifecycle.** ITEM-6 has
named executable proof for distro-volume preference, C: fallback, the
single-volume C: boundary, low-space and unsupported-storage refusal,
sealed-path replay, conflicting override refusal, and the post-allocation
reserve gate. The live drill confirmed fixed VHDX allocation, identity proof,
reserve preservation, and rollback by exact-path uninstall on both C: and I:.
`Test-RamSharedOriginStatic.ps1` also confirms the unique local-volume and
filesystem gates, that manufactured tests bypass live host discovery, and that
reserve checks precede proof, promotion, and manifest publication.

Step 3 remains **PARTIAL** until a guarded cascade activation, bounded stress,
and swapoff-first teardown pass on the disposable guest origin. This audit does
not claim a literal one-volume physical host or CoCo qualification.
