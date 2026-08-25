# Safe WSL2 memory and storage reclaim

This runbook is guidance for review and an attended maintenance window. It does
not authorize changing `.wslconfig`, shutting down WSL, or compacting a VHDX
during an active work session.

## Production policy

- Omit `sparseVhd`. On WSL 2.7.12, live sparse conversion is refused because of
  its corruption risk; RamShared never uses `--allow-unsafe` on the daily host.
- RAM reclaim and disk reclaim are different operations. RAM may be returned in
  bounded `memory.reclaim` increments, with a fresh pressure observation between
  increments. Never write `max` or run an unbounded reclaim loop.
- Physical VHDX shrinkage belongs only to an attended maintenance transaction:
  save and back up work, stop workloads, run `fstrim` while the guest is still
  reachable and idle, prove every WSL distribution is fully stopped, and only
  then run `Optimize-VHD` against the offline VHDX.
- Maintenance refuses an attached VHDX, an ambiguous parent/child chain, a
  missing backup, insufficient free host-volume capacity, or identity that
  differs from the sealed manifest.

## Migrating the current configuration

1. The merger or reviewer compares the current `.wslconfig` with the read-only
   output of `scripts/safety/wslconfig-ctl.sh render`.
2. Remove `sparseVhd=true`; do not translate it into
   `wsl --manage ... --set-sparse true`, and never force it with
   `--allow-unsafe`.
3. Apply the configuration only after every session has saved and synchronized
   its work. A complete WSL restart is a later, attended action.
4. After reopening the distribution, review memory, swap, kernel, and modules.
   Removing `sparseVhd` alone does not compact or rewrite the existing VHDX.

## Unsafe laboratory exception

The renderer emits `sparseVhd=true` only when both gates below are present:

- `WSLCONFIG_UNSAFE_LAB_MODE=1`;
- `WSLCONFIG_UNSAFE_LAB_SPARSE_APPROVAL` equals the explicit token documented
  by `wslconfig-ctl.sh --help`.

This opt-in is only for a disposable distribution with a verifiable backup. It
does not authorize `--allow-unsafe`, is not production evidence, and must not
appear in the daily-host configuration.
