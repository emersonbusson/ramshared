# ramshared-config

Shared configuration schemas, validation rules, and fail-closed limit enforcement.

## Scope & Responsibility

`ramshared-config` parses and validates TOML configuration files across broker and agent environments:
- **Broker & Agent Schemas:** Strongly typed representations of listen addresses, slice sizes, allocation floors, and watchdog timeouts.
- **Fail-Closed Validation:** Validates memory bounds, socket permissions, and backend selections before daemons attempt resource initialization.
- **Pure Library Design:** Parsing logic is fully decoupled from I/O to enable deterministic offline unit testing.

## Workspace Dependencies

- Pure configuration library; zero internal workspace dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced.
- **Typed Errors:** Returns [`ConfigError`](src/error.rs) on invalid or unparseable input without process termination.

## Testing

```bash
cargo test -p ramshared-config
```
