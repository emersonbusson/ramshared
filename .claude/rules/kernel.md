# Global Rules for LLMs in RamShared

This document defines how AIs and Assistants must generate C/Rust code for the RamShared project.

## Language and Formatting
- **C Kernel Style:** Rigorously use the standard Linux Kernel style (indentation with 8-space TABs, 80-character line limit, open brackets on the same line for `if`/`for`).
- **Rust for Linux:** If using Rust, strictly follow `alloc` and `core` standards, avoiding `std`. Use `rustfmt` with mainline configurations.
- All AI output for `.c` or `.rs` files must be ready to pass the Linux kernel's `checkpatch.pl` script.

## Semantics and Memory
- Never assume virtual pointers can be passed directly to hardware. Explicitly map and unmap (e.g., `dma_map_single`, `pci_iomap`).
- Every lock (spinlock, mutex) must have a clear and justified scope. Give extreme attention to priority inversion and deadlocks in interrupt contexts.
- Do not leave memory leaks and free resources in the exact reverse order of allocation in error routines (using the `goto out_err;` idiom).

## Documentation and SPEC
- Never write direct implementations based on the PRD. Use the SSDV3 methodology (PRD -> SPEC -> IMPL).
- PRDs and SPECs must have documented Kernel Panic mitigation disciplines using the Kahneman framework.
