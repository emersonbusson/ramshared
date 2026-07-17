# Product teardown hardening — static verification 2026-07-16

## Verdict

Implementation and static gates are **PASS**. Live product promotion remains **PARTIAL** until a
new signed package is deployed to the isolated VM and the corrected campaign passes.

## RED/GREEN cases

- Missing fail-closed pagefile merge, lock-deadline decision, and complete campaign conjunction:
  compile-time RED, then five `host_safety` tests GREEN.
- Harness missing `CONSOLE_EXIT_ZERO` and fresh lifecycle rounds: static RED, then GREEN after the
  complete no-retry verdict and three lifecycle rounds were implemented.
- DIRID 13 initially produced InfVerif `ERROR(1199)`; build-16299 model decoration made `/w` GREEN.
- `VramBackend` could not be consumed before lease release: compile-time RED, then
  `vram_backend_into_inner_allows_explicit_release_order` GREEN.
- Broker flush failure lost authoritative lease state: RED, then
  `failed_release_retains_lease_and_is_not_replayed` GREEN.

## Verification

```text
cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets
  block: 42 passed
  cuda: 5 passed, 1 ignored (live GPU)
  winsvc: 84 passed, 1 ignored (live CUDA)

cargo clippy ... -D warnings: PASS (native and Windows target)
Windows MSVC release build from disposable local staging: PASS
  SHA-256: 3BBB69722F1BE47A5AC2CA39EE66A24052227085238DC3B3DE08ABD10407E25A
WDK /W4 /WX /wd4324 /Z7 ramshared.sys + poolstress.sys build: PASS
InfVerif.exe /w: PASS
PowerShell 5.1 parse + product/INF/static injector tests: PASS
docs-check + git diff --check: PASS
host_safety.rs line coverage: 96.5%; broker_tenant.rs: 83.3%; vram_backend.rs: 91.6%;
all required slices >=80%
```

## Environment-bound blocker

The machine certificate is visible in `LocalMachine`, but SignTool's private-key filter returns zero
for the current non-elevated token. The PFX password is not present. No ACL, certificate, trust-store,
driver install, or VM state was changed to bypass this. Therefore no new CAT/package was signed and
no live campaign was attempted.
