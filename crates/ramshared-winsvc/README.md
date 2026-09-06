# ramshared-winsvc

Windows StorPort virtual disk worker daemon, volume locking governor, and pagefile safety monitor.

## Scope & Responsibility

`ramshared-winsvc` executes as the user-mode service backing the Windows StorPort virtual miniport driver:
- **StorPort Request Completion:** Fulfills read and write SCSI requests forwarded by `drivers/windows/ramshared`.
- **Volume Lock Isolation:** Uses the [`LockedVolume`](src/windows_host.rs) RAII guard pattern to isolate drive volumes during format and mount transitions.
- **BugCheck 0x7A Prevention:** Enforces ordered teardown (DT-9) to guarantee that virtual disks backing Windows pagefiles are never detached while active.
- **Cross-Platform Testability:** Decouples core logic from Windows APIs, enabling full unit test execution on Linux via `FakeDriver` in `#![forbid(unsafe_code)]`.

## Workspace Dependencies

- [`ramshared-broker`](../ramshared-broker/README.md) — Broker protocol tenant.
- [`ramshared-config`](../ramshared-config/README.md) — Service configuration.
- [`ramshared-cuda`](../ramshared-cuda/README.md) — Host CUDA allocation probes.

## Safety Invariants

- **Safe Volume Locking:** Strictly manages volume handles with typed errors to prevent deadlock during unmounts.
- **Non-Windows Safety:** Entire non-Windows codebase compiles under `#![forbid(unsafe_code)]`.

## Testing

```bash
cargo test -p ramshared-winsvc
```
