# ramshared-config

Shared configuration schemas, validation rules, and fail-closed limit enforcement.

## Scope & Responsibility

`ramshared-config` parses and validates TOML configuration files across broker, agent, and resource-policy surfaces:
- **Broker & Agent Schemas:** Strongly typed representations of listen addresses, slice sizes, allocation floors, and watchdog timeouts.
- **Resource Profile Model:** Versioned variable tier ceilings and multiple stable Linux/WSL2 storage targets. Swap and origin can be placed on the same or different volumes; checked capacity is grouped by stable volume identity. The model is currently a pure parser/validator and is not yet loaded or persisted by `ramshared config`.
- **Fail-Closed Validation:** Validates memory bounds, socket permissions, and backend selections before daemons attempt resource initialization.
- **Pure Library Design:** Parsing logic is fully decoupled from I/O to enable deterministic offline unit testing.

## Workspace Dependencies

- Pure configuration library; zero internal workspace dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced.
- **Typed Errors:** Returns [`ConfigError`](src/error.rs) on invalid or unparseable input without process termination.
- **Resource Profile Bounds:** The profile parser caps input at 64 KiB, rejects unknown schema fields and unsafe paths, and keeps host/guest mutations outside this crate.

## Testing

```bash
cargo test -p ramshared-config
```

The resource-profile slice has its own test target, `tests/resource_profile.rs`.
