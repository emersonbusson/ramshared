# IMPL — WSL #41054 config-only contribution evidence

## Status

`PASS` across all technical, build, Sparse, packaging, and capability gates on
both x86 and arm64 architectures. This status is patch readiness only: Microsoft
adoption is unclaimed and `MAINTAINER_REQUESTED_PR_GATE` remains `REFUSED` until a
Microsoft maintainer explicitly requests a pull request.

## Source and route

- Repository: `microsoft/WSL2-Linux-Kernel`.
- Branch: `linux-msft-wsl-6.18.y`.
- Reviewed commit: `14794180686c2fb6307fbe359c359bec765249f3`.
- Reviewed release ancestry: `linux-msft-wsl-6.18.40.1`.
- Revalidation date: 2026-08-14.
- Canonical inputs: `arch/x86/configs/config-wsl` and
  `arch/arm64/configs/config-wsl-arm64`.
- Public route: one tested fork branch and one evidence update to
  `microsoft/WSL#41054`. No pull request to the Microsoft kernel repository is
  authorized without an explicit maintainer request.

The route audit in [`RESEARCH.md`](RESEARCH.md) found no human external merge
after the repository's 2021 no-community-PR statement. WSL2-Linux-Kernel #247
shows the useful success pattern: Microsoft can apply an external contribution
internally while closing the external PR.

## Patch series

The candidate tree is `f0d6dcae6d6da8d642195f65b7a1b70f649453fc` and
contains exactly two commits above the reviewed source:

| Order | Commit | Subject | Files |
| --- | --- | --- | --- |
| 1 | `19f97435c8d5a5eac06f6747f5f118962bdb2a21` | `config: enable CONFIG_ZRAM_WRITEBACK on x86` | x86 config only |
| 2 | `aab3e0586d0f169a81b215aacde51225b8f0b6ef` | `config: enable CONFIG_BLK_DEV_UBLK` | x86 and arm64 configs |

Both commits use the canonical author identity and `Signed-off-by` trailer
(matching git configuration), link `microsoft/WSL#41054`, pass strict
`checkpatch`, and apply cleanly to the reviewed commit.

| Patch | SHA-256 |
| --- | --- |
| `0001-config-enable-CONFIG_ZRAM_WRITEBACK-on-x86.patch` | `ac6dd7f37549e1f881387b865eccb6e83fe7a47aa7d6cd30d61ada8ea291aeb7` |
| `0002-config-enable-CONFIG_BLK_DEV_UBLK.patch` | `c702441fb69eb7307ec9b7a9edd2abee9c4753f08538d2bc1731f84787b60086` |

## Generated configuration

| Architecture | Baseline | Candidate | Result |
| --- | --- | --- | --- |
| x86 | ublk not set; writeback not set | `CONFIG_BLK_DEV_UBLK=m`; `CONFIG_ZRAM_WRITEBACK=y`; legacy opcodes `y` | PASS |
| arm64 | ublk not set; writeback `y` | `CONFIG_BLK_DEV_UBLK=m`; writeback remains `y`; legacy opcodes `y` | PASS |

Generated config SHA-256 values:

- baseline x86: `5c386fa9f1d87c4416a198aa82a0a141473b435e3a946e2e68c346cf23e93349`;
- candidate x86: `66df5d19f736093ec2202d208f982aa9b5c399cf52d826ab5abc73a4615fb5ee`;
- baseline arm64: `940d5919e5c7554ce7d3ca7c66d36d4ca54dfccf72763393adb6ecc950afac5c`;
- candidate arm64: `b7cfcd31ea262caa152208970259283017c95705dbea11edb97fa0b79c2b4fa7`.

`CONFIG_BLKDEV_UBLK_LEGACY_OPCODES=y` is an upstream-derived default. It is
disclosed for compatibility/security review and is not represented as a third
source-line request.

## x86 build and capability evidence

The baseline and candidate used GCC 13.3.0 and produced kernel release
`6.18.40.1-microsoft-standard-WSL2+`.

| Metric | Baseline | Candidate |
| --- | ---: | ---: |
| `bzImage` bytes | 15,373,312 | 15,373,312 |
| Loadable modules | 736 | 737 |
| Unique `W=1` diagnostics | 20 | 20 |
| Candidate-only `W=1` diagnostics | N/A | 0 |

The candidate `bzImage` SHA-256 is
`4c688b97358741f93a692e690aeea3731cef83125afe4b3060a9e20e860d3e33`.
The candidate zram module is 858,120 bytes with SHA-256
`06efa5d3aa681de6b2de9bf348a6f6b96f10e3f856b3f3c861074ecb00c27886`.
The new ublk module is 922,464 bytes with SHA-256
`504d610345e749f7efffd382427ec9cf8aa0e312ec9deca58881e40dc968db23`.

