# Fuzzing Guide

## Continuous Fuzzing Setup

This guide documents the setup and execution of the continuous fuzzing infrastructure for RamShared.

### Framework
We use `cargo-fuzz` for coverage-guided fuzzing, built on libFuzzer. It provides structure-aware mutational fuzzing for our Rust interfaces, particularly the memory allocation and swap isolation layers.

### Targets
1. `allocate_vram`: Fuzzes VRAM allocation sizes and bounds checking.
2. `protocol_parser`: Fuzzes the WSL-to-Host communication protocol parsing.

### Workflow
1. Initialize fuzzing environment:
   ```bash
   cargo fuzz init
   ```
2. Run a specific target:
   ```bash
   cargo fuzz run allocate_vram
   ```

### Corpus Management
The fuzzing corpus is maintained in the `fuzz/corpus/` directory. New findings from continuous CI fuzzing are automatically minimized and merged into this directory to prevent regression.
