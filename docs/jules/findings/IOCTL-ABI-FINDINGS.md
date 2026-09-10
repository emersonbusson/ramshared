# Finding: RamShared Kernel IOCTL Interfaces

## Context
Task: Replace `long`/`int` with `__u32`, `__u64` in user-kernel shared ABI headers (specifically `drivers/block/ramshared/ramshared.h`) to guarantee 32/64-bit parity.

## Finding
I have audited the `drivers/block/ramshared/` codebase, focusing on `ramshared.h` and other header files, looking for IOCTL interface structs and instances of `long` or `int` that need to be migrated to fixed-width integer types (`__u32`, `__u64`).

The Linux in-tree driver (`drivers/block/ramshared`) **does not use custom IOCTL operations**. It relies entirely on standard `blk-mq` mechanisms and sysfs attributes for control and configuration. There are no custom IOCTL interface structs defined in `ramshared.h` (or anywhere else in the Linux driver source).

The Windows driver counterpart (`drivers/windows/ramshared/protocol.h`) and its service daemon (`crates/ramshared-winsvc/src/proto.rs`) *do* define custom device I/O control codes and communication structures. However, an audit of these files confirms they **already use proper fixed-width types** (`ramshared_u32`, `ramshared_u64`, etc.).

## Conclusion
Safe code modification is not possible because the target architecture (Linux in-tree driver IOCTL structs) does not exist. No modifications are needed to guarantee 32/64-bit parity for the Linux driver ABI.

This finding is reported in accordance with Contract Rule 4 ("If safe code modification is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/").
