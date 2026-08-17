# WSL2-Linux-Kernel contribution-route research

> Snapshot: 2026-08-14. This audit uses public GitHub repository, pull
> request, issue, comment, and commit records. It evaluates the route most
> likely to result in Microsoft shipping the requested configuration; it does
> not treat an externally merged pull request as the only successful outcome.

## Executive conclusion

The current contribution route for WSL kernel configuration requests is a
feature request in [`microsoft/WSL`](https://github.com/microsoft/WSL), followed
by Microsoft-owned internal integration. A community pull request to
[`microsoft/WSL2-Linux-Kernel`](https://github.com/microsoft/WSL2-Linux-Kernel)
is not an accepted general route.

The successful target for
[`microsoft/WSL#41054`](https://github.com/microsoft/WSL/issues/41054) is
therefore:

1. exact current-source evidence;
2. independently reviewable x86 and arm64 decisions;
3. a build- and capability-tested two-commit patch series;
4. a concise issue update asking whether Microsoft will integrate the changes
   internally or explicitly requests a pull request;
5. a pull request only after a maintainer requests that route.

Opening an unsolicited pull request before that response would repeat a known
closed path and reduce, rather than improve, review efficiency.

## Pull-request population

The GitHub API returned 51 pull requests in the repository at this snapshot.
The classification uses GitHub `author_association` and `merged_at` fields.

| Population | Count | Interpretation |
| --- | ---: | --- |
| All pull requests | 51 | Complete public PR population at the snapshot. |
| Merged pull requests | 7 | Primarily Microsoft release/integration work. |
| External-associated pull requests | 41 | `NONE`, `CONTRIBUTOR`, or first-time contributor associations. |
| External-associated merged pull requests | 1 | Microsoft policy automation, not a human community kernel contribution. |
| External-associated PRs since the 2021 route statement | 6 | Five closed human submissions plus the policy bot. |
| Human external PRs merged since the route statement | **0** | No observed counterexample to the published route. |

The sole externally associated merge after the policy statement is
[`#250`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/250), created by
`microsoft-github-policy-service[bot]` to add Microsoft security policy text.
It is not evidence that community kernel/config patches are accepted.

All four currently open pull requests are Microsoft member work:
[`#258`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/258),
[`#259`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/259),
[`#260`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/260), and
[`#261`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/261).

## Why external pull requests were closed

| PR | Subject | Outcome | Decisive reason |
| --- | --- | --- | --- |
| [`#247`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/247) | Add the `dwarves` build dependency | Closed, but technically accepted and applied internally | The maintainer said the commit was applied, but the PR could not be accepted because of the repository's source-management flow. The later Microsoft commit [`3dff7287`](https://github.com/microsoft/WSL2-Linux-Kernel/commit/3dff72875492128d48ed21114406a3f2108c1cdc) preserves the external author, PR link, and maintainer sign-offs. |
| [`#249`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/249) | Enable EROFS configs | Closed | Maintainer explicitly stated that the repository does not take PRs and routed the request to a WSL feature issue for internal discussion. |
| [`#251`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/251) | Add a workflow | Closed | The same no-PR route was restated in 2025, showing that the policy was still active. |
| [`#248`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/248) | Update `config-wsl` | Closed by author | The author stated that the PR was opened accidentally instead of updating only the fork. It provides no acceptance evidence. |
| [`#246`](https://github.com/microsoft/WSL2-Linux-Kernel/pull/246) | Kernel download question | Closed | It was not a code contribution and was redirected to documentation/support. |

The repository
[`README`](https://github.com/microsoft/WSL2-Linux-Kernel#feature-requests)
matches those outcomes: WSL/kernel feature requests belong in
`microsoft/WSL`; general kernel code belongs in the normal upstream Linux
process.

## What successful configuration requests demonstrate

There is no deterministic acceptance formula, but completed and abandoned
issues expose useful review signals.

### Strong signals

- [`microsoft/WSL#12987`](https://github.com/microsoft/WSL/issues/12987) was a
  concrete regression with diagnostics and a specific application failure.
  Microsoft identified the dropped symbol and marked the fix inbound within
  days. Regression priority does not transfer to #41054, which is a feature.
- [`microsoft/WSL#5526`](https://github.com/microsoft/WSL/issues/5526) linked a
  concrete eBPF/header use case, accumulated independent demand, and was later
  closed against a Microsoft-owned enabling commit. It also demonstrates that
  feature adoption can take years.
- [`microsoft/WSL#8302`](https://github.com/microsoft/WSL/issues/8302) and
  [`#11021`](https://github.com/microsoft/WSL/issues/11021) combined concrete
  hardware/use cases with custom-kernel verification before the capability
  appeared in a released kernel.

### Failure and ambiguity signals

- [`microsoft/WSL#9819`](https://github.com/microsoft/WSL/issues/9819) and
  [`#6044`](https://github.com/microsoft/WSL/issues/6044) show that technically
  meaningful requests can be closed automatically after a year without a
  maintained evidence trail.
- [`microsoft/WSL#13174`](https://github.com/microsoft/WSL/issues/13174) was
  closed after users confirmed that a custom kernel solved the problem. A
  custom-kernel PASS must therefore be described as solution validation, not
  as resolution of the stock-kernel request.
- [`microsoft/WSL#11335`](https://github.com/microsoft/WSL/issues/11335) gained
  independent custom-kernel and preview-kernel confirmation but was later
  auto-closed. Evidence should always identify whether the stock, preview, or
  custom kernel was tested.

## Review barriers specific to #41054

The current source pin is
[`14794180686c2fb6307fbe359c359bec765249f3`](https://github.com/microsoft/WSL2-Linux-Kernel/commit/14794180686c2fb6307fbe359c359bec765249f3).
At that revision:

| Architecture | `CONFIG_BLK_DEV_UBLK` | `CONFIG_ZRAM_WRITEBACK` |
| --- | --- | --- |
| x86 | not set | not set |
| arm64 | not set | `y` |

The requests must be independently reviewable:

- `CONFIG_ZRAM_WRITEBACK=y` is an x86 alignment request. The arm64 WSL config
  already enables it. Its Kconfig behavior remains dormant until an
  administrator configures a backing device.
- `CONFIG_BLK_DEV_UBLK=m` is an x86 and arm64 module request. The upstream
  Kconfig still labels the interface experimental, so module size, autoload
  behavior, capability cleanup, and attack-surface reasoning must be explicit.

Microsoft can then accept the lower-risk x86 writeback alignment without being
forced to accept ublk in the same decision.

## Patch-ready format if requested

The patch series should remain two logical commits:

1. `config: enable CONFIG_ZRAM_WRITEBACK on x86`
2. `config: enable CONFIG_BLK_DEV_UBLK`

Each commit must contain a `Signed-off-by` trailer. The series changes only:

```text
arch/x86/configs/config-wsl
arch/arm64/configs/config-wsl-arm64
```

The second commit changes both architectures; the first changes x86 only.
Evidence must record `olddefconfig` for each architecture, an x86 `W=1` build,
module packaging, size deltas, a bounded capability smoke test, and clean
teardown. No RamShared Rust source belongs in the Microsoft patch.

## Decision

`#41054` remains the canonical public route. The next external action should be
one substantive issue update with the evidence packet and this explicit
question:

> Would the WSL kernel team consider these two independently reviewable config
> changes for the stock kernel? If so, should they be integrated internally,
> or would you like the prepared two-commit patch series submitted as a pull
> request against `linux-msft-wsl-6.18.y`?

Only the latter response opens the pull-request gate. A Microsoft internal
commit that credits or links the external patch is a successful contribution,
even if the community PR itself is never merged, as demonstrated by #247.
