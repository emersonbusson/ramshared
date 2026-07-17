# WDK build audit — 2026-07-16

## Deterministic RED

The canonical build script initially used `/Zi` from a WSL UNC repository path. `cl.exe` selected
`C:\Windows\vc140.pdb` and failed once with `C1041`. This was classified as deterministic; the same
command was not blindly retried.

## Right-layer correction

`Build-Drivers.ps1` now uses `/W4 /WX /wd4324 /O2 /Z7`. `/Z7` embeds compiler debug information in
the object files, so the build no longer depends on a current-directory PDB outside the declared
output tree. The only disabled warning remains WDK aligned-structure warning C4324.

`Test-WinDriveIoctlValidationStatic.ps1` requires those exact flags and rejects `/Zi`.

## GREEN

Environment: Visual Studio 2022 Build Tools 17.14.35, WDK `10.0.26100.0`, x64, repository accessed
through `\\wsl.localhost\Ubuntu-24.04\home\emdev\codespace\ramshared`.

```text
STATIC_SCSI_LIFECYCLE_TEST=PASS
STATIC_INJECTOR_TEST=PASS
STATIC_WDK_FLAGS_TEST=PASS
BUILD_DRIVERS_OK
ramshared.sys length=32256
ramshared.sys SHA256=A56D4C4F2885CBD2E141F0715B704E92CBE964E2878A4139F00A9F9B9E68FC98
poolstress.sys length=7680
poolstress.sys SHA256=D3E03C342CB5F22175B76B6839BD01B11C36123725A0AC9942D98FD5E23C0CD3
```

These are unsigned local build outputs and were not deployed. The signed guest campaign remains
bound to package SHA256 `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34`.
