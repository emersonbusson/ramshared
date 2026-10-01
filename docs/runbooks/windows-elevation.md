# Windows Elevation from the WSL2 Shell

How to run a Windows command **as Administrator** from inside WSL2, and which
RamShared actions actually require it. Written so a future operator never has to
guess whether a permission prompt will appear.

---

## 1. The rule

| Question | Answer |
| --- | --- |
| Does Linux `sudo` elevate a Windows process? | **No.** `sudo powershell.exe ...` still runs unelevated. |
| Does `powershell.exe` from WSL2 start elevated? | **No.** It inherits a standard user token. |
| What elevates a Windows process from WSL2? | `Start-Process -Verb RunAs`. |
| Is UAC prompt shown on this host? | **No.** UAC is disabled, so `Verb RunAs` elevates silently. |
| Does `-ExecutionPolicy Bypass` change Windows policy? | **No.** It is per-process only. |
| Which RamShared actions need elevation? | `wsl.exe --mount --vhd`, origin attach, diskpart, service install/uninstall. |

An unelevated `wsl.exe --mount --vhd` fails with:

```
Wsl/Service/AttachDisk/MountDisk/HCS/E_ACCESSDENIED
(exit code 255)
```

That is a Windows token problem, not a WSL or RamShared bug. Re-run elevated.

---

## 2. The elevation recipe

One shell line from anywhere in WSL2. Substitute the inner script:

```bash
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command \
  "Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList \
  '-NoProfile','-ExecutionPolicy','Bypass','-Command','<SCRIPT>'"
```

Parameter meanings:

| Parameter | Why it is there |
| --- | --- |
| `-NoProfile` (outer and inner) | skips `$PROFILE`, so a machine profile cannot change the command |
| `-Verb RunAs` | requests the Administrator token — this is the elevation |
| `-Wait` | returns only after the elevated process exits, so the exit is observable |
| `-ExecutionPolicy Bypass` | lets a `.ps1` run in **this process only**; Windows policy is unchanged |
| `-Command '...'` | the elevated body |

**On this host UAC is disabled**, so `-Verb RunAs` elevates with no dialog.
On a host with UAC enabled, a consent prompt appears and someone must click Yes;
`-Wait` then blocks until they do. Never assume silent elevation on other hosts.

### Working example

```bash
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command \
  "Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList \
  '-NoProfile','-ExecutionPolicy','Bypass','-Command',
  'Get-Service -Name RamSharedBroker, RamSharedWinSvc | Out-File -Encoding utf8 C:\ProgramData\RamShared\elevated-check.txt'"
```

Write output to a **fresh** Windows path (`C:\ProgramData\RamShared\...`).
Do not redirect into a read-only or pre-existing log: a failed `>` into a
read-only file produces an empty file and looks like a silent failure.

---

## 3. PowerShell script execution

A `.ps1` on this host is refused with *"a execução de scripts foi desabilitada"*
unless the process opts out:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\ProgramData\RamShared\Manage-RamSharedOrigin.ps1 ...
```

`-ExecutionPolicy Bypass` applies to that process only. Nothing is written to
registry policy (`HKLM:\Software\Policies\...`), and the next process sees the
unchanged machine policy.

---

## 4. Origin attach (the action that needs it)

`wsl.exe --mount --vhd` needs the Administrator token. The governed path is
`C:\ProgramData\RamShared\Manage-RamSharedOrigin.ps1`.

### 4.1 The approval token is size-derived

The token is **not a size you choose**. It is generated from whatever the
origin VHDX size actually is:

```powershell
$ApprovalToken = if ($OriginSize -eq 25GB) {
  "RAMSHARED_ORIGIN_25GIB_PARTUUID"          # only for a 25 GiB VHDX
} else {
  "RAMSHARED_ORIGIN_${OriginSizeGiB}GIB_PARTUUID"
}
```

**Always read the token from the script's own `PLAN` output. Never hardcode one.**

> **Worked example (this host, 2026-09-30).** The origin VHDX is **5 GiB**, so the
> generated token is `RAMSHARED_ORIGIN_5GIB_PARTUUID`. Passing the 25 GiB spelling
> here is refused with `origin action requires exact approval token`.
>
> This is an **example of one host**, not a product default. Another host with a
> 25 GiB VHDX gets `RAMSHARED_ORIGIN_25GIB_PARTUUID`; a 12 GiB VHDX gets
> `RAMSHARED_ORIGIN_12GIB_PARTUUID`. Substitute your own `PLAN` output.

**Sizes are separate things** — do not add them together or treat the token as
the VRAM size. On this host:

| Thing | Value | Meaning |
| --- | --- | --- |
| Token size component | `5GIB` | physical size of the origin VHDX file on Windows |
| `logical_capacity_mib` | 4096 | sealed logical swap area inside that VHDX |
| `VRAM_MIB` / `physical_cache_cap_mib` | 4096 | GPU VRAM used as the cache tier |
| `ZRAM_MIB` | 2048 | zram tier |

So a 5 GiB file on Windows carries a 4 GiB logical swap area, cached in 4 GiB of
GPU VRAM. The VRAM is the cache tier; the SSD origin is the authoritative store.

### 4.2 Attach

```bash
# EXAMPLE for a 5 GiB VHDX — substitute the token from your own PLAN output.
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command \
  "Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList \
  '-NoProfile','-ExecutionPolicy','Bypass','-Command',
  'C:\ProgramData\RamShared\Manage-RamSharedOrigin.ps1 -AttendedOriginApply -ApproveOriginProvision RAMSHARED_ORIGIN_5GIB_PARTUUID *>&1 | Out-File -Encoding utf8 C:\ProgramData\RamShared\attach-now.log'"
