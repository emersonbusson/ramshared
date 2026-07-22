# Changelog

## [0.7.1](https://github.com/emersonbusson/ramshared/compare/v0.7.0...v0.7.1) (2026-07-22)


### Documentation

* **readme:** align v0.7 support and safety status ([75e47a1](https://github.com/emersonbusson/ramshared/commit/75e47a1f0e961766aba7ca30239af9d27e87be11))
* **readme:** align v0.7 support and safety status ([9cc7833](https://github.com/emersonbusson/ramshared/commit/9cc7833bf175ff47dd9302f21c1082471bfc7aa3))

## [0.7.0](https://github.com/emersonbusson/ramshared/compare/v0.6.4...v0.7.0) (2026-07-22)


### Features

* **cli:** add telemetry diagnosis command ([69f9eba](https://github.com/emersonbusson/ramshared/commit/69f9ebaef88a062d7d61cfed39b7890c8097e649))
* **core:** add generic workload lease bridge ([abfaf9f](https://github.com/emersonbusson/ramshared/commit/abfaf9fd58d0e06c96370340e79571d951a27d9a))
* **p0:** add generic GPU workload gate ([ff9030e](https://github.com/emersonbusson/ramshared/commit/ff9030e927c2aa0428ccc6dc39977207c4de02a6))
* **package:** add Linux bundle builder ([a6ae2ea](https://github.com/emersonbusson/ramshared/commit/a6ae2ea04c76f3df32d945e1fa22c19619ae9585))


### Bug Fixes

* **ci:** allow explicit security redactions ([c0225a4](https://github.com/emersonbusson/ramshared/commit/c0225a41a79b8f7a45c673e4d3a6fcb3eddaba0c))
* **ci:** restore required docs check name ([456d9aa](https://github.com/emersonbusson/ramshared/commit/456d9aa35b73a99b940c999550bcee730aa7eeec))
* **reclaim:** harden supervised split campaigns ([02850b2](https://github.com/emersonbusson/ramshared/commit/02850b25badb7e4bd2efdbc1a91a4e63d7e02f87))
* **scripts:** refuse freeze thrash on shared Windows desktop ([99b60fc](https://github.com/emersonbusson/ramshared/commit/99b60fc60471f85344a669da439532d55eeb6a70))
* **windows:** fail preflight on residual vram disks ([7a8b5ef](https://github.com/emersonbusson/ramshared/commit/7a8b5eff0e2a45622877ccbde07df1b37a7e355c))
* **windows:** harden physical disk identity gates ([02109d3](https://github.com/emersonbusson/ramshared/commit/02109d34a96c4f7fe8048ea2f69cc3af6a6877c7))
* **windows:** open control device for preflight ([6f8c9f5](https://github.com/emersonbusson/ramshared/commit/6f8c9f5cb3d7c7f0b46851b213ec3bfe76313d0b))
* **wsl2:** capture daemon telemetry reliably ([2a07bf1](https://github.com/emersonbusson/ramshared/commit/2a07bf149761b0b31d3e871f6ed85ec2d256fe90))
* **wsl2:** demote on global GPU free-floor pressure ([aba2b8e](https://github.com/emersonbusson/ramshared/commit/aba2b8ee805883b01ba05df8ce1555dfeacaede3))
* **wsl2:** demote sparse tier on free-floor pressure ([ab39b38](https://github.com/emersonbusson/ramshared/commit/ab39b389d5bd05a62034f7af4f72029c3b2cdca1))
* **wsl2:** fail fast on missing guest service ([14c731a](https://github.com/emersonbusson/ramshared/commit/14c731aebc7f9cc675d1dac8507a0ab67f2c5853))
* **wsl2:** let free-floor probe preempt latency ([89553f7](https://github.com/emersonbusson/ramshared/commit/89553f7f4800e37b105bf91a91ae836975c03c8f))
* **wsl2:** log low-free probe samples ([55e670f](https://github.com/emersonbusson/ramshared/commit/55e670f97bcbea40b4c0b1c91c76cbac68ff3222))
* **wsl2:** trace probe samples in pressure harness ([da5b6ef](https://github.com/emersonbusson/ramshared/commit/da5b6ef1199e0338602952704702da711a70157e))
* **wsl2:** validate configured campaign rounds ([ad762d8](https://github.com/emersonbusson/ramshared/commit/ad762d8d900486f8e1e00fa617afe20273c29c32))


### Documentation

* **broker:** record isolated P1 drill evidence ([949fecc](https://github.com/emersonbusson/ramshared/commit/949fecc6294eba9f93b70534e4542bf3e798e05c))
* **gpu:** record wddm demote partial audit ([cd033fa](https://github.com/emersonbusson/ramshared/commit/cd033fafab4cfaa1bc1b9dfc04bf5dea894d5a4a))
* **gpu:** record windows pressure matrix pass ([fc62b8e](https://github.com/emersonbusson/ramshared/commit/fc62b8e9d8b07497494321cee472e959178ec84b))
* **lab:** clarify disposable vm boundary ([0ee6ce2](https://github.com/emersonbusson/ramshared/commit/0ee6ce27bd1e6f05365ae5230aa3b2d96f09e0c4))
* **lab:** lock vm inventory policy ([8d4beca](https://github.com/emersonbusson/ramshared/commit/8d4beca3f84770f15f0c304a8c22e3c54c8905b4))
* **lab:** record remaining WSL runtime blocker ([6fc7263](https://github.com/emersonbusson/ramshared/commit/6fc7263ba9120f16e9e56eb27fdd9036f78300aa))
* **lab:** record ublk capability pass ([3c74b35](https://github.com/emersonbusson/ramshared/commit/3c74b35dea3723d539d71f162f84a78a27f122a7))
* **reliability:** use generic workload wording ([8ad6a3f](https://github.com/emersonbusson/ramshared/commit/8ad6a3ff2a210c63be6bcec1b7fc64d995ab5029))
* **security:** redact signing password example ([cfdec09](https://github.com/emersonbusson/ramshared/commit/cfdec0916ab2ae2da5c0a94168fa503bc91a9610))
* **windows:** close supported disk counter gate ([83abbbf](https://github.com/emersonbusson/ramshared/commit/83abbbffac895bf19094f08ce4e992ca4c61b906))
* **wsl2:** capture daily host freeze baseline ([f81f3da](https://github.com/emersonbusson/ramshared/commit/f81f3dacf774e84aed04fcc9e942b761f61365fe))
* **wsl2:** close external GPU pressure gap ([a358acc](https://github.com/emersonbusson/ramshared/commit/a358acc0fe4db0fb149ff90b6087b97b9e659b1d))
* **wsl2:** record clean runtime reinstall attempt ([2bd16d6](https://github.com/emersonbusson/ramshared/commit/2bd16d65f955577a12fe7da72be1a58c0ec99f08))


### CI

* **deps:** bump actions/setup-node from 6 to 7 in the all-actions group ([73b21c5](https://github.com/emersonbusson/ramshared/commit/73b21c5a211b193993ceb905fd4baac9f53244d8))
* run public hygiene in docs workflow ([6cfea54](https://github.com/emersonbusson/ramshared/commit/6cfea54a94bf604b4f47dca1a9f74b95f0fc29a1))

## [0.6.4](https://github.com/emersonbusson/ramshared/compare/v0.6.3...v0.6.4) (2026-07-17)


### Documentation

* **ssdv3:** close security checklist for storport Day-0 path ([0315de0](https://github.com/emersonbusson/ramshared/commit/0315de03fe4af4e0fcb19391b2d7a7927ad3abca))
* **ssdv3:** close security checklist for storport Day-0 path ([b445101](https://github.com/emersonbusson/ramshared/commit/b44510178391346d8d5ad54f5d5a39ca7d61e799))

## [0.6.3](https://github.com/emersonbusson/ramshared/compare/v0.6.2...v0.6.3) (2026-07-17)


### Documentation

* **ssdv3:** close SDV as N/A (DT-30) on VS2022/WDK 26100 ([73a4097](https://github.com/emersonbusson/ramshared/commit/73a4097e9617c047c8ab31b5e9204e2306943075))
* **ssdv3:** close SDV as N/A (DT-30) on VS2022/WDK 26100 ([da6272f](https://github.com/emersonbusson/ramshared/commit/da6272f4edb64ae68261d4b11bb036d3ed2b9c84))
* **windows:** record SDV retirement on modern WDK ([ab65e6e](https://github.com/emersonbusson/ramshared/commit/ab65e6e835d7c9201c7950ea509a3de8c38d270e))
* **windows:** record SDV retirement on modern WDK ([8ad6d8d](https://github.com/emersonbusson/ramshared/commit/8ad6d8d71ce13579cf1d0c84d70974cbaafb64c8))

## [0.6.2](https://github.com/emersonbusson/ramshared/compare/v0.6.1...v0.6.2) (2026-07-17)


### Bug Fixes

* **windows:** claim StartIo READ-copy race under Verifier ([4ffd0e9](https://github.com/emersonbusson/ramshared/commit/4ffd0e95fb98fc0d0e140d466525a3616f9dfe29))
* **windows:** claim StartIo READ-copy race under Verifier ([4cc459e](https://github.com/emersonbusson/ramshared/commit/4cc459edbd8dd70db463dd49b984d3e513c61c3d))

## [0.6.1](https://github.com/emersonbusson/ramshared/compare/v0.6.0...v0.6.1) (2026-07-17)


### Bug Fixes

* **windows:** hang-safe StartIo probe; re-prove Verifier ITEM-3 ([dd366bc](https://github.com/emersonbusson/ramshared/commit/dd366bc26cad13ef7b8064d9d22c352cc813acf9))
* **windows:** hang-safe StartIo probe; re-prove Verifier ITEM-3 ([2d3c882](https://github.com/emersonbusson/ramshared/commit/2d3c8825ef9fcd448e544634be0861c809758e18))

## [0.6.0](https://github.com/emersonbusson/ramshared/compare/v0.5.0...v0.6.0) (2026-07-17)


### Features

* **windows:** StartIo READ-race harness + WSL2 freeze scaffold ([5a94f00](https://github.com/emersonbusson/ramshared/commit/5a94f00886a0ddc310d1f8521f600419767c015c))
* **windows:** StartIo READ-race harness + WSL2 freeze scaffold ([cfbdf92](https://github.com/emersonbusson/ramshared/commit/cfbdf92f26b96fbbcb908496f033c6bdcd6f6609))

## [0.5.0](https://github.com/emersonbusson/ramshared/compare/v0.4.3...v0.5.0) (2026-07-17)


### Features

* **scripts:** guest product Online harness; 64MiB lab PARTIAL proof ([2f58477](https://github.com/emersonbusson/ramshared/commit/2f5847754a752d4520d9a06e6a19dbe33706bdbb))
* **winsvc:** graceful stop, Gate A volume filter, guest IOCTL evidence ([440460a](https://github.com/emersonbusson/ramshared/commit/440460ae4a1c422c75904b7c135c0322b71ed407))
* **winsvc:** implement storport CUDA VRAM product path (SSDV3 partial) ([b3801ac](https://github.com/emersonbusson/ramshared/commit/b3801acc931e016519fd8f104962401ff40c73dd))
* **winsvc:** product Online CUDA path + live 3-round proof ([1817409](https://github.com/emersonbusson/ramshared/commit/181740918e289c29e3fffd6faf3aef1c5ab3e679))
* **winsvc:** Windows driver/host adapters + live CUDA probe ([b5384cc](https://github.com/emersonbusson/ramshared/commit/b5384cce7006b021d820a7eb75d80cf9b9968274))


### Bug Fixes

* **docs:** normalize validation.md schema for CI gate ([68d9cc4](https://github.com/emersonbusson/ramshared/commit/68d9cc4b06105aab04dd02e50156ba65727998a0))
* **mm:** empty LUN before CREATE for exact VPD identity ([1852b51](https://github.com/emersonbusson/ramshared/commit/1852b51af23e9fddf8cf855820a3f99210326950))
* **windows:** close isolated GPU-PV storage gate ([f865c94](https://github.com/emersonbusson/ramshared/commit/f865c94c3d2e19d527e8fdfe9a7cfcf3746ede1e))
* **windows:** close isolated GPU-PV storage gate ([4e4b3c8](https://github.com/emersonbusson/ramshared/commit/4e4b3c8c6faf57e7d91ad933142e95c2d0821bad))
* **winsvc:** fail-closed teardown identity and product Online gates ([e8de695](https://github.com/emersonbusson/ramshared/commit/e8de695e4a16448bbccf0941401e7f6df858128c))
* **winsvc:** FSCTL dismount teardown; careful lab drive letters ([acba7c6](https://github.com/emersonbusson/ramshared/commit/acba7c6c8a9543680fcb5801005e6910954052ae))
* **winsvc:** graceful stop via I/O-pump volume lock ([661ed17](https://github.com/emersonbusson/ramshared/commit/661ed170a000564c248d9eb648edaca0ad8f8686))


### Documentation

* **mm:** physical Online skip; GPU-PV probe-cuda PASS; InfVerif BusType ([bcb77ed](https://github.com/emersonbusson/ramshared/commit/bcb77ed9061eac91b321086c427758d7d681004f))
* **ssdv3:** align cascade-lifecycle SPEC to platform template ([47b2a24](https://github.com/emersonbusson/ramshared/commit/47b2a2424be76e40135f55962d666d1af81092f6))
* **ssdv3:** record exact VPD Verifier PASS and closeout evidence ([c28a23c](https://github.com/emersonbusson/ramshared/commit/c28a23c68a74db66a8b9f03bce86132811b76cce))
* **ssdv3:** tighten RamShared SSDV3 and Kahneman gates ([06922fb](https://github.com/emersonbusson/ramshared/commit/06922fbb14098358bdf13cf89ac00cb6b4150caa))
* **winsvc:** add product Online SHA single-round log artifact ([4b5f33f](https://github.com/emersonbusson/ramshared/commit/4b5f33fc07aa41dee259e09280042f0f875448a7))
* **winsvc:** commit CUDA probe evidence artifact ([31a9f27](https://github.com/emersonbusson/ramshared/commit/31a9f27bdf43f87b047b1ea5c827c681e5d24db2))
* **winsvc:** record MSVC/probe/guest StorPort live evidence ([13b8aa7](https://github.com/emersonbusson/ramshared/commit/13b8aa74e19c89f63a69ab976f733251d5a02e24))

## [0.4.3](https://github.com/emersonbusson/ramshared/compare/v0.4.2...v0.4.3) (2026-07-14)


### Documentation

* backlog close-out issues [#28](https://github.com/emersonbusson/ramshared/issues/28) [#30](https://github.com/emersonbusson/ramshared/issues/30) [#32](https://github.com/emersonbusson/ramshared/issues/32) (honest gates) ([3a20cff](https://github.com/emersonbusson/ramshared/commit/3a20cffe313b204e4c2e9a6fa47aaf26fb70d201))

## [0.4.2](https://github.com/emersonbusson/ramshared/compare/v0.4.1...v0.4.2) (2026-07-14)


### Bug Fixes

* **scripts:** escape-safe .wslconfig (no invalid \w) ([caebbe9](https://github.com/emersonbusson/ramshared/commit/caebbe9299fcaef260a2207d91f0e84199d6d5a2))
* **scripts:** escape-safe .wslconfig encode/validate/apply ([0b94417](https://github.com/emersonbusson/ramshared/commit/0b944171fb338806da51585cee2edb4b9748527b))


### Documentation

* **faq:** .wslconfig invalid escape + wslconfig-ctl ([0b577c5](https://github.com/emersonbusson/ramshared/commit/0b577c5985cc8358b29c5d78706ad7d8b51bc905))
* WSL 16G host residual + 4G VRAM cascade policy ([b0bd0ff](https://github.com/emersonbusson/ramshared/commit/b0bd0ff1847e2b8ad67368743a0c600685b3244f))
* WSL 16G residual + 4G VRAM cascade host policy ([d182e53](https://github.com/emersonbusson/ramshared/commit/d182e538e6abcbfc6dc5e1d3163c40b9f25b322c))

## [0.4.1](https://github.com/emersonbusson/ramshared/compare/v0.4.0...v0.4.1) (2026-07-14)


### Bug Fixes

* **drm:** StorPort TUR sense + format/measure for TM 100% ([6f99614](https://github.com/emersonbusson/ramshared/commit/6f996141d5ecb907a2781877a961b0ac580f8e21))
* **drm:** StorPort TUR sense + format/measure for TM 100% ([b40e83d](https://github.com/emersonbusson/ramshared/commit/b40e83d5a441f1fe02bdd579ed8521a46d24eb84))


### Documentation

* **validation:** guest signed sys reload + CREATE/FORMAT/MEASURE ([2451a33](https://github.com/emersonbusson/ramshared/commit/2451a33163cda5478f61924775314910bc81beaf))
* **validation:** guest signed sys reload CREATE/FORMAT/MEASURE PASS ([4bd6c12](https://github.com/emersonbusson/ramshared/commit/4bd6c1274dfbaf31be637be89597be42bf697c15))


### CI

* re-trigger PR body check after template fix ([70fe511](https://github.com/emersonbusson/ramshared/commit/70fe511b2edc63b2728cabc832779ae57ab37679))

## [0.4.0](https://github.com/emersonbusson/ramshared/compare/v0.3.1...v0.4.0) (2026-07-14)


### Features

* **cascade:** export demote status for lifecycle JSON ([7821d46](https://github.com/emersonbusson/ramshared/commit/7821d460f9007e04ef75f1d8829429f3d0f248e9))
* **cascade:** lifecycle status phase + demote export ([c33c95a](https://github.com/emersonbusson/ramshared/commit/c33c95a2488597991a9d6e4b4b2f913f1e1faeaf))
* **cascade:** lifecycle status phase and status --json ([39a9665](https://github.com/emersonbusson/ramshared/commit/39a9665afeca7474cb2142bb36fd963cd146168d))
* **win-driver:** close [#29](https://github.com/emersonbusson/ramshared/issues/29)/[#40](https://github.com/emersonbusson/ramshared/issues/40) gaps, charts, and live pressure proof ([39dfe72](https://github.com/emersonbusson/ramshared/commit/39dfe7280c0d083a539b23919191337ae9a841a0))


### Bug Fixes

* **ci:** green gates + port security scans from advoq pattern ([7242b2b](https://github.com/emersonbusson/ramshared/commit/7242b2bd674f638912448667876c965873f48042))


### Documentation

* record win11-drill PSD guest lab drill PASS ([2d07fd4](https://github.com/emersonbusson/ramshared/commit/2d07fd4c716a2ae9a56c1ed2a181b3343cab3164))
* **ssdv3:** PRD cascade lifecycle observability (Step 1) ([9a88e7e](https://github.com/emersonbusson/ramshared/commit/9a88e7ed85bc423c7229579fb513815ff2989c55))
* **ssdv3:** SPEC cascade lifecycle observability (Step 2) ([015196f](https://github.com/emersonbusson/ramshared/commit/015196f7c5c6f30e2943aa03d0129c0887af92b1))

## [0.3.1](https://github.com/emersonbusson/ramshared/compare/v0.3.0...v0.3.1) (2026-07-13)


### Bug Fixes

* **win-driver:** secure partition formatting in install script ([dbf064d](https://github.com/emersonbusson/ramshared/commit/dbf064dc62132ff18e231ad9bc75e8d2541f870d))
* **win-driver:** secure partition formatting in install script ([f4c88cb](https://github.com/emersonbusson/ramshared/commit/f4c88cb09a2c7db96203a4e29382994c016a7e15))

## [0.3.0](https://github.com/emersonbusson/ramshared/compare/v0.2.0...v0.3.0) (2026-07-13)


### Features

* **cascade:** hang cover gate, SSDV3 Passo3, superprompt, postmortem ([d89f481](https://github.com/emersonbusson/ramshared/commit/d89f4817916dede1f0ae759d672ceb391caa68c3))


### Bug Fixes

* **ci:** green pre-push gates (fmt, validation schema, comments) ([a866c0b](https://github.com/emersonbusson/ramshared/commit/a866c0bb7704a7ead3b7d45f87155dc964c9e76d))


### Performance

* **win-driver:** add E2E StorPort performance benchmark comparison ([4813cff](https://github.com/emersonbusson/ramshared/commit/4813cff31e7c2c44bc3e129c5e840ef69b54d19f))
* **win-driver:** add E2E StorPort performance benchmark comparison ([fd6c1c4](https://github.com/emersonbusson/ramshared/commit/fd6c1c42ca012816b050bfe06854345e65c0b9ce))


### Documentation

* align boot SPEC conf defaults and record live health ([16160bb](https://github.com/emersonbusson/ramshared/commit/16160bbcb12cc5053cacefab3da7b0d7e824f81a))
* cascade SPEC↔code confrontation + SSDV3 hang gates ([c30f2ca](https://github.com/emersonbusson/ramshared/commit/c30f2caa2997e55e7251b90a6463c46cffb4ab93))
* confront cascade SPECs with code and named tests ([271ebf1](https://github.com/emersonbusson/ramshared/commit/271ebf1509f0989f48609abb8eedccb00fcdd7c3))
* confront remaining cascade SPECs with unit evidence ([66f4983](https://github.com/emersonbusson/ramshared/commit/66f4983b8a3f7924ec898b4083fb73eb9e5ddcdb))
* English SSDV3, superprompt, hang audit, validation schema ([1be585e](https://github.com/emersonbusson/ramshared/commit/1be585ed700dccc6ece825e25ccc48b1650099c4))
* **readme:** add WSL2 guest benchmarks and coexistence policy ([d26f297](https://github.com/emersonbusson/ramshared/commit/d26f297e1130ef29e7bbcd83a63e874a5d3875fc))
* **readme:** add WSL2 guest benchmarks and coexistence policy ([0f242a3](https://github.com/emersonbusson/ramshared/commit/0f242a3456ce60efd5e5734a57b77c327d980202))
* record PR 33 merge and post-merge cascade health ([30d842e](https://github.com/emersonbusson/ramshared/commit/30d842e37ded83146adf790e1812a1cd58e63cf0))
* scope agent docs to ramshared only ([0fe4d7b](https://github.com/emersonbusson/ramshared/commit/0fe4d7b63a824db1f58f39cbae60a401d20cf7df))
* **ssdv3:** require SPEC↔code confrontation for long-lived SPECs ([faf6b59](https://github.com/emersonbusson/ramshared/commit/faf6b59334ce966f05e6ae8aad87ee25c7056f8f))
* **ssdv3:** use Step N wording in English rules ([102d469](https://github.com/emersonbusson/ramshared/commit/102d4691133cc1b3cf63bccc21db822e494bbaf4))
* strip foreign-project residue and dedupe SSDV3 agent docs ([e6751a0](https://github.com/emersonbusson/ramshared/commit/e6751a0fcaeaaabf87dfeba04897b471e5f971d8))
* tighten SSDV3 prompts and hang superprompt for agents ([a4ab2ac](https://github.com/emersonbusson/ramshared/commit/a4ab2ac27d84c25e5a98c89796680a5b2f3f0ff1))
* validation entry after PR 33 merge ([5d14d81](https://github.com/emersonbusson/ramshared/commit/5d14d81bd45468fdda2285554601bb812899cae4))

## [0.2.0](https://github.com/emersonbusson/ramshared/compare/v0.1.0...v0.2.0) (2026-07-12)


### Features

* **cli:** check reporta tier zram + linha Tiers da cascata (§6.1) ([14a11fd](https://github.com/emersonbusson/ramshared/commit/14a11fdb95e30a645b8d82603ed1ca5887842619))
* **core:** add --advertise-nbd for cross-host broker (ITEM-12 prep) ([02faf6b](https://github.com/emersonbusson/ramshared/commit/02faf6b4f3c5c40d0104d6b79c2b89b4210f96f1))
* **core:** add --backend ram for broker mode (qemu-drillable) ([4b14070](https://github.com/emersonbusson/ramshared/commit/4b1407055d24181ec05c7a822568634da747915d))
* **core:** add NBD server-side handshake (ramshared-block) ([b289dba](https://github.com/emersonbusson/ramshared/commit/b289dba0e9143e917ff536501372827e28bc17af))
* **core:** add ramshared-agent tenant (psi/swap/watchdog, DT-27) ([cd15ba4](https://github.com/emersonbusson/ramshared/commit/cd15ba4afa4c78755b5cdd37330bcd948583ee0b))
* **core:** add ramshared-broker crate (protocol, model, slices, arbiter) ([49d37fc](https://github.com/emersonbusson/ramshared/commit/49d37fc3ae7a14a4abad54e1c7a973630635ee60))
* **core:** add ramshared-tier swap cascade (priorities + DEMOTE safety-net) ([c1cbc5f](https://github.com/emersonbusson/ramshared/commit/c1cbc5f36f099822034c2410f2072239acac7522))
* **core:** add ramshared-vram crate (VramProvider/VramMemory traits, RF-G1) ([d898488](https://github.com/emersonbusson/ramshared/commit/d898488663527e8cf430c1e53936c1f9cea02d48))
* **core:** block checksum + test patterns (ramshared-integrity) ([bbcaa93](https://github.com/emersonbusson/ramshared/commit/bbcaa9390d6daf246dce41690314329aae22a002))
* **core:** CLI up/down/status -- monta a cascata zram-&gt;VRAM-&gt;VHDX ([36ac879](https://github.com/emersonbusson/ramshared/commit/36ac879221d09a8d1a448a1e8bd4a5097412ec80))
* **core:** coletor de telemetria & reconciliação do broker (RF-1..5) ([ef792b1](https://github.com/emersonbusson/ramshared/commit/ef792b1d3212cae14e68f05c4190b44b669c72b5))
* **core:** daemon mlockall + oom_score_adj (anti-deadlock, Disciplina 3) ([1ac9eec](https://github.com/emersonbusson/ramshared/commit/1ac9eec4da28f7086b0682ac271c20fee98cab3f))
* **core:** daemon multi-conn (H1) + hardening + typed errors + Fase B specs ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([f1b6675](https://github.com/emersonbusson/ramshared/commit/f1b667577d3e1df34bc6d0f0b9194c0be2d77a4d))
* **core:** exponential reconnect backoff in the agent ([7787fc2](https://github.com/emersonbusson/ramshared/commit/7787fc2d1a06ff649f8154f0412dd99862f447c1))
* **core:** impl VramProvider/VramMemory for CUDA types (RF-G1) ([74f4052](https://github.com/emersonbusson/ramshared/commit/74f4052802aff95b837ed5e7da950d1086244613))
* **core:** impl VramProvider/VramMemory no backend Vulkan (RF-V2/RF-V3) ([d15bd82](https://github.com/emersonbusson/ramshared/commit/d15bd82634c95178c996e318be00536161476944))
* **core:** NBD multi-connection daemon — reader/worker/writer ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([db49a6e](https://github.com/emersonbusson/ramshared/commit/db49a6e697ab7abfae91ae10648b8af7c799c866))
* **core:** NBD server daemon loop (ramshared-wsl2d serve /dev/nbdX) ([7f0aecc](https://github.com/emersonbusson/ramshared/commit/7f0aecc4ad42b4e2208404f6ecb8f5338f05fe42))
* **core:** P1 broker data-plane + core in wsl2d (RF-B1..B4, RF-L1..L2) ([e3518b4](https://github.com/emersonbusson/ramshared/commit/e3518b4840f264fca50955289732436e55ee7b18))
* **core:** P1 hardening — VramProvider (RF-G1), Vulkan backend (RF-G2), broker telemetry & supply-chain gate ([e939f63](https://github.com/emersonbusson/ramshared/commit/e939f635ca9e5ab7a7bc9df8dfa0cafc3a35ae32))
* **core:** port CUDA driver layer to Rust (ramshared-cuda) ([15c7792](https://github.com/emersonbusson/ramshared/commit/15c7792ce64856ee4d9cabb8d785b037781430fc))
* **core:** port NBD protocol + I/O model to Rust (ramshared-block) ([87a1162](https://github.com/emersonbusson/ramshared/commit/87a11623b27b86e90af7f4e9b6b6f437737ead1a))
* **core:** ramshared-vulkan — open + enumeração (RF-V1, validado em lavapipe) ([b19d8a3](https://github.com/emersonbusson/ramshared/commit/b19d8a3df43f3694b28b5dcaab1b8e1b8d07e87d))
* **core:** residency canary decision logic (ramshared-wsl2d §9) ([d273b45](https://github.com/emersonbusson/ramshared/commit/d273b4537d9e44340a7c7a8f84f26c452f1fab94))
* **core:** shell --backend vulkan no daemon (RF-V4) ([afad4d5](https://github.com/emersonbusson/ramshared/commit/afad4d532364a191d80384ba2cc020f7c42eb35a))
* **core:** VRAM-as-swap cascade for WSL2 (SPECv3 Rust port, §14 validated) ([4aed9e2](https://github.com/emersonbusson/ramshared/commit/4aed9e28d168f774a27954b3aa116a007467d21c))
* **core:** wire §9 canary + DEMOTE into daemon serve loop ([dc1485e](https://github.com/emersonbusson/ramshared/commit/dc1485ef0c4321c84561e4e4ab5f151a4825b5f4))
* **core:** wire broker into the daemon runtime (run_broker, ITEM-8) ([b0bae97](https://github.com/emersonbusson/ramshared/commit/b0bae979553f75a180fc25c329e0677800f70c3a))
* **core:** wire CUDA&lt;-&gt;NBD (VramBackend) + daemon state machine ([726aac8](https://github.com/emersonbusson/ramshared/commit/726aac8d0436c0dd3e20541069c9f2500b777669))
* **cuda:** implement cross-platform CUDA dynamic loader (ITEM-1) ([45dedb9](https://github.com/emersonbusson/ramshared/commit/45dedb9d9972e5961010a4ba5f63a5714432890a))
* **mm:** add dxg budget provider and autotier policy ([e0565a5](https://github.com/emersonbusson/ramshared/commit/e0565a557ff2a33122e6632257d9120a1165315e))
* **mm:** add StorPort virtual miniport sources (ITEM-5) ([f149541](https://github.com/emersonbusson/ramshared/commit/f14954106460d7b77ee5f7a856ed90ad29cd66fe))
* **mm:** add TransportKind::WinDrive lease-only path (ITEM-3) ([ae9cc44](https://github.com/emersonbusson/ramshared/commit/ae9cc44c86529d5fa8b8613e9d254d6fc7ef5d11))
* **mm:** auto-recover zero-used cascade orphans after WSL terminate ([688504c](https://github.com/emersonbusson/ramshared/commit/688504c76153b3e7ad16f0b5007d7b2fbc154848))
* **mm:** build and load StorPort + poolstress on win11-drill ([ad7701b](https://github.com/emersonbusson/ramshared/commit/ad7701ba9b5bc6db375c23ea807244023050043f))
* **mm:** cascade transport auto→NBD on WSL2; boot unit policy ([e0b8a33](https://github.com/emersonbusson/ramshared/commit/e0b8a33cc3446948a1cf872364ab8795085c3f96))
* **mm:** dedicated residency canary (§9.4) with hysteresis sampler ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([8fc1a27](https://github.com/emersonbusson/ramshared/commit/8fc1a27f9f98b43558d97f4f0fa4e917b2ed8c26))
* **mm:** dedicated residency canary (§9.4) with hysteresis sampler ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([e808daa](https://github.com/emersonbusson/ramshared/commit/e808daa7970fd35f77875281621cbe6dd6fc891f))
* **mm:** DT-9 fail-closed teardown and ordered-kill lab ([ce2720e](https://github.com/emersonbusson/ramshared/commit/ce2720efa858901d2fa69acba6e37868719a6c10))
* **mm:** gate sparse commits with WDDM budget ([1e96e43](https://github.com/emersonbusson/ramshared/commit/1e96e4344655d17f4b939cdb3b637c923c6c9a73))
* **mm:** implement ramshared-winsvc pure logic (ITEM-3/6/7) ([2145401](https://github.com/emersonbusson/ramshared/commit/2145401c447e0e379cd92a5da72501d4b1c6a9e8))
* **mm:** lab SCM RamSharedWinSvc with delayed auto-start ([65861ce](https://github.com/emersonbusson/ramshared/commit/65861cefae89037eb8daf8fb339e00d4ae203ea2))
* **mm:** opt-in WSL cascade boot + human-facing docs ([8495155](https://github.com/emersonbusson/ramshared/commit/849515518f13616122855651ebd46460229cf3da))
* **mm:** poll WDDM and recover empty tier ([f17fb33](https://github.com/emersonbusson/ramshared/commit/f17fb33f32e0fa93c6653a30520831ef99a2ee43))
* **mm:** promote VramBackend into ramshared-block (ITEM-2) ([28a7960](https://github.com/emersonbusson/ramshared/commit/28a7960828abe30d47c9c733c9a3bed754dd2401))
* **mm:** sparse on-demand VRAM for NBD cascade (cascade-vram-ondemand) ([f18979b](https://github.com/emersonbusson/ramshared/commit/f18979b982f035c78d5092fc2c8f2baedb8dd16b))
* **mm:** VRAM free-floor + commit_cap; product capacity 4GiB ([4d9d76e](https://github.com/emersonbusson/ramshared/commit/4d9d76e24bb41d7111e79578df2b3f966ebbf06d))
* **mm:** wire CREATE_DISK to active LUN and add VM install helpers ([8a3f1c7](https://github.com/emersonbusson/ramshared/commit/8a3f1c74314a7e41cac58a09cce6489ab158c5bb))
* **scripts:** add P0 measurement harness for the memory broker gate ([54fc596](https://github.com/emersonbusson/ramshared/commit/54fc59616760232c5bb3fd28f017545dd94d15ba))
* **scripts:** cascade control app + kernel track inventory ([7e6ee30](https://github.com/emersonbusson/ramshared/commit/7e6ee30013a38c58202d33f0852d8842de5ce586))
* **scripts:** continuous cascade health JSONL sampler ([f586d19](https://github.com/emersonbusson/ramshared/commit/f586d19fb7f12edce3656bc9e0523d7ba8c040a1))
* **scripts:** custom WSL2 kernel CLI, lab docs, and P1 SSDV3 pack ([4afc719](https://github.com/emersonbusson/ramshared/commit/4afc719e7c0255d77f288c10e953c6b62814a3b6))
* **scripts:** harden generic GPU workload VRAM probe for the P0/P2 gate ([6a20727](https://github.com/emersonbusson/ramshared/commit/6a2072751d0f995444a419f06b89ad2ece51444f))
* **scripts:** harden generic GPU workload VRAM probe for the P0/P2 gate ([8c5de79](https://github.com/emersonbusson/ramshared/commit/8c5de79771a0d651c9fabd99d2a8a6571f03a1ed))
* **scripts:** Hyper-V Linux lab on R: + DDA inventory + mainline PRD ([eb92787](https://github.com/emersonbusson/ramshared/commit/eb9278728d4e87d327f5df307710d3573c70dacf))
* **scripts:** kernel toolkit — build + QEMU isolated validate + self-healing boot ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([38378b2](https://github.com/emersonbusson/ramshared/commit/38378b214d8291903d4ea9bf830050b819b13c83))
* **scripts:** lab Start/Stop with DT-9 reboot-kill proof ([aaf4e49](https://github.com/emersonbusson/ramshared/commit/aaf4e49d3a5f0e831e58bb32205b885f4930160e))
* **scripts:** sistema de caixa-preta forense + preflight fail-safe ([e324a10](https://github.com/emersonbusson/ramshared/commit/e324a1061077ebc850482e67b8e68a6c3e68af84))
* **scripts:** Windows kernel-page and soak harnesses (ITEM-8..11) ([d2f87f5](https://github.com/emersonbusson/ramshared/commit/d2f87f58d97f2fca713ced013c4e69268f7e0f8a))
* **scripts:** WSL elevated Hyper-V/PowerShell helper via Windows sudo ([cc2ec0d](https://github.com/emersonbusson/ramshared/commit/cc2ec0d8e4638fde005b2665701da3be6f16e6f2))
* **wsl2d:** add --backend ram for gpu-free ublk daemon (qemu) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([a2a4af2](https://github.com/emersonbusson/ramshared/commit/a2a4af2af2cdfb85de101f87fe5d793531d890a5))
* **wsl2d:** DT-3 ublk worker with residency canary ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([31f8395](https://github.com/emersonbusson/ramshared/commit/31f83955a203b24984f5c831c2ca40532341a556))
* **wsl2d:** support multi-page ublk requests via max_sectors ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([3cf690e](https://github.com/emersonbusson/ramshared/commit/3cf690e95fcab4b21ec8ca0975dc7a457cc527af))
* **wsl2d:** ublk daemon mode (F2) gated off on WSL2 after freeze ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([2471039](https://github.com/emersonbusson/ramshared/commit/2471039c6be2f9312b3dfa971d5c0a289916d260))


### Bug Fixes

* **ci:** check PR author instead of actor in pr-body gate ([14431f9](https://github.com/emersonbusson/ramshared/commit/14431f9f04443c5b93f55d270637b49c1a77c655))
* **ci:** check PR author instead of actor in pr-body gate ([71df1e5](https://github.com/emersonbusson/ramshared/commit/71df1e55cc1e6b0fff073efb114e9f626283d7b7))
* **cli:** add generic swap device and ublk transport gate ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([4fd0ad7](https://github.com/emersonbusson/ramshared/commit/4fd0ad709090e836ca053afeecd87c392746f21f))
* **cli:** harden cascade A1 oracle + teardown (review) ([c108b1a](https://github.com/emersonbusson/ramshared/commit/c108b1a7fc6461c402259d6d294919f6d5dd5205))
* **cli:** refuse ghost ublk/nbd and swapoff-first on cascade down ([57f3d0b](https://github.com/emersonbusson/ramshared/commit/57f3d0b9867cfdf5239c475bf41ea3f6f3c0b77a))
* **cli:** require io_uring runtime for ublk check ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d530959](https://github.com/emersonbusson/ramshared/commit/d5309594a4d14161148a1fb19c0f33c904c8cf6b))
* **cli:** require ublk control access in check ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([78738c1](https://github.com/emersonbusson/ramshared/commit/78738c191352e80de46a430b43cc7714dafe6df8))
* **core:** absolute swapoff path + close daemon hardening findings ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([6969645](https://github.com/emersonbusson/ramshared/commit/6969645aa0ba4701fe309599a4e63acf94a198ef))
* **core:** align broker tick to SPEC (2s, was 1s) — audit finding ([b7bb557](https://github.com/emersonbusson/ramshared/commit/b7bb557117830768c5f022c22efbe8b07280c6f6))
* **core:** broker arbiter tick must not be starved by Psi (ITEM-12) ([32d2911](https://github.com/emersonbusson/ramshared/commit/32d2911b118dff4fd10f30fddab6e5e7e9422840))
* **core:** complete ramsharedd rename in CLI + test (DT-5) ([f3a6dff](https://github.com/emersonbusson/ramshared/commit/f3a6dff3e3706a0643be72717e154d7ec6576dfe))
* **core:** eviction bypassa histerese + teto de --slices + reconcile alloc=0 ([e12efcf](https://github.com/emersonbusson/ramshared/commit/e12efcfcc346e95ef3c222fc63472181762a9e6e))
* **core:** harden wsl2d daemon (DEMOTE failure, overflow, mlock honesty, alloc cap) ([0aa5948](https://github.com/emersonbusson/ramshared/commit/0aa5948728b5f69ade3a2916f514c855973b1442))
* **core:** recalibrate residency latency canary 8x-&gt;64x (DT-31) ([bbf76ec](https://github.com/emersonbusson/ramshared/commit/bbf76eca81f00744a955de18ad649eabd514d101))
* **core:** retry stuck-Draining slice zero on tick (R4) ([79f9ce6](https://github.com/emersonbusson/ramshared/commit/79f9ce64233a970594fa3a279c5aa6dc243a90ab))
* **mm:** complete inflight SRBs with real AdapterExt on B2 teardown ([0d7b093](https://github.com/emersonbusson/ramshared/commit/0d7b0938881798100319fc22d2150d0a8f9d0fae))
* **mm:** evita kernel BUG por colisao mlockall+dxgkrnl no ublk+vram ([9f5a6ca](https://github.com/emersonbusson/ramshared/commit/9f5a6caf398dbebc0a0a4dc7959f18f49811f732))
* **mm:** harden B2 teardown; document 0x7A on pagefile-hot kill ([3a4d1d9](https://github.com/emersonbusson/ramshared/commit/3a4d1d915bc70911e436475f1effd9c7c28c75fc))
* **mm:** keep swap writes lossless under WDDM pressure ([30ffb61](https://github.com/emersonbusson/ramshared/commit/30ffb612de3676c5b51491ad8b8efcbef962ab22))
* **mm:** raise WinDriveBackend maxIo to 1MiB for NTFS format ([d4fe1df](https://github.com/emersonbusson/ramshared/commit/d4fe1dfe874a2665b2db7ded8f2cb771aad44125))
* **mm:** scrub canary region even if backend zero fails ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([1beb0c7](https://github.com/emersonbusson/ramshared/commit/1beb0c7a905b496d35408acd65911229cb7fea0f))
* **mm:** StorPort I/O via system address and INF install path ([d564f41](https://github.com/emersonbusson/ramshared/commit/d564f41bc76b85bc1dbf220095f375e1b4c520b7))
* review hardening batch 2 — M2/M4/M5 + name-buffer ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([c121ae9](https://github.com/emersonbusson/ramshared/commit/c121ae9c67e4b6c174e6f8a2ba672201516dc81b))
* review hardening batch 2 (M2/M4/M5 + name-buffer) ([2bffc8f](https://github.com/emersonbusson/ramshared/commit/2bffc8fe5b8f1c7518d7b9992103bf361b17ab61))
* **scripts:** add WSL kernel preflight mode ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d3a99e0](https://github.com/emersonbusson/ramshared/commit/d3a99e0f85e7152e574e26342ec520203e6ccd85))
* **scripts:** harden lab VMs against disk fill and host C: pressure ([7ee3ddf](https://github.com/emersonbusson/ramshared/commit/7ee3ddf05b85a65f598f8fec3be1d5b110eade78))
* **scripts:** launcher fail-safe — disarm determinístico + retry + finally que não escapa ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([9fc50da](https://github.com/emersonbusson/ramshared/commit/9fc50dad8dd02751c9444fde009d518e66ad069e))
* **scripts:** make Windows harnesses parse on PS 5.1 guest ([58c6986](https://github.com/emersonbusson/ramshared/commit/58c698673f21b38a1e564337ee19fc941926c60f))
* **scripts:** move win11-drill off C: and protect system disk ([5f3bfdc](https://github.com/emersonbusson/ramshared/commit/5f3bfdccb47225e7d7cce43fa2d27cfe2b324f01))
* **scripts:** persist WSL kernel boot logs ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([f5691f1](https://github.com/emersonbusson/ramshared/commit/f5691f11102b0088fa3f89bc530d79fe9ef10ce8))
* **windows:** resolve remote driver load and backend loop validation ([14b7900](https://github.com/emersonbusson/ramshared/commit/14b79003007d16732a8efb7c8015c976b73594a1))
* **wsl2d:** add io_uring smoke gate ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([a52a2bb](https://github.com/emersonbusson/ramshared/commit/a52a2bb879470118024967787622b7f2250da0de))
* **wsl2d:** add pure ublk work bridge ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([8a473aa](https://github.com/emersonbusson/ramshared/commit/8a473aa2ea5ee606bd79da89c3254dffba560f97))
* **wsl2d:** add safe ublk uapi helpers ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([782d3da](https://github.com/emersonbusson/ramshared/commit/782d3da5e7f89c399a59e6ea186d70ea59069e07))
* **wsl2d:** add ublk add delete smoke ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([1bae47f](https://github.com/emersonbusson/ramshared/commit/1bae47f245ce97870c071ad3499cb3fc9b9de56f))
* **wsl2d:** add ublk control features smoke ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([8680bac](https://github.com/emersonbusson/ramshared/commit/8680bacab6dd748c0226295ab3039ebdfb230cd6))
* **wsl2d:** add ublk dt-3 ring owner loop ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([ffc542f](https://github.com/emersonbusson/ramshared/commit/ffc542f9713c35d89ce3ed08c21e3f608a0a8c7a))
* **wsl2d:** add ublk dt-3 worker over channels ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([9575d8f](https://github.com/emersonbusson/ramshared/commit/9575d8fb1eff0feed45bbfca30c45d0a1ef2fca0))
* **wsl2d:** add ublk ram backend and serve logic ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([dd58baa](https://github.com/emersonbusson/ramshared/commit/dd58baa2f6d1ac55317549633f8744a31c128161))
* **wsl2d:** add ublk set/get params control ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([883de60](https://github.com/emersonbusson/ramshared/commit/883de607b2dc3b92f8041f2598be228b7f1b52b4))
* **wsl2d:** encode ublk fetch io cmd ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([237aff9](https://github.com/emersonbusson/ramshared/commit/237aff97b5213c12a324098f8b36cd18069d15ef))
* **wsl2d:** map ublk io descriptors to block requests ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([0ff3a24](https://github.com/emersonbusson/ramshared/commit/0ff3a244fac8c6b8d154f53b3a871e158b683729))
* **wsl2d:** mmap ublk io-desc buffer read-only ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([c6e2890](https://github.com/emersonbusson/ramshared/commit/c6e28902381bcf6ca55d22657ea01ec8489cff18))
* **wsl2d:** return backend from ublk server loop ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([698604b](https://github.com/emersonbusson/ramshared/commit/698604b59f586292ca593a9f484d6a98aff109ba))
* **wsl2d:** serve ublk block io with ram backend ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([79065f6](https://github.com/emersonbusson/ramshared/commit/79065f6b8f2b67062c1ad822649f011ebafd08d9))
* **wsl2d:** serve ublk from vram via dt-3 worker ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([ac915f7](https://github.com/emersonbusson/ramshared/commit/ac915f7e0d798f3759771e88b30d6e2993d7ccab))
* **wsl2d:** submit ublk FETCH_REQ without waiting ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([8a325a3](https://github.com/emersonbusson/ramshared/commit/8a325a376adb106f769b4077babbf5cfb1ac9ed4))


### Performance

* **wsl2d:** block instead of poll in ublk dt-3 ring owner ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([b5032aa](https://github.com/emersonbusson/ramshared/commit/b5032aa295c6f83ee07033e41c55da527a115384))
* **wsl2d:** recycle ublk dt-3 buffers to drop hot-path alloc ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([aa2f060](https://github.com/emersonbusson/ramshared/commit/aa2f060ed8093e911df19bdf7d7769f86de5e71a))


### Refactor

* **cli:** drop duplicated CUDA FFI, use ramshared-cuda crate (C3) ([dfdc4a6](https://github.com/emersonbusson/ramshared/commit/dfdc4a642c5785cb82edfc836fbacc7cc19403a3))
* **cli:** use ramshared-cuda crate, drop duplicated CUDA FFI ([#3](https://github.com/emersonbusson/ramshared/issues/3) C3) ([674f6a8](https://github.com/emersonbusson/ramshared/commit/674f6a852c3f4e4c32550d29172485ed6c969c84))
* **core:** generic serve_ublk_residency over VramMemory (RF-G1) ([ba6b411](https://github.com/emersonbusson/ramshared/commit/ba6b4111fb727e651c1414f09e20c47e31912bef))
* **core:** generic VramBackend/CanaryProbe over VramMemory (RF-G1) ([ca5194d](https://github.com/emersonbusson/ramshared/commit/ca5194d67d9118a22fe3925aa49278d2c5134215))
* **core:** run_nbd + run_broker generic over VramProvider (RF-G1) ([c65e0de](https://github.com/emersonbusson/ramshared/commit/c65e0de81ed675c78c16e1ffb6eaaf692de40130))
* **core:** typed CascadeError; document clap-not-used (zero-dep) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([b3d97ba](https://github.com/emersonbusson/ramshared/commit/b3d97ba7aca147ea8c68dd166df678b9a4cbd790))
* **wsl2d:** isolate io_uring behind wrapper crate ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d15aa32](https://github.com/emersonbusson/ramshared/commit/d15aa321fff167d584dd1124f2181e3b0faf03de))
* **wsl2d:** reuse BlockBackend trait in ublk serve ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([baf2203](https://github.com/emersonbusson/ramshared/commit/baf22035b69e90e8c645e02f0d36fd7fa1365588))


### Documentation

* accept gated io-uring dependency for ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([8255d6b](https://github.com/emersonbusson/ramshared/commit/8255d6b749e98a4bbdb68ed9d89f02e50de2462a))
* add docs/decisions (ADR-0001/0002) + LIBRARIES.md, close vapor anchors ([c9208cd](https://github.com/emersonbusson/ramshared/commit/c9208cd42ffbd7bf696a5a213b3969c2b85a7b03))
* add PR template (.github/pull_request_template.md) per governance ([1ac3cb8](https://github.com/emersonbusson/ramshared/commit/1ac3cb808f8d6073173659acdae0bc214712f851))
* add README, ARCHITECTURE, ROADMAP (root docs, PT-BR) ([8233636](https://github.com/emersonbusson/ramshared/commit/823363603b3162b4d1ae910586473b946abda1bd))
* add REVIEW-ADR runbook (revisao periodica anti-cargo-cult) ([7549f6f](https://github.com/emersonbusson/ramshared/commit/7549f6f89ac8d3ed6e4cdf045995f6116bd00038))
* **core:** add kernel-adapted superprompt (noise audit + model clarity) ([823515b](https://github.com/emersonbusson/ramshared/commit/823515b6fac17cd97a1379f1a5a22a2ef21a2c62))
* **core:** add postmortem + degradation-matrix anchors (Kahneman [#5](https://github.com/emersonbusson/ramshared/issues/5)/[#7](https://github.com/emersonbusson/ramshared/issues/7)) ([a7121e5](https://github.com/emersonbusson/ramshared/commit/a7121e524f2d76335a2de985762c2bdbb939bf4d))
* **core:** CIVM-TENANT runbook for cross-host e2e (ITEM-12) ([129c177](https://github.com/emersonbusson/ramshared/commit/129c17756a6e712477243c77d888c40bdae3585e))
* **core:** DT-29 server-only safety boundary + status updates ([651360b](https://github.com/emersonbusson/ramshared/commit/651360bbfaa4fc4f4f0c2a33d9ab88cf2198cd46))
* **core:** ITEM-12 Fase B (VRAM cross-host) = PASS ([f134dfa](https://github.com/emersonbusson/ramshared/commit/f134dfa65d75f73ba57914da0fe3e49f2cc8a84d))
* **core:** MEMORY — fechamento do branch p1-hardening (gate supply-chain + PR) ([afdfd23](https://github.com/emersonbusson/ramshared/commit/afdfd23bc74d025948341977be64d92f6f56401e))
* **core:** MEMORY — RF-G2 backend Vulkan completo + validado (lavapipe + drills) ([9b899f2](https://github.com/emersonbusson/ramshared/commit/9b899f2c7276faf87008f56ef86589dbe3357935))
* **core:** MEMORY — SPEC do P2 Windows + auditoria 2.5 (no-go-&gt;SPECv2) ([b21e898](https://github.com/emersonbusson/ramshared/commit/b21e8988b5e0d209ce1ca81848ea14df4c3b1f9d))
* **core:** MEMORY.md — auditoria + validacao + commits do P1 broker ([cdee969](https://github.com/emersonbusson/ramshared/commit/cdee9698663b5ac4f0c832bdc7b4a33c7a2a88f2))
* **core:** MEMORY.md — frentes pós-P1 (F1 feito, F4 SPEC) ([55d0b12](https://github.com/emersonbusson/ramshared/commit/55d0b120ead5ba476d64daf16f0910bec19b435c))
* **core:** MEMORY.md — memory broker P0 + P1 session log ([a50042e](https://github.com/emersonbusson/ramshared/commit/a50042ecdf066800925e219863fffcce76191c05))
* **core:** MEMORY.md — P1 broker ITEM-8/9/11/12 + drill qemu PASS ([243f9ab](https://github.com/emersonbusson/ramshared/commit/243f9abd772abda4935a231e07c951a1fd891170))
* **core:** MEMORY.md — rename ramsharedd + DT-29 + lição de validação ([0144e4b](https://github.com/emersonbusson/ramshared/commit/0144e4b3999544ec8d01ba37387d8704792c3dfc))
* **core:** record ITEM-12 Fase A PASS + DT-30 (arbiter tick) ([6c59098](https://github.com/emersonbusson/ramshared/commit/6c590983673c063f1e2358090898fdd637565b0d))
* **core:** regra de benchmarks + sync CLAUDE/AGENTS/ssdv3 ([be45e16](https://github.com/emersonbusson/ramshared/commit/be45e1652f7dcb808169bbce8ae55111c400c163))
* **core:** ROADMAP — item LOW (Fase A) resolvido (typed errors feito, clap rejeitado) ([39ad8db](https://github.com/emersonbusson/ramshared/commit/39ad8db2d3f5d7697f284b46df41ab3681d4ce55))
* **core:** SPEC for VramProvider trait extraction (P3 prep, RF-G1) ([c3ae8d5](https://github.com/emersonbusson/ramshared/commit/c3ae8d5033f7cba97e9b75d94e62c8c9aac9615c))
* **core:** SPECv2 status + IMPL.md for the P1 broker ([8863a8c](https://github.com/emersonbusson/ramshared/commit/8863a8c6263020d3a34d5c15a3eea73cf23496ae))
* **core:** VramProvider SPEC status + MEMORY (F4 passos 1-3 feitos) ([ee69a8f](https://github.com/emersonbusson/ramshared/commit/ee69a8fa58e34da528a2b908d21d0393fd85e713))
* **core:** VramProvider SPEC/MEMORY — daemon genérico feito (RF-G1) ([e1f35a8](https://github.com/emersonbusson/ramshared/commit/e1f35a8a482fbb83a620e76a62f887eae9678ce1))
* **cuda:** documenta afinidade de thread do Context CUDA ([#3](https://github.com/emersonbusson/ramshared/issues/3) M3) ([497cccf](https://github.com/emersonbusson/ramshared/commit/497cccf11ae8f196ac9e6f4cf855ef22f0693ba5))
* de-web SSDV3-PROMPTS (remove conteudo sem contexto kernel) ([4ad64e5](https://github.com/emersonbusson/ramshared/commit/4ad64e571701e8596185c0288c862925e4da21e4))
* **decisions:** ADR-0003 page-state/swap-safety model ([4a36f15](https://github.com/emersonbusson/ramshared/commit/4a36f151ea26ee961c4a9d5940a8e35bb59a24ec))
* fecha design dt-3 do worker ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([450e560](https://github.com/emersonbusson/ramshared/commit/450e560d9c084cc9b420d3eb48c277bcebb2083c))
* fix stale serve-only comments in daemon + session memory ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([29593df](https://github.com/emersonbusson/ramshared/commit/29593dfdf007811a100de8f2025109761f1b3491))
* **governance:** comentarios de codigo em PT-BR (regra alinhada ao projeto) ([#3](https://github.com/emersonbusson/ramshared/issues/3) LOW) ([4beee05](https://github.com/emersonbusson/ramshared/commit/4beee05eb2bd602bac6c4ae310c68827688d235b))
* humanize public surface — plain language first ([4fa4790](https://github.com/emersonbusson/ramshared/commit/4fa4790a768b76a3bf2d8be2351de6fdb166ec7a))
* init MEMORY.md (memoria de sessao do repo) ([ece1756](https://github.com/emersonbusson/ramshared/commit/ece1756a7fb30e895bb73e3ddac19404e30f000a))
* link wsl2-native-vram-tier PRD from README ([fef8dfc](https://github.com/emersonbusson/ramshared/commit/fef8dfcf3bfc47707da852fa1a74dd52bfb814a7))
* **marketing:** add EN/PT launch kit and cascade diagram ([ab11e30](https://github.com/emersonbusson/ramshared/commit/ab11e308f1119eb01e6bd1ac502604cf7bbed6b1))
* **marketing:** add human PT-BR cascade diagram ([d3cd5d5](https://github.com/emersonbusson/ramshared/commit/d3cd5d5fc7e022ec839e1241e20b0be9e7ac8f87))
* **marketing:** add step IDs for r/rust launch checklist ([f73361e](https://github.com/emersonbusson/ramshared/commit/f73361e5d9ed2bfb72d02347343fcfc7f3aac4b7))
* **marketing:** document IMG-PT-1 in posts index ([e7bc207](https://github.com/emersonbusson/ramshared/commit/e7bc2072d52ca0ca487a5c41519a1e5be51232e0))
* **marketing:** drop unnatural PT-BR "colchão" phrasing ([8ac32a2](https://github.com/emersonbusson/ramshared/commit/8ac32a2fbbc0c1eadeafab2cab404f6a542640a9))
* **marketing:** make post-01 paste blocks unmistakable ([a5cc7f6](https://github.com/emersonbusson/ramshared/commit/a5cc7f6f21a6d002680498c2bb413a35f00ed051))
* **marketing:** split launch posts into one file per channel ([9881842](https://github.com/emersonbusson/ramshared/commit/98818428a9dbf2e9d33e3cc894cbd2ac56495146))
* **marketing:** unify COPY START/END markers on all post files ([5a3243b](https://github.com/emersonbusson/ramshared/commit/5a3243bcdf641d2ad244dcc7d73f8cdbb9248823))
* mature dual-track status (WSL2 Day-1, Windows lab-only) ([d19dfdf](https://github.com/emersonbusson/ramshared/commit/d19dfdfa4c2a3283f15297353abccef12ffde470))
* **methodology:** expand Kahneman disciplines to match advoq full rigor ([b547fca](https://github.com/emersonbusson/ramshared/commit/b547fca0c05c9a2a324bf46900cb0eeb03f3fc96))
* **mm:** ADR-0007 + audit — kernel-native work is C not app Rust ([547e0a7](https://github.com/emersonbusson/ramshared/commit/547e0a7d24b0bed381d965c239a364b8cabf11bb))
* **mm:** align RF-L10 preflight with sparse gate from AUDIT-2.5 ([7b8412e](https://github.com/emersonbusson/ramshared/commit/7b8412e857567ce3e675381e0b6c124f89aef121))
* **mm:** close Phase 1 code audit ([b1c2156](https://github.com/emersonbusson/ramshared/commit/b1c21565acdf8eb7d78a1046b425106d8f0e3a3e))
* **mm:** converge vram-as-ram to SPECv3 (Fase 0 + auditorias Passo 2.5) ([0fa7876](https://github.com/emersonbusson/ramshared/commit/0fa78768ab100e5e7b7e7e6697b05c194cb7c50b))
* **mm:** disciplined Hyper-V campaign evidence for windows-swap-driver ([189e7eb](https://github.com/emersonbusson/ramshared/commit/189e7ebf5ebe17a04ed20b9cb7304350c4d001a3))
* **mm:** link windows-swap-driver IMPL to commit SHAs ([46ec072](https://github.com/emersonbusson/ramshared/commit/46ec072b9fe07bb8aefda32b6177cf8b10582be5))
* **mm:** mark WinDrive B1/B2 partial after DT-21 residency evidence ([f465eb3](https://github.com/emersonbusson/ramshared/commit/f465eb392714abeb1fb96d6a294d3b89a3907128))
* **mm:** parallel win11 reinstall + custom WSL 6.18 kernel track ([92ed2b9](https://github.com/emersonbusson/ramshared/commit/92ed2b98952d7a9beafb64cb8a345fb981a11b92))
* **mm:** Passo 2.5 cascade-vram-ondemand — GO + SPEC fixes ([bac5ccd](https://github.com/emersonbusson/ramshared/commit/bac5ccd857f1bef1a2561a435242b9f0a10b0e1f))
* **mm:** PRD for WSL2/Ubuntu native VRAM tier phases ([67beb9c](https://github.com/emersonbusson/ramshared/commit/67beb9c99c1f83840b11368d969f063395223bcc))
* **mm:** preflight windows-swap-driver for IMPL ([ad91b16](https://github.com/emersonbusson/ramshared/commit/ad91b16ac55df2305a35bb629bc178b72566d18a))
* **mm:** record cascade up/down validation (real swap zram&gt;VRAM&gt;VHDX) ([1c5bacd](https://github.com/emersonbusson/ramshared/commit/1c5bacd9000f19ae24386f5dc0000edced5cf9de))
* **mm:** record NBD device-wiring validation (/dev/nbd0 &lt;-&gt; VRAM) ([3172fc5](https://github.com/emersonbusson/ramshared/commit/3172fc5fb95180bd3c5b80c3bf80b839e7facf00))
* **mm:** record WDDM autotier partial-green gates ([9092223](https://github.com/emersonbusson/ramshared/commit/90922237e793d54a28331a9cb8977b5e8391c447))
* **mm:** record windows-swap-driver IMPL Passo 3 status ([6d3fb4a](https://github.com/emersonbusson/ramshared/commit/6d3fb4a707716e56ccbea1e986da713349a83fac))
* **mm:** SSDV3 cascade-vram-ondemand PRD/SPEC/AUDIT-2.5 ([622175f](https://github.com/emersonbusson/ramshared/commit/622175f8ae12ad330a58a9fa88979ff338cd8ebd))
* **mm:** SSDV3 decision PRD for kernel-true VRAM memory ([0f4f430](https://github.com/emersonbusson/ramshared/commit/0f4f430496a39e4d05386bf7c4d2207890c70a6c))
* **mm:** unblock dual-boot space on E: ESPANHA for kernel-true ([8c6082c](https://github.com/emersonbusson/ramshared/commit/8c6082cd26094b0b238c91bbc9fd0adca3eb9de5))
* point README to per-channel post files ([0400a5f](https://github.com/emersonbusson/ramshared/commit/0400a5fc98d9e2376fad11a5371fe78496851cec))
* PT comment rule + CUDA Context thread doc ([#3](https://github.com/emersonbusson/ramshared/issues/3) M3/LOW) ([9cd6e22](https://github.com/emersonbusson/ramshared/commit/9cd6e225ed1034586694e8538038b9bd89b912bc))
* public onboarding — human README, FAQ, quickstart, demo script ([69a0638](https://github.com/emersonbusson/ramshared/commit/69a0638a222cdf9b9c68323d9708b26f0578985d))
* registra acesso root (Linux) e admin (Windows) autorizado ([c153ef5](https://github.com/emersonbusson/ramshared/commit/c153ef52d17c695fea163cee926dc7ca037ee2a1))
* registra ADR io-uring aceita ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([9aa270c](https://github.com/emersonbusson/ramshared/commit/9aa270c914458bf98da55861bce057383d4122e6))
* registra bench e ring owner bloqueante ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([6aa3116](https://github.com/emersonbusson/ramshared/commit/6aa3116dff338bacd5f95891931e0182041ac26e))
* registra docs de raiz + esteira SSDV3 do canario ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([85f3d70](https://github.com/emersonbusson/ramshared/commit/85f3d70f3c2849c993e46ca1af1af77e2e949868))
* registra encoding do fetch io_cmd ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d0c1858](https://github.com/emersonbusson/ramshared/commit/d0c18582ab04c03d84a09757ea07ec053b81e666))
* registra fechamento parcial do [#3](https://github.com/emersonbusson/ramshared/issues/3) (PR [#6](https://github.com/emersonbusson/ramshared/issues/6), idioma + M3) ([e4f6dd2](https://github.com/emersonbusson/ramshared/commit/e4f6dd2aa2ce28bd397b4d1baf890327224e6f6b))
* registra fluxo de servico ublk no spec ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([fb18cb8](https://github.com/emersonbusson/ramshared/commit/fb18cb841ad20f54499b899550096c0bdbd269b5))
* registra gate de permissao ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([1cbfe96](https://github.com/emersonbusson/ramshared/commit/1cbfe9655586fcfe4238da78cb06bf6dc663142b))
* registra gate runtime ublk do check ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([83536af](https://github.com/emersonbusson/ramshared/commit/83536af82f368f0c487b43d100e4c329ad184b25))
* registra gate ssdv3 da frente b (daemon) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([8c4f7e7](https://github.com/emersonbusson/ramshared/commit/8c4f7e7e63296d763c010dc1ee180e3931629db7))
* registra gate ublk vs nbd passado ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d8d7271](https://github.com/emersonbusson/ramshared/commit/d8d7271dd00e6ee09d4f46d20b0406dc7ed42116))
* registra helpers ublk seguros da Fase B ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([52831e3](https://github.com/emersonbusson/ramshared/commit/52831e3cc8d3fbe94e9da2513f4611cac6ffc1f2))
* registra M1 mmap io-desc ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([3233b2f](https://github.com/emersonbusson/ramshared/commit/3233b2fe5b5e2966f52d36ecd9499306f2cf096d))
* registra M2 fetch no-wait e teardown ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([4bf68c3](https://github.com/emersonbusson/ramshared/commit/4bf68c3866f913d5766f12f86c1f2987545233ab))
* registra mapper ublk para request ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([1d80bc1](https://github.com/emersonbusson/ramshared/commit/1d80bc1621ff2936bfdf828c9e9e60559352ef80))
* registra merge do PR [#2](https://github.com/emersonbusson/ramshared/issues/2) + revisao adversarial (MEMORY.md) ([1b75e07](https://github.com/emersonbusson/ramshared/commit/1b75e07ca27f487b260700b790a570ab3894d179))
* registra no-alloc dt-8 e fase b completa ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([c6028bc](https://github.com/emersonbusson/ramshared/commit/c6028bc5c85cedceed7969c8f349754c8c412ad6))
* registra Passo 2.5 (2a) -&gt; SPECv3 do canario ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([46d3683](https://github.com/emersonbusson/ramshared/commit/46d368366fa42e1d1198eb8ad787f43f653426fd))
* registra ponte ublk pura ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([4b1af7b](https://github.com/emersonbusson/ramshared/commit/4b1af7b275dd287a464ff7f547be5171bc26bea7))
* registra preflight seguro da Fase B ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([1e17ef6](https://github.com/emersonbusson/ramshared/commit/1e17ef6d3b3a4b640de5c898f84ec8fad864e3a1))
* registra progresso da issue [#3](https://github.com/emersonbusson/ramshared/issues/3) (C3/M1 + M2/M4/M5) ([d04b650](https://github.com/emersonbusson/ramshared/commit/d04b6506225d9577589fcb8a1111308b44d5c7e2))
* registra requests multi-pagina e gotcha de teardown ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([4276c73](https://github.com/emersonbusson/ramshared/commit/4276c734f5b7f0dbee51dbbd4e82a2255cffe620))
* registra retomada segura da Fase B ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([501a0b5](https://github.com/emersonbusson/ramshared/commit/501a0b5cf74a2213644a6559719febb6e409f2e1))
* registra ring owner dt-3 do ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([94f82e9](https://github.com/emersonbusson/ramshared/commit/94f82e9fb3e2aca00234570ab47104b93fc36a47))
* registra set_params ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([c62b5dc](https://github.com/emersonbusson/ramshared/commit/c62b5dc41d79a22dd44a9683025d68932dd348b1))
* registra smoke minimo io_uring ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([1ab3f0d](https://github.com/emersonbusson/ramshared/commit/1ab3f0d9c73a672c654f17e28eecb683f3141b1b))
* registra smoke ublk add delete ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([21ef128](https://github.com/emersonbusson/ramshared/commit/21ef128d6c22e4da334a361f6a2e6a0d13b2a1de))
* registra smoke ublk GET_FEATURES ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([38a6ca1](https://github.com/emersonbusson/ramshared/commit/38a6ca180f78da142eb4d557481b85ea13043cab))
* registra ublk funcional com backend ram ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([f428a1e](https://github.com/emersonbusson/ramshared/commit/f428a1e8899181bb10c146190730bf37a2a4a2d3))
* registra ublk servindo vram ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([ebfb3a1](https://github.com/emersonbusson/ramshared/commit/ebfb3a1a6b1fb12d1bf447e94f8f47ea0c81f750))
* registra validacao de queue_depth&gt;1 no ublk dt-3 ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([59434bf](https://github.com/emersonbusson/ramshared/commit/59434bf164755d112827df404ea085c556d5427a))
* registra vram como swap por ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([183c418](https://github.com/emersonbusson/ramshared/commit/183c418dd70ffeba5f5cbd7d7b69e9583027527c))
* registra worker dt-3 do ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([0a9694d](https://github.com/emersonbusson/ramshared/commit/0a9694d6508058d5a3d8433581be90c5d0cb8248))
* registra wrapper ramshared-uring ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([690e554](https://github.com/emersonbusson/ramshared/commit/690e5544d3c08c18cf0c1bbc4d6254acfc10ba18))
* registra write concorrente qd&gt;1 e cap de request 4KB ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([e43d2dd](https://github.com/emersonbusson/ramshared/commit/e43d2dd66a4e70e4c244ad55f28cffd97684c4b4))
* registra write e2e e plano m3c ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([bc56f98](https://github.com/emersonbusson/ramshared/commit/bc56f989c4ae8ceeed1aa5c984abaf2177a623ae))
* **release:** add validation.md tracing empirical tests ([9c02a18](https://github.com/emersonbusson/ramshared/commit/9c02a18bc7a5b61da62c7356e305510fc913328a))
* **release:** fix absolute links in specs and remove obsolete arbiter ([8672742](https://github.com/emersonbusson/ramshared/commit/8672742dac6df5de094fac0b44cd7b3d4bdf2e70))
* **release:** standardize prds/specs under milestone structure and delete redundant docs ([1d7454b](https://github.com/emersonbusson/ramshared/commit/1d7454bf1127f83e55b533675eb86d462d851165))
* **release:** translate core docs and add open-source files ([aa0319a](https://github.com/emersonbusson/ramshared/commit/aa0319a9a288e158a9c0a3f9255bad0b6fac96e5))
* **release:** translate kahneman disciplines and remove ai prompts ([0080481](https://github.com/emersonbusson/ramshared/commit/00804817cd89ef600285a1438653fcb281a7659b))
* root docs (README, ARCHITECTURE, ROADMAP) ([7aa80d5](https://github.com/emersonbusson/ramshared/commit/7aa80d5d6ee1f789fc4aa08dead3b9e11141a824))
* **rules:** convert plain text references to standard markdown links ([5dda9b5](https://github.com/emersonbusson/ramshared/commit/5dda9b5cae90134c64da50170a24ed2026806f17))
* **rules:** expand AGENTS.md/CLAUDE.md and import governance ([f075386](https://github.com/emersonbusson/ramshared/commit/f0753862bda69a9593463ab52f4d21f9e8ef8725))
* **rules:** explicitly list coding and governance in root docs ([ae82c17](https://github.com/emersonbusson/ramshared/commit/ae82c17e3e24021cbf8931b5f89f877d315aaa9f))
* **rules:** fix ssdv3 paths + de-web "como linkar" example (S3) ([28feec3](https://github.com/emersonbusson/ramshared/commit/28feec31f01fc9388add5872b9191ffc27ee2c25))
* **rules:** scope SSDV3 obrigatorio to kernel triggers + sync AGENTS ([5d10025](https://github.com/emersonbusson/ramshared/commit/5d10025957c5259d65bb0df3185c37d88b837d0a))
* **rules:** translate development rules to English ([d81f763](https://github.com/emersonbusson/ramshared/commit/d81f76354ccacf7b4f3acc5add1ad0802d45c858))
* spec ssdv3 do ring loop ublk ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([a3ec754](https://github.com/emersonbusson/ramshared/commit/a3ec75454eff480e26676c22a85eb16100b00558))
* **ssdv3:** adopt unique SPEC model and kernel methodology stack ([3d77d32](https://github.com/emersonbusson/ramshared/commit/3d77d3270e7fbc1f7f0a4681c0eb68b43e2d0034))
* **ssdv3:** analise do teardown F2 + recipe de validacao em qemu ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([9153405](https://github.com/emersonbusson/ramshared/commit/91534051bc2d96a4b22e2cfdedbfb4ffd67ca498))
* **ssdv3:** expand prompt framework to match advoq rigor (full length) ([366939a](https://github.com/emersonbusson/ramshared/commit/366939a864822f04bbaea0a7dd92e15d9a8624f5))
* **ssdv3:** Fase B design specs — zram-writeback & ublk (esteira até SPECv2, go) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([9c63a2e](https://github.com/emersonbusson/ramshared/commit/9c63a2e59faae918a7caab7d17449a14329be4cf))
* **ssdv3:** Fase B prep — ADR-0004 (io_uring crate) + runbook do kernel custom ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([6613638](https://github.com/emersonbusson/ramshared/commit/66136385975e1411d6a7b74271653a0a2f35e9ea))
* **ssdv3:** fix runbook — CONFIG_ZRAM_WRITEBACK é bool (--enable), pego ao validar o build ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([adda3f9](https://github.com/emersonbusson/ramshared/commit/adda3f90cf2c0666fa9c721694e26e8162316ed0))
* **ssdv3:** IMPL — precisao do gap Vulkan (ICD NVIDIA no WSL2 e DLL Windows) ([931792e](https://github.com/emersonbusson/ramshared/commit/931792e09108f4b05f9fccce94eeb03c64dbdcc0))
* **ssdv3:** IMPL — rota Dozen/dzn investigada p/ real-GPU no WSL2 ([38ceffa](https://github.com/emersonbusson/ramshared/commit/38ceffa8812ffae7f1e0bf32336dd0f7ec02e48f))
* **ssdv3:** IMPL do backend Vulkan (RF-G2) + LIBRARIES (ash) ([b6ebdee](https://github.com/emersonbusson/ramshared/commit/b6ebdee1238d0ffe46f5e9b0fa952f98ff50aa51))
* **ssdv3:** IMPL F1 da integracao ublk (worker com residencia) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([da4639e](https://github.com/emersonbusson/ramshared/commit/da4639e026b53dd25458cabbd478ae70defe956b))
* **ssdv3:** memory-broker SPECv2 (go) + P0 results + ADR-0005 ([09fb1ea](https://github.com/emersonbusson/ramshared/commit/09fb1eaec0373d076161d9f9b6a92ca462889d4c))
* **ssdv3:** P2 Windows SPEC (RF-W1..W3/P1/P3) + auditoria 2.5 (no-go → SPECv2) ([56735aa](https://github.com/emersonbusson/ramshared/commit/56735aa3f3bfaa4dcb7c8e1bcc5fe66466524a70))
* **ssdv3:** PRD + SPEC for dedicated residency canary ([#8](https://github.com/emersonbusson/ramshared/issues/8), §9.4) ([6460db4](https://github.com/emersonbusson/ramshared/commit/6460db4a3e9e2ffde14e1c064987eaed697326d1))
* **ssdv3:** PRD da integracao ublk no daemon ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([f3a5f7a](https://github.com/emersonbusson/ramshared/commit/f3a5f7aaa997e5f736960e4720abaf4296df974c))
* **ssdv3:** PRD da P2 — ponte Windows + MVP DCC (RF-W1..W3) ([1fba443](https://github.com/emersonbusson/ramshared/commit/1fba4433272f9f1d66b2c60d7085855d7ef7c8f2))
* **ssdv3:** PRD do backend Vulkan (RF-G2) ([7f195a1](https://github.com/emersonbusson/ramshared/commit/7f195a1e2f4d77260ffa1e208effbc17d7be966d))
* **ssdv3:** PRD do canario de residencia dedicado (§9.4) ([#8](https://github.com/emersonbusson/ramshared/issues/8) Passo 1) ([07f47ba](https://github.com/emersonbusson/ramshared/commit/07f47ba1ec8296cd611684efbb470937ddecf8e3))
* **ssdv3:** PRD do tier vram multi-tenant wsl2-civm ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([439b461](https://github.com/emersonbusson/ramshared/commit/439b4613bd404bde56528703bdb655525fdada01))
* **ssdv3:** PRD unificado final do memory broker ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([f189bde](https://github.com/emersonbusson/ramshared/commit/f189bde276cf2a541c0ed70f39901bf15c1f9fc8))
* **ssdv3:** PRD/SPEC/SPECv2 — coletor de telemetria & reconciliação do broker ([7a95ee9](https://github.com/emersonbusson/ramshared/commit/7a95ee9cd22de420726047a451867df08947e9cc))
* **ssdv3:** runbook — add make modules_install (pego ao instalar o kernel) ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([d8c0a23](https://github.com/emersonbusson/ramshared/commit/d8c0a23d762149014ee74cac796f91ca5c7f8edf))
* **ssdv3:** SPEC da integracao ublk no daemon ([#3](https://github.com/emersonbusson/ramshared/issues/3)) ([ecb389b](https://github.com/emersonbusson/ramshared/commit/ecb389b5f892035b286d52f557d0adeb3096e1eb))
* **ssdv3:** SPEC do backend Vulkan (RF-G2) + auditoria 2.5 (go) ([4626350](https://github.com/emersonbusson/ramshared/commit/4626350b8d104ed08aff995a4a8c291cb94f1bb3))
* **ssdv3:** SPEC do canario de residencia dedicado (§9.4) ([#8](https://github.com/emersonbusson/ramshared/issues/8) Passo 2) ([aa5a778](https://github.com/emersonbusson/ramshared/commit/aa5a778d585c5a6bc992d0b114c033ded8c1a2bf))
* **ssdv3:** SPEC do P2 Windows (RF-W1..W3/P1/P3) + auditoria 2.5 (no-go -&gt; SPECv2) ([6d17fb9](https://github.com/emersonbusson/ramshared/commit/6d17fb942b1ea0d467df1c780814db71c4d41e34))
* **ssdv3:** SPEC DT-10 — VK_EXT_memory_budget opcional no backend Vulkan ([36f3586](https://github.com/emersonbusson/ramshared/commit/36f35864ef05149e13c24b4dd33bf501a0e4255b))
* **ssdv3:** SPEC DT-11 — ublk+vulkan deferido (servidor de residencia CUDA-fixo) ([d4617e7](https://github.com/emersonbusson/ramshared/commit/d4617e7781fec7ed8f466f4ad2e089e692bb5323))
* **ssdv3:** SPECv2 after Passo 2.5 no-go (hybrid canary) ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([48f7446](https://github.com/emersonbusson/ramshared/commit/48f74463768408ce8f993d66324c9b42d3c0f525))
* **ssdv3:** SPECv2 do canario apos Passo 2.5 (no-go -&gt; desenho hybrid) ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([bbce96c](https://github.com/emersonbusson/ramshared/commit/bbce96c22fd027a243c99eb72e94f21af9d3a88f))
* **ssdv3:** SPECv3 after Passo 2.5 on SPECv2 (free-floor hysteresis) ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([6f48f89](https://github.com/emersonbusson/ramshared/commit/6f48f89f614f84e42a9f328ae33ba7367033ed21))
* **ssdv3:** SPECv3 do canario apos Passo 2.5 sobre o SPECv2 (no-go -&gt; histerese) ([#8](https://github.com/emersonbusson/ramshared/issues/8)) ([d93d938](https://github.com/emersonbusson/ramshared/commit/d93d9380150227371b699bb5fef97e0afd0aad1f))
* translate all remaining legacy comments to English ([a4260bc](https://github.com/emersonbusson/ramshared/commit/a4260bc4f2658449a29ed8f7d0e6e8f97a8a62a7))
* **vram:** registra validação de aceitação §14 (cascata + DEMOTE) ([5bd6b99](https://github.com/emersonbusson/ramshared/commit/5bd6b995477b4a710920263401fbc8ece4248544))
* **windows-vram-drive:** drill Passo 0 executado em VM — R7 refinado ([e931844](https://github.com/emersonbusson/ramshared/commit/e931844668732117dec94e881ec6a40f05374765))
* **windows-vram-drive:** translate PRD and adapt to advoq layout ([166aeca](https://github.com/emersonbusson/ramshared/commit/166aeca2c94a66f4e79501028ff0774044d2acbf))


### CI

* add GitHub Actions (fmt + clippy -D warnings + test) ([5a10b97](https://github.com/emersonbusson/ramshared/commit/5a10b9778da8830fc86e057511615e5983c99e4f))
* add release-please, dependabot, and custom static gates ([eb13045](https://github.com/emersonbusson/ramshared/commit/eb13045362abfdefda5b2f032d63e1fde6b14cf2))
* add release-please, dependabot, and custom static gates ([49cb1fd](https://github.com/emersonbusson/ramshared/commit/49cb1fd5abf0aacf0d0491c358743a2c7faab458))
* **deps:** bump the all-actions group across 1 directory with 5 updates ([503f7e8](https://github.com/emersonbusson/ramshared/commit/503f7e8706e923d9a10a58134d6f4dc88e440af6))
* **deps:** bump the all-actions group across 1 directory with 5 updates ([f44cb0a](https://github.com/emersonbusson/ramshared/commit/f44cb0a4b7823851e207af81d4ba80273ceb9b59))
