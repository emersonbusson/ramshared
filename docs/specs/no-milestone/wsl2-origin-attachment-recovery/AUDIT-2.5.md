# AUDIT-2.5 — Attended recovery of the sealed WSL2 origin attachment

## Findings

| Severity | SPEC section | Issue | Required fix |
| --- | --- | --- | --- |
| Critical | DT-2 | A generic disk number or path would allow attachment of the fallback swap or a foreign VHDX. | Derive all identity from the sealed manifest and match a temporary VHDX ownership proof. |
| High | DT-3 | Repeated attach after success could make recovery non-idempotent. | Probe the exact PARTUUID before mutation and return `ALREADY_ATTACHED`. |
| Critical | DT-4 | A mount timeout may have taken effect; an automatic broad unmount could detach a foreign disk. | One mount only, bounded read-only postcondition probes, then fail closed with no automatic cleanup. |
| High | DT-5 | Attachment could be confused with permission to format or activate swap. | Prohibit guest block writes, swap/NBD actions, authority publication, WSL shutdown, and broad unmount. |

## Open questions

- The live WSL device name is intentionally not a contract; the exact PARTUUID
  is the only accepted guest identity.
- The live proof must confirm that the installed WSL version honors bare VHD
  attachment without mounting a filesystem.

## Verdict

**go** for the narrowly scoped attended attachment action. **No-go** for any
automatic startup path, mount retry, disk-number selection, broad unmount,
format, or claim that attachment alone makes the cascade ready.
