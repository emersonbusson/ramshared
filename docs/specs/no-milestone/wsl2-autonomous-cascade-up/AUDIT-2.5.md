# AUDIT-2.5 — wsl2-autonomous-cascade-up

## Findings

| Sev | SPEC § | Issue | Required Fix |
| :--- | :--- | :--- | :--- |
| **Low** | DT-4 | If `systemd-run` fails to set `INVOCATION_ID` in an unusual environment, a child process could loop infinitely re-executing itself. | Add an explicit recursion guard environment variable (`_RAMSHARED_SCOPED=1`) so that re-exec is attempted at most once before failing closed. |
| **Low** | DT-1 | If WSL interop is disabled in `/etc/wsl.conf` (`[interop] enabled=false`), `wsl.exe` cannot be executed. | Detect interop availability; if disabled, fail-closed with explicit error directing the operator to attach the disk or re-enable interop. |

## Open Questions

The previous live command exercised an older `cmd.exe` path and does not qualify the corrected direct `wsl.exe` invocation, sealed manifest verification, or current recovery state. Re-run on a clean controlled host with exact binary identity.

## Verdict

**`partial`** — source checks pass; live corrected-path evidence remains open.