```

Two explicit gates must both be present:

- `-AttendedOriginApply` — the action is a separate attended step
- `-ApproveOriginProvision <token>` — the exact size-derived token from your `PLAN`

Omitting either throws before any mount. The token is case-sensitive and must
match the VHDX size exactly.

### 4.3 Verify

From WSL2:

```bash
# The sealed PARTUUID must appear as a partition
readlink -f /dev/disk/by-partuuid/5039dca1-61a0-41ff-aa08-221f49326a1b
# -> /dev/sdX2   (dev_t must match origin.conf partition_dev_t)

# The sealed swap signature must be present
blkid -s UUID -s TYPE /dev/sdX2
# -> UUID=<expected_swap_uuid> TYPE=swap
```

`/etc/ramshared/origin.conf` is the authority for `partition_dev_t`,
`parent_dev_t`, `partuuid`, and `expected_swap_uuid`. A mismatch is a failed
attach, not something to paper over.

---

## 5. Swap-signature provisioning (separate, also elevated on the device)

Normal `ramshared up` **never** runs `mkswap`. The NBD origin must already carry
the sealed swap header. Provision once with
`scripts/safety/provision-origin-swap.sh`:

```bash
# Plan (no mutation)
sudo scripts/safety/provision-origin-swap.sh

# Execute — approval string is exact and derived from the sealed UUID + device
sudo env RAMSHARED_ORIGIN_PROVISION_APPROVAL="provision:<expected_swap_uuid>:<origin_path>" \
  scripts/safety/provision-origin-swap.sh --execute
```

The approval value is `provision:<expected_swap_uuid>:<origin_path>` exactly as
`origin.conf` declares them. A wrong string is refused with
`RAMSHARED_ORIGIN_PROVISION_REASON=EXACT_APPROVAL_REQUIRED`. The script prints
`RAMSHARED_ORIGIN_PROVISION=PROVISIONED`, or `ALREADY_PROVISIONED` when the
sealed header is already correct.

### 5.1 Pitfall: header size must match the NBD export, not the partition

`mkswap` writes `last_page` for the size it is given. `swapon` then compares it
to the **device** size and fails with `Invalid argument` when the header claims
more space than the device offers.

On this host (2026-09-30) `mkswap` had been run on the whole 5103 MiB
partition, so the header said `last_page=1306367` (5103 MiB) while the NBD
export presents `logical_capacity_mib=4096` (4096 MiB, `maxpages=1048575`).
`blkid` still reported `TYPE=swap` with the sealed UUID, so
`verify_preprovisioned_swap` passed and only `swapon` refused.

Re-provisioning with the sealed logical size fixes it:

```
last_page=1048575 → 4096 MiB   expected last_page = 4096*256-1 = 1048575   MATCH
```

Check quickly:

```bash
sudo python3 -c "
import struct
with open('/dev/sdX2','rb') as f:
    f.seek(1028); last = struct.unpack('<I', f.read(4))[0]
print(f'last_page={last} -> {(last+1)*4096/1024/1024:.0f} MiB')
"
```

`last_page` must equal `logical_capacity_mib * 256 - 1`. A mismatch means
re-run `provision-origin-swap.sh --execute`; never paper over it.

---

## 6. Troubleshooting

| Symptom | Cause | Action |
| --- | --- | --- |
| `E_ACCESSDENIED` / exit 255 on `wsl.exe --mount` | process not elevated | use the `Start-Process -Verb RunAs` recipe |
| elevated launch produces an empty log | redirected into a read-only or stale file | write to a fresh path under `C:\ProgramData\RamShared\` |
| `origin action requires exact approval token` | token does not match VHDX size | take the token from the `PLAN` output |
| `execução de scripts foi desabilitada` | `.ps1` run without a per-process policy | add `-ExecutionPolicy Bypass` |
| `swapon ... Invalid argument` on `/dev/nbd0` | NBD export lacks the sealed swap header | run the provisioner (section 5), then `ramshared up` |
| `sudo powershell.exe ...` still denied | Linux `sudo` does not elevate Windows | use the `Start-Process` recipe |

---

## 7. Boundary

This runbook is a **permission recipe**, not authority to mutate a host. The
origin attach and swap provisioner still require their exact approval tokens and
their identity revalidation gates. Elevation makes those gates reachable; it does
not replace them.
