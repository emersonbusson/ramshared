# InfVerif DIRID 13 isolation pass — 2026-07-16

## Scope

Static Windows Driver Kit validation of `drivers/windows/ramshared/ramshared.inf`. No driver was
installed or loaded by this validation.

## RED

After migrating `DefaultDestDir` and `ServiceBinary` from DIRID 12 to DIRID 13, InfVerif reported
`ERROR(1199)`: the models section still permitted installation before Windows 10 build 16299, where
run-from-Driver-Store support was unavailable.

## GREEN

The manufacturer/models decoration was restricted to `NTamd64.10.0...16299`, and the real WDK tool
was rerun:

```text
Windows Kits 10.0.26100.0 x64 InfVerif.exe /w drivers/windows/ramshared/ramshared.inf
exit=0
stdout/stderr: empty
```

The package now uses `DefaultDestDir = 13`, `ServiceBinary = %13%\ramshared.sys`,
`PnpLockdown = 1`, and service-relative registry values only below `Parameters`.

## Verdict

`InfVerif /w`: **PASS**. Live install/load/BINARY_MATCH remains a separate isolated-VM gate.
