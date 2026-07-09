# Windows drivers tree

Day-0 StorPort path for **native Windows** VRAM pagefile (P4 / Track 2).

- Product SPEC: `docs/specs/no-milestone/windows-swap-driver/`
- Userspace service: `crates/ramshared-winsvc/`
- VM harness scripts: `scripts/windows/`

Linux / WSL2 cascade remains the day-1 public product under `crates/ramshared-*` (no Windows driver required).
