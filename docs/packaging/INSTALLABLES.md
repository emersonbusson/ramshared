# RamShared Installables

RamShared has two supported packaging paths today:

1. **Linux/WSL2 bundle** for the product path (`ramshared`, `ramsharedd`, agents, systemd templates, safety scripts).
2. **Windows lab scripts** for driver and service drills. Windows driver output remains lab-only and is not bundled into the Linux product archive.

## Build Linux/WSL2 Bundle

```bash
scripts/package/build-linux-bundle.sh
```

Outputs:

- `artifacts/packages/ramshared-linux-<version>/`
- `artifacts/packages/ramshared-linux-<version>.tar.gz`
- `SHA256SUMS` inside the staged directory

The bundle excludes local VM notes, credentials, build caches, driver artifacts,
and anything ignored as local-only.

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

## Generic GPU Workload Gate

From Windows PowerShell:

```powershell
.\scripts\p0\Invoke-GpuWorkloadGate.ps1 -AttachOnly -WorkloadLabel external-gpu-workload
```

The gate is application-agnostic. It measures aggregate idle/load/recovery VRAM
pressure and does not claim process attribution.
