# IMPL — cascade-transport-policy

> Passo 3 SSDV3. Implements [`SPEC.md`](SPEC.md). AUDIT-2.5: **GO** (NBD Day-1).  
> **Date:** 2026-07-10  
> **Status:** **HISTORICAL NBD CAPABILITY EVIDENCE; CURRENT AUTOMATIC BOOT
> DISABLED.** The 2026-07-10 custom-kernel smoke remains evidence for that
> exact run only. The 2026-08-23 DXG/systemd finding makes current live kernel
> promotion NO-GO; NBD remains the independent Day-1 transport decision.

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| V1 priority log | **GREEN** | `[up] prioridade: zram(200) > VRAM/nbd(100) > VHDX` |
| V2 zram 200 + nbd 100 | **GREEN** | `swapon --show` live |
| V3 down clean | **GREEN** | prior cascade smoke; unit keeps managed path |
| V4 auto → nbd on WSL2 | **GREEN** | log + no ublk product path |
| V5 unit enabled | **GREEN** | `systemctl is-enabled ramshared-cascade` = enabled |
| ublk fail-closed | **GREEN** | explicit `--transport ublk` errors before mutate |
| cargo test -p ramshared-cli | **GREEN** | see validation entry |

## 2026-08-23 bounded-custody remediation

- Every short-lived cascade command now uses the shared bounded runner: a new
  invocation-private process group, 64 KiB per-stream storage, concurrent
  capture, exact group SIGKILL on timeout or inherited output, and bounded
  direct-child reap.
- Pre-attach daemon children also lead private groups. Failed readiness and
  proven-no-effect rollback terminate the exact group and reap the direct
  child; uncertain NBD/swapon effects still preserve the identity-bound daemon
  and forensics instead of guessing cleanup.
- `bounded_command_contains_descendant_that_inherits_output` pairs the
  adversarial inherited-pipe case with legitimate success/nonzero cases.
  `unreaped_group_selects_fatal_controller_containment` injects an unreapable
  target and proves stable exit-125 containment is selected without exiting
  the test process.
- No live cascade, module, swap, NBD, daemon, WSL, device, or systemd action was
  performed by this source remediation.

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 priorities | `crates/ramshared-cli/src/cascade/cascade_io.rs` (`up` priority log); `ramshared-tier` defaults |
| ITEM-2 boot | `scripts/safety/install-cascade-boot.sh --enable`; unit already in wsl2-cascade-boot |
| ITEM-3 transport auto | `cascade/mod.rs`: `Transport::Auto`, `is_wsl2`, `resolve_transport`; ublk refuse in `cascade_io.rs` |
| ITEM-4 idempotent | existing `cascade_already_healthy` (unchanged contract) |

## Small decisions

1. **Default transport = Auto**, not Nbd, so off-WSL2 can prefer ublk later without flag flip.  
2. **ublk check runs before idempotent `up`** so explicit ublk on WSL2 never returns “already healthy”.  
3. **Did not** implement full ublk wire in `up` (SPEC out of scope; AUDIT NO-GO).  
4. Kernel `ublk_drv` live is **capability**, not product path.

## Validation numbers (this host, 2026-07-10)

| Metric | Value |
| --- | --- |
| uname | 6.18.35.2-microsoft-standard-WSL2+ |
| swap order | zram0 prio **200**, nbd0 prio **100**, sdc prio **−2** |
| sizes | zram 1024M, nbd 1024M, sdc 8G |
| unit | enabled + active (exited), daemon `ramsharedd --nbd /dev/nbd0` |
| ublk kernel | `ublk_drv` loaded; `/dev/ublk-control` present; **not** used by cascade |

## Gaps

| Gap | Class |
| --- | --- |
| Soak reboot 2× after enable | **Hygiene** — human/lab; no new SPEC (covers existing boot SPEC) |
| Full ublk `up` wire | Future SPEC + AUDIT-2.5 |
| Pressure thrash | Host-unsafe on live WSL2 — only qemu/civm |
| Custom-kernel DXG/systemd requalification | **NO-GO** until the exact-distro bundled/custom A/B canary passes; see the [2026-08-23 finding](../../../reliability/incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md) |

## Rollback trigger

- Ghost nbd/ublk in `/proc/swaps` after boot → `ramshared down` then investigate; if unit loops, `uninstall-cascade-boot.sh`.  
- Host freeze after any ublk experiment → never re-enable product ublk; keep NBD.
- Any custom-boot FORTIFY/init-timeout/unclean/p9/fatal signal, failed DXG probe,
  systemd non-running state, or query-error regression → disarm to bundled and
  keep all RamShared device activation off.

## Traceability

PRD RF-T1..T5 → SPEC ITEM-1..4 → this IMPL.  
Related: `wsl2-cascade-boot` (unit), `wsl2-custom-kernel-p1` (ublk module available).
