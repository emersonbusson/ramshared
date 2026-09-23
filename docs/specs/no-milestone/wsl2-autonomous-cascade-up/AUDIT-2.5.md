# AUDIT-2.5 — wsl2-autonomous-cascade-up

## Findings

| Sev | SPEC § | Issue | Required Fix |
| :--- | :--- | :--- | :--- |
| **Low** | DT-4 | If `systemd-run` fails to set `INVOCATION_ID` in an unusual environment, a child process could loop infinitely re-executing itself. | Add an explicit recursion guard environment variable (`_RAMSHARED_SCOPED=1`) so that re-exec is attempted at most once before failing closed. |
| **Low** | DT-1 | If WSL interop is disabled in `/etc/wsl.conf` (`[interop] enabled=false`), `cmd.exe` cannot be executed. | Detect interop availability; if disabled, fail-closed with explicit error directing the operator to attach the disk or re-enable interop. |

## Open Questions

None. The mechanism was empirically proven live in this session (`timeout 10 cmd.exe /c "wsl.exe --mount --vhd C:\ProgramData\RamShared\ramshared-origin.vhdx --bare"` mounted the origin disk in <100ms, and `sudo systemd-run --scope ./target/release/ramshared up` armed the 3-tier cascade cleanly).

## Verdict

**`go`**
