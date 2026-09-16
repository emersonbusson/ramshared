# AUDIT-2.5 — wsl2-origin-capacity-policy

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| Medium | DT-2 | Sub-GiB sizes or unaligned bytes could break fixed-block extent mapping. | Require strict modulo verification `fixed_size_bytes % (1024^3) == 0`. |
| High | DT-2 | Physical container could be sized smaller than partition 1 (MSR) + partition 2 (swap). | Enforce mathematical floor `fixed_size_bytes >= (logical_capacity_mib + 1024) * 1024^2`. |
| Low | DT-3 | Approval token could be ambiguous across different physical sizes. | Incorporate container GiB into the token string: `RAMSHARED_ORIGIN_${N}GIB_PARTUUID`. |
| Low | DT-5 | Static tests might fail if `25GB` string was removed from PowerShell script. | Retain `25GB` in parameter `ValidateRange` and fallback defaults. |

## Open questions

- None. All boundary checks are algebraic and verified deterministically against the manifest configuration hash.

## Verdict

**go**. Conditional on mathematical headroom verification and named unit tests covering 5 GiB acceptance and under-capacity rejection.
