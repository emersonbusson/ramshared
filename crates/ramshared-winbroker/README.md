# ramshared-winbroker

Windows Service Control Manager (SCM) broker daemon for least-privilege lease arbitration.

## Scope & Responsibility

`ramshared-winbroker` runs as a dedicated Windows service (`RamSharedBroker`) on the host system:
- **SCM Service Lifecycle:** Integrates with Windows Service Control Manager for automated start, stop, and failure recovery.
- **Local Named-Pipe Boundary:** Listens exclusively on local authenticated named pipes (`\\.\pipe\ramshared-broker`), exposing zero network listeners.
- **Console Debug Mode:** Supports `--config` and `console` execution for development and continuous integration tests.

## Workspace Dependencies

- [`ramshared-broker`](../ramshared-broker/README.md) — Pure broker protocol, slice map, and arbitration rules.
- [`ramshared-config`](../ramshared-config/README.md) — Windows configuration file parser.

## Safety Invariants

- **Least Privilege:** Runs under isolated local service credentials.
- **Zero Remote Exposure:** Refuses network-routable connections.

## Testing & Compilation

```bash
cargo test -p ramshared-winbroker
```
