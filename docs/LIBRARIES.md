# LIBRARIES — decisões de API/subsistema (RamShared)

Registro anti-halo (Kahneman #11): nenhuma API/subsistema/dependência entra sem
**critério mensurável**, **alternativas** e um **"quando revisitar"**. Um LKM
ideal tem zero deps externas — aqui o registro é de **escolhas de API de kernel**
e das poucas deps userspace. Inclui "deliberadamente NÃO usado". O caminho NBD atual segue
zero-dep externa; a Fase B/ublk tem uma exceção userspace explicitamente gated.

## Escolhas ativas

| Escolha | Critério (mensurável / compatibilidade dura) | Quando revisitar |
| --- | --- | --- |
| **Block backend: NBD** (Fase A) | único que funciona em GeForce consumer (`nvidia_p2p_*` → `EINVAL`); `nbd.ko` presente, só `modprobe` | quando o kernel WSL2 tiver `CONFIG_BLK_DEV_UBLK` |
| **Block backend: ublk** (Fase B) | latência menor (io_uring), sem round-trip socket | exige kernel custom; só após Fase B |
| **Userspace ring: `ramshared-uring` + `io-uring` crate** (Fase B, gated) | `ramshared-uring` isola qualquer `unsafe` de SQE; `io-uring 0.7.12` (MIT/Apache-2.0) evita hand-roll de barreiras acquire/release; lockfile traz também `libc`, `bitflags`, `cfg-if`; ADR-0004 aceita a exceção | remover se bench ublk não superar NBD ou se auditoria de supply chain falhar |
| **Broker protocol: `serde`/`serde_json` (JSON-lines)** (P1, broker) | control-plane ~1 msg/s/tenant, debugável com `nc`/`jq`; `serde 1.0.228` + `serde_json 1.0.150` (MIT/Apache-2.0) dão validação de shape de graça e evitam parser hand-rolled frágil; confinado ao crate `ramshared-broker` (daemon/lib seguem `#![forbid(unsafe_code)]`); [ADR-0005](decisions/ADR-0005-broker-protocol-jsonl.md) | migrar p/ length-prefixed (`bincode`) se payload >64 KiB/msg ou >100 msg/s/tenant |
| **Tier quente: zram (lzo-rle)** | RAM comprimida, baixa latência; presente (`CONFIG_ZRAM=m`) | se `CONFIG_ZRAM_WRITEBACK` for habilitado → writeback p/ VRAM |
| **VRAM: CUDA Driver API via `dlopen`** | funciona sem toolkit sobre a stub `libcuda` do WSL2; `cuMemcpyHtoD/DtoH` em qualquer GPU | se surgir caminho coerente (CXL bare-metal) |
| **VRAM backend Vulkan: `ash` 0.38** (RF-G2) | binding Vulkan padrão (battle-tested, mantido); reuso > hand-roll do loader/FFI (regra dura #1); `DEVICE_LOCAL` + staging + transfer queue cobre "qualquer GPU" (AMD/Intel/NVIDIA); **validado no lavapipe** (Vulkan em CPU): round-trip bytes-iguais. `unsafe` isolado no crate `ramshared-vulkan` (`// SAFETY:` por bloco), fronteira do trait sem `unsafe`. MIT/Apache-2.0 | se `cargo audit`/`deny` sobre `ash` falhar, ou se D3D12/`/dev/dxg` (RF-G3) cobrir não-NVIDIA dentro do WSL2 |
| **Userspace lang: Rust (std)** | safety + RAII de recursos GPU (ver [ADR-0002](decisions/ADR-0002-rust-userspace-port.md)) | se FFI provar instável (rollback do ADR-0002) |

## Deliberadamente NÃO usado

- **`nvidia_p2p_get_pages_persistent` / BAR1 `ioremap_wc`** — `EINVAL` em GeForce consumer; BAR1 mapeia só ~16 MiB (framebuffer).
- **zram-writeback** — exige `CONFIG_ZRAM_WRITEBACK` (kernel custom); cascata por prioridade resolve Day-0.
- **MTD/phram (MMIO direto)** — descartado por performance (CPU memcpy).
- **OpenCL** (proposta original do PRD-2) — CUDA escolhido para o caminho WSL2/GPU-PV.
- **`vulkano` (Vulkan alto nível)** — esconde o controle de memória/filas que o tier precisa (alloc `DEVICE_LOCAL`, transfer queue, staging) e adiciona peso; `ash` (binding fino) escolhido no RF-G2. Hand-roll do FFI `libvulkan` também descartado: superfície grande demais p/ Day-0 (regra dura #1).
- **`clap` (arg parsing)** — descartado p/ preservar **zero-dep externa** num projeto
  Ring-0/Day-0 (#11). Para ~4-9 flags o parser hand-rolled (`std::env::args`) atende; clap
  traria ~10 crates transitivas + custo de build. A qualidade do "polish" (issue #3 LOW) veio
  de **erros tipados** (`CascadeError`, sem dep), não de clap. Revisitar se o CLI crescer
  muito (muitos subcommands/validações com `--help` rico).

## Forward (bare-metal — decisões a registrar quando aplicável)

`HMM`/`devm_memremap_pages(DEVICE_PRIVATE)` vs NUMA hotplug · `spinlock` vs
`mutex` em hot path · `workqueue` vs `kthread` — cada uma exigirá critério
mensurável e ADR própria.