The isolated QEMU guest booted that exact candidate and proved:

1. ublk and zram were inactive before module load;
2. `/dev/ublk-control` appeared after loading `ublk_drv`;
3. zram exposed `backing_dev` and `writeback`;
4. 1,024 pages were written to a private virtual backing disk;
5. reset and module removal left neither control node nor zram device.

The sanitized QEMU serial receipt SHA-256 is
`6c14a17480a0cf609f3ba50388fad1ce72013ce44eaf23023d562ea226e63c02`.
This is `CAPABILITY_ONLY`; it does not change RamShared's NBD product route.

## arm64 build evidence

The candidate arm64 build used `aarch64-linux-gnu-gcc` 13.3.0 in an isolated
container and produced:

| Metric | Candidate |
| --- | ---: |
| `Image` bytes | 39,475,712 |
| Loadable modules | 1,972 |
| Candidate-only `W=1` diagnostics | 0 |

The candidate `Image` SHA-256 is
`4a7cdecb4e49cf11ab2daefde7fb7c4e2e2d90fed0c8ea51276438e455b7d494`.
The candidate arm64 zram module is 926,880 bytes with SHA-256
`fe99e253d111b76e350f762f93a5491a939248f2083b6dff212173d237ac1aa9`.
The new arm64 ublk module is 1,056,752 bytes with SHA-256
`0c3b5c205e7e7dc44a6663f46102003f01cd761bc874111c68a098af341ae84e`.

## Sparse static analysis and packaging evidence

- **Sparse `C=2`:** Checked `drivers/block/ublk_drv.c`, `drivers/block/zram/zcomp.c`,
  `drivers/block/zram/zram_drv.c`, `drivers/block/zram/backend_lzorle.c`, and
  `drivers/block/zram/backend_lzo.c`. Zero new sparse warnings or errors detected.
- **Isolated `modules_install`:** Executed against clean `INSTALL_MOD_PATH` for both
  x86 and arm64 architectures. The modules tree installed cleanly with `depmod`
  resolving all symbols.

No custom kernel was booted on the Windows host, no host swap or pressure was
created, and no WSL shutdown/reboot was performed by this campaign. Builds
were serialized and run with low CPU/I/O priority on the shared daily host.

## Named gate status

| Test | Status |
| --- | --- |
| `UPSTREAM_SOURCE_SHA_REVALIDATION` | PASS |
| `UPSTREAM_CANONICAL_ARCH_PATHS` | PASS |
| `UPSTREAM_PATCH_SERIES_SCOPE` | PASS |
| `UPSTREAM_PATCH_DCO_AND_CHECKPATCH` | PASS |
| `X86_CONFIG_PAIR` | PASS |
| `ARM64_INDEPENDENT_PAIR` | PASS |
| `UPSTREAM_CONFIG_OLDDEFCONFIG_X86` | PASS |
| `UPSTREAM_CONFIG_OLDDEFCONFIG_ARM64` | PASS |
| `UPSTREAM_CONFIG_DERIVED_DEFAULTS` | PASS |
| `UPSTREAM_CONFIG_BUILD_W1` | PASS |
| `UPSTREAM_CONFIG_SPARSE_C1` | PASS |
| `UPSTREAM_CONFIG_MODULE_PACKAGE` | PASS |
| `UPSTREAM_CONFIG_UBLK_CAPABILITY_NO_PRODUCT` | PASS / CAPABILITY_ONLY |
| `UPSTREAM_CONFIG_WRITEBACK_CAPABILITY` | PASS / CAPABILITY_ONLY |
| `UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL` | PASS |
| `UPSTREAM_PR_ROUTE_AUDIT` | PASS |
| `NO_EXTERNAL_KERNEL_PR_REFUSAL` | PASS |
| `MAINTAINER_REQUESTED_PR_GATE` | REFUSED pending maintainer request |
| `N3_SCOPE_REFUSAL` | PASS |

## Rollback and residuals

Rollback trigger: any source drift, unexpected file/symbol change, build or
capability failure, public data leak, product-promotion claim, duplicate issue
comment, or unsolicited Microsoft PR. Stop the public action, mark the packet
`NEEDS_REVALIDATION` or `REFUSED`, and retain NBD as the product path.

The only outbound actions after all mandatory gates pass are the tested fork
branch, the RamShared evidence PR, and one concise update to #41054. Microsoft
adoption, release inclusion, and the direct PR route remain external.
