# FAQ — current candidate

## Current boundary — disabled staging only

RamShared is not currently installable or activatable from this documentation.
Do not use it as a quick-start, desktop-control, boot-integration, WSL
configuration/application, VM lifecycle, guest-formatting, storage/VHDX/GPU
action, or pressure-campaign guide. Historical command transcripts are omitted
on purpose; retained figures and verdicts apply only to their recorded builds.

The legacy full-VRAM NBD backend composition and
`RAMSHARED_VRAM_PREALLOC_LEGACY` selector were removed from executable source.
The named sunset test, thresholded checker coverage, clean active-source/current
document scan, and documentation-governance check close only that source
governance prerequisite. Focused Rust tests, rustfmt, and Clippy for this exact
worktree remain pending on the external Guard repair. Qualification, release
promotion, and activation remain `BLOCKED` on live incident-specific evidence,
and managers remain disabled/plan-only.

## What is RamShared intended to model?

The source candidate models compressed RAM first, an SSD-authoritative logical
device with a clean revocable VRAM cache second, and existing disk/VHDX swap
last. Acknowledged data belongs to the origin, not VRAM. If GPU measurement or
allocation fails, cache capacity becomes zero while the origin path remains the
correctness boundary.

## Will it freeze a PC?

The candidate's retained safety contract requires identity-checked,
swapoff-first origin detach; it must never detach a daemon while its device can
still be used for swap. Historical hard-reclaim evidence measured a roughly
**1.2 s** tiny-read stall and full demotions of hundreds of MiB on the order of
**tens of seconds**. Those are observations, not responsiveness guarantees.

The current candidate does not authorize any thrash or pressure test, especially
not on a daily WSL2 host. A `PARTIAL` result is evidence of an open gate, not a
failed test and not a release claim.

## Is this free RAM for games?

No. A game or other external workload has priority for the GPU budget. The
candidate reserves `max(2 GiB, 20% of total VRAM)` and treats unknown WDDM/GPU
measurement as zero cache target. It neither promises a fixed amount of VRAM
nor identifies applications by name.

## Why did Task Manager show an unusual virtual disk?

This is retained historical lab evidence, not a current lab procedure. A
64 MiB virtual LUN could appear fully busy with zero throughput or latency when
class-driver polling and a miniport readiness condition disagreed. The Day-0
driver correction changed the not-ready result to a standards-compliant
not-ready condition rather than an indefinitely busy response.

The retained live record used a sanitized product LUN, generated **304 MiB** of
write/read traffic during sampling, and matched a direct **8 MiB** checksum
probe. It observed non-zero busy/write/queue counters and recorded
`DISK_IO_MEASURE_OK=true`. The verdict is that Task Manager alone was not a
correctness gate; the historical measurement path passed for its exact build.

## What do the candidate status terms mean?

| State | Intended meaning |
| --- | --- |
| `Armed` | The SSD-authoritative logical tier would be present; cache use may still be near zero. |
| `UsingZram` | Pressure is primarily in compressed RAM. |
| `UsingVram` | The cache would contain attributable data. |
| `UsingDisk` | The existing lower disk/VHDX tier would be in use. |
| `Demoting` | The cache would be releasing capacity under a restricted budget. |
| `Degraded` | Identity, origin, control, guardian, or cache evidence is not safe to rely on. |
| `Off` | No product cascade is present. |

Schema v4 distinguishes physical GPU use, logical capacity, cached VRAM,
authoritative-origin writes, fallback swap use, memory pressure, and control or
guardian state. The interface description does not authorize inspecting or
changing a host.

## Can the desktop control or boot integration be used?

No. Disabled definitions may exist in source for future review, but no current
desktop-control, package-install, boot, resume, uninstall, or systemd action is
authorized. The source-removal prerequisite is closed; a future attended
rollout still requires fresh incident-specific qualification, exact sealed
origin identity, fresh watchdog proof, and a separate approval.

## What about WSL configuration paths?

Historical evidence found that Windows-style backslashes can be interpreted as
escapes in WSL configuration. The public record deliberately uses placeholders
instead of a host-observed path. No configuration rewrite or WSL apply action is
authorized by this FAQ.

## What happens under external GPU pressure?

The intended candidate policy stops new cache commits, drops clean chunks, and
continues through the authoritative origin. A future guardian would require
independent failed-health evidence, safe-mode persistence, exact sealed-distro
identity, and a bounded recovery proof before any targeted action. It has no
broad WSL shutdown or automatic Windows reboot path.

## Can the Windows driver be installed on a physical host?

No. Physical-host qualification is open. Historical test-signed lab evidence is
not public distribution evidence. A future physical campaign would require a
production-trusted package, exact identity/integrity checks, explicit fresh
reboot approval, supported teardown, and rollback evidence. A pagefile-active
backend teardown can cause Windows bugcheck **0x7A**.

## Does GDDR6 mix directly with DDR4?

No. GPU and system memory are managed by different controllers; data crosses
PCIe. Historical bandwidth/latency figures are transport observations, not a
promise of memory compatibility or performance.

## Where are the verified records?

[validation.md](../validation.md) is the append-only empirical log and
[reliability evidence](reliability/) records open gates. If a number is not
recorded there with context and a verdict, treat it as unverified.
