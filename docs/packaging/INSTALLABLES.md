# RamShared Installables

RamShared has two supported packaging paths today:

1. **Linux/WSL2 bundle** for the product path (`ramshared`, `ramsharedd`, agents, systemd templates, safety scripts).
2. **Windows driver packaging** for StorPort miniport driver and service integration. Windows driver output is packaged independently under its own distribution pipeline and is not bundled into the Linux product archive.

## Build Linux/WSL2 Bundle

```bash
scripts/package/build-linux-bundle.sh
```

Outputs:

- `artifacts/packages/ramshared-linux-<version>/`
- `artifacts/packages/ramshared-linux-<version>.tar.gz`
- `SHA256SUMS` inside the staged directory

The bundle excludes credentials, build caches, intermediate driver build artifacts,
and local-only development files.

## Smoke the Bundle

```bash
tar -tzf artifacts/packages/ramshared-linux-<version>.tar.gz | head
tar -xzf artifacts/packages/ramshared-linux-<version>.tar.gz -C /tmp
/tmp/ramshared-linux-<version>/bin/ramshared check
```

`check` may report blocked on hosts without WSL2 GPU or required kernel modules;
that is an environment result, not a packaging failure.

## Boot Install

Use the existing opt-in safety installer from an unpacked tree:

```bash
sudo RAMSHARED_BIN_DIR="$PWD/bin" bash scripts/safety/install-cascade-boot.sh
```

Enable only after `ramshared check` and `cascade-preflight.sh` pass. Stop/removal
must continue through `ramshared down` / `uninstall-cascade-boot.sh` so swapoff
precedes daemon shutdown.

## Direct Linux/WSL2 Install

`sudo bash scripts/install.sh` installs the CLI under `/usr/local/bin`. Each
successful install or update records the host's UTC installation time at
`/usr/local/share/ramshared/INSTALL_TIMESTAMP` and writes
`INSTALL_METADATA.json` schema v2 with the source version, full commit SHA,
tree state, timestamp, and SHA-256 digests for both installed binaries. The
metadata is published after both binaries are copied. `ramshared top` shows the
version, short commit SHA, and install time; legacy or mismatched metadata is
reported as unknown. The sealed bundle path verifies the installed manifest
receipt and the binary and identity-file hashes, then reads the timestamp from
`INSTALL_PROVENANCE.json`.

While `ramshared top` is open from a recognized installed path, an update is
detected by the running and installed executable hashes. The process restores
the terminal and replaces itself with the updated CLI while preserving its
arguments. If replacement fails, the existing dashboard stays open and shows
the failure. A `ramshared top` launched from a checkout does not switch to an
installed binary.

## Generic GPU Workload Gate

From Windows PowerShell:

```powershell
.\scripts\p0\Invoke-GpuWorkloadGate.ps1 -AttachOnly -WorkloadLabel external-gpu-workload
```

The gate is application-agnostic. It measures aggregate idle/load/recovery VRAM
pressure and does not claim process attribution.
