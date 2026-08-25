# 2026-08-23 WSL2 DXG FORTIFY warning and kernel-lab boot NO-GO

## Verdict

The observed DXG warning is a real guest-kernel integrity warning in the
WSLg/Xwayland wait-sync-object path. It is classified as
`upstream_open_dxg_fortify_warning`, not as a defect introduced by RamShared.
The repeated `RamShared-Kernel` init timeouts are separately classified as
`custom_kernel_lab_boot_unqualified`; their root cause is unresolved.

No RamShared service or device was activated during this investigation. The
custom-kernel promotion gate remains **NO-GO**.

## Observed evidence

On the current `6.18.35.2-microsoft-standard-WSL2+` boot,
`dmesg --level=err,warn` contains:

- `memcpy: detected field-spanning write (size 4)` at
  `drivers/hv/dxgkrnl/dxgvmbus.c:3095`;
- a warning from Xwayland through
  `dxgvmb_send_wait_sync_object_gpu` and
  `dxgkio_submit_wait_to_hwqueue`;
- repeated `dxgkio_query_adapter_info`/`is_feature_enabled` failures with
  `-22` and `-2`;
- repeated attempts in the `RamShared-Kernel` distro where
  `WaitForBootProcess: /sbin/init failed to start within 10000ms`;
- journal files renamed as corrupt or unclean and later p9
  `Operation canceled` messages.

The warning trace was not tainted and did not contain a kernel panic. The
operator's bounded checks reported no new EXT4 error and no Windows System
disk event in the last 12 hours. Those negative observations reduce evidence
for a new storage failure; they do not prove GPU or systemd health.

## Upstream comparison

The exact FORTIFY signature predates the local artifact and is independently
reported against multiple Microsoft kernels:

- [`microsoft/WSL#40580`](https://github.com/microsoft/WSL/issues/40580):
  `6.18.26.1`, Xwayland, same function and source line;
- [`microsoft/WSL#41060`](https://github.com/microsoft/WSL/issues/41060):
  bundled `6.18.33.2`, same warning plus a separate resource-exhaustion
  environment;
- [`microsoft/WSL#41017`](https://github.com/microsoft/WSL/issues/41017):
  bundled package `6.18.33.2-2`, same warning and a reporter-observed CUDA
  hang/device-not-ready correlation;
- [`microsoft/WSL#41093`](https://github.com/microsoft/WSL/issues/41093):
  `6.18.35.2` and a proposed patch that remains an issue-level proposal, not
  an accepted upstream change.

The official
[`WSL2-Linux-Kernel` release index](https://github.com/microsoft/WSL2-Linux-Kernel/releases)
labels `6.18.40.1` as latest at the 2026-08-23 review time. The local artifact
is still `6.18.35.2`. A newer tag is only a candidate: neither release metadata
nor source inspection substitutes for the live A/B canary.

Therefore:

1. `6.18.35.2` did not introduce the signature relative to bundled
   `6.18.33.2-2`;
2. reproducing it on the bundled kernel does not make it safe;
3. issue-reporter correlation is not proof that this warning caused a CUDA
   hang, a distro init timeout, or the earlier storage incident;
4. RamShared must not carry or auto-apply the unaccepted patch from #41093.

## Causal classification

| Observation | Classification | Confidence | Consequence |
| --- | --- | --- | --- |
| DXG field-spanning warning | upstream open regression/confounder in the WSL 6.18 line | high for provenance; unresolved for runtime consequence | GPU-backed promotion blocked |
| `query_adapter_info -22/-2` burst | upstream/protocol telemetry requiring same-host A/B | medium | compare count; do not invent a zero-error threshold |
| `WaitForBootProcess` init timeout | custom-kernel lab boot unqualified | high for qualification, low for root cause | systemd promotion blocked |
| unclean journal rename and p9 cancellation | evidence of interrupted/failed distro attempts | medium | fresh occurrence is a rollback signal; no causal bridge to DXG claimed |
| 2026-08-22 host incident | `host_volume_exhausted` from NTFS 137/`STATUS_DISK_FULL` | high | remains separate; no RamShared causation |

## Source control added

The corrected normal path first installs a versioned, hash-bound host bundle
containing the reviewed wrapper/launcher and exact kernel/modules/layout/QEMU
pair. It does so before shutdown, so loss of the WSL repository UNC cannot
retarget the launcher to a stale global script. The installed scripts run under
Windows PowerShell 5.1.

`scripts/kernel/boot-kernel-safe.ps1` now confirms a custom kernel only after
a bundled/candidate A/B transaction against an exact distro proves all of the
following:

- exact sealed `uname -r`;
- systemd state `running`;
- `/dev/dxg` is exactly one character device, a bounded `xdpyinfo` transaction
  exercises WSLg/Xwayland, and a bounded `nvidia-smi` metadata query succeeds;
- required module metadata exists without loading a module;
- the fresh warning log is readable;
- zero field-spanning warnings, init timeouts, current-boot unclean-journal
  messages, p9 cancellations, or kernel-fatal signatures;
- DXG query-error count does not exceed an attended same-host bundled-kernel
  baseline.

Any missing, malformed, timed-out, degraded, or non-zero hard gate causes
deterministic disarm and rollback. A natural-reboot `arm` without auto-revert is
refused by default and retained only behind an explicit unsafe-lab token.
Rollback is recorded as restored only after a third fresh boot returns to the
same kernel, distro, GPU driver, and DXG cardinality as the valid bundled
baseline; key removal or a fresh boot ID by itself is not sufficient evidence.

The 6.18.40.1 unified modules artifact is not an upgrade path on WSL 2.7.12.
Unified-layout support was merged after that runtime; admission therefore
rejects unified or double-nested artifacts until an exact reviewed released WSL
runtime is allowlisted. No 6.18.40.1 artifact was downloaded, built, or used by
this remediation.

## Required live A/B gate

Live work remains pending and requires separate attended approval. The future
campaign must use the same host, Windows/WSL/WSLg build, GPU driver, distro,
and lightweight probe for both legs:

1. fully shut down WSL and boot only the sealed daily distro on the bundled
   kernel;
2. capture the fresh-boot query-error baseline and every hard canary field;
3. fully shut down and run the custom candidate through the auto-reverting
   launcher;
4. do not activate RamShared, swap, NBD, ublk, zram, CUDA allocation, Docker,
   or a pressure workload during this gate;
5. promote only if all hard fields pass and the query-error count is no worse
   than baseline.

If the bundled leg itself emits the FORTIFY warning, both legs remain NO-GO for
the DXG-qualified product path. A clean bundled leg plus a failing custom leg
is a custom-candidate regression. A clean candidate does not close separate
ublk, swapoff-first, storage, or pressure qualification.

## Rollback criteria

Rollback/disarm on any of these signals in the candidate boot:

- exact-version mismatch or canary timeout;
- systemd not `running`, `/sbin/init` boot timeout, or unreadable evidence;
- missing DXG/Xwayland/NVIDIA probe;
- any `memcpy: detected field-spanning write` in the DXG path;
- any fresh current-boot unclean journal, p9 cancellation, kernel BUG/Oops, or
  panic signature;
- DXG query-error count above the sealed bundled baseline;
- required module metadata absent.

This rollback list is intentionally stricter than “the kernel returned an
`uname`”. It makes uncertainty a refusal and preserves the bundled-kernel path.
