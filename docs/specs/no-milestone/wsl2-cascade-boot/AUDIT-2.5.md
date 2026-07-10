# AUDIT-2.5 — wsl2-cascade-boot

## Findings

| Sev | Finding | Resolution in SPEC |
| --- | --- | --- |
| HIGH | Old `ramsharedd.service` is ublk lab path — must not be product boot | New unit uses `ramshared up` NBD only |
| HIGH | Kill/stop order wrong → WSL freeze | ExecStop = `down` only; TimeoutStopSec=600 |
| MED | systemd `${VAR}` in ExecStart unreliable | Wrapper `cascade-up.sh` sources conf |
| MED | Auto-enable by default surprises users | install defaults to files only; `--enable` opt-in |
| LOW | Cannot promise zero lag under WDDM | Docs honest; RF-6 |

## Open questions

None blocking. WSL must have systemd (documented).

## Verdict

**go** — implement SPEC as written.
