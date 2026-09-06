# ramshared-uring

Safe, high-performance Linux `io_uring` asynchronous I/O abstractions.

## Scope & Responsibility

`ramshared-uring` isolates raw Linux `io_uring` system call mechanics to provide asynchronous, zero-copy block transfers:
- **Submission and Completion Ring Wrappers:** Type-safe abstractions over SQEs and CQEs.
- **Buffer Registration & Fixed Files:** Reduces kernel-space translation overhead during continuous high-throughput block paging.
- **Semantic Error Mapping:** Converts raw Linux errno codes into strongly typed [`UringError`](src/lib.rs) enums (`InvalidInput`, `OutOfRange`, `Busy`, `Timeout`, etc.).

## Workspace Dependencies

- Pure low-level I/O abstraction; zero internal workspace dependencies.

## Safety Invariants

- **Narrow Unsafe Scope:** Confines `unsafe` ring manipulation strictly behind safe, documented interfaces (`#![deny(unsafe_op_in_unsafe_fn)]`).
- **Memory Safety:** Prevents use-after-free by ensuring registered user buffers remain pinned throughout asynchronous operations.

## Testing

```bash
cargo test -p ramshared-uring
```
