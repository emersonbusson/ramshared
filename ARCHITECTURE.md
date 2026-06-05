# Arquitetura — RamShared

Foco: a implementação **WSL2** (SPECv3), única viável no guest GPU-PV. O destino
bare-metal (NUMA/HMM/CXL) está no [ROADMAP.md](ROADMAP.md).

## Visão geral

O RamShared **orquestra** uma cascata de swap por prioridade e **gerencia o tier
VRAM**; `zram` e `VHDX` são mecanismos do kernel que ele apenas configura.

```text
pressão de memória ─► zram  (RAM comprimida)  prio 200  HOT
                   ─► VRAM  (daemon CUDA+NBD)  prio 100  COLD
                   ─► VHDX  (swap do WSL2)     prio  -2  LAST
```

## Modelo de segurança (o pivô)

A Fase 0 mediu, em GPU real, que a eviction WDDM é:

- **data-safe** — o hash da página continua íntegro após a eviction;
- **latency-unsafe** — uma leitura 4K com a VRAM cheia custou **1,18 s**.

Como swap quente, a VRAM congelaria o sistema. Como tier **frio** atrás do zram, só
recebe o spill de acesso raro — escondendo a latência. Quando o sinal de latência
dispara (host reavendo VRAM), o **DEMOTE** faz `swapoff` do tier VRAM e o kernel
migra as páginas vivas para o VHDX, **sem derrubar processos**.

Invariante **A1**: o DEMOTE só é seguro se existe um tier *estritamente abaixo* da
VRAM para drenar (checado em `up` e na rede de segurança do `ramshared-tier`).

## Componentes

| Crate | Responsabilidade | `unsafe` |
|---|---|---|
| `ramshared-tier` | prioridades (`zram 200 > vram 100 > vhdx -2`), `validate_order`, rede A1 | `forbid` |
| `ramshared-cuda` | `libcuda` via `dlopen` (runtime, sem toolkit/`build.rs`); RAII (`Cuda`→`Context`→`DeviceMem`) | **isolado aqui** |
| `ramshared-block` | NBD fixed-newstyle: parse/encode, validação §8, handshake | `forbid` |
| `ramshared-integrity` | checksum (FNV-1a) + padrões de teste | `forbid` |
| `ramshared-wsl2d` | daemon: máquina de estados §7, `VramBackend` (liga CUDA↔NBD), `mlockall`/`oom_score_adj`, canário/DEMOTE | só `mlockall` |
| `ramshared-cli` | `check`/`doctor` (preflight) + `up`/`down`/`status` (orquestração) | `forbid` |

Todo o `unsafe` do projeto vive no `ramshared-cuda` (FFI), com `// SAFETY:` por bloco;
a exceção é o `mlockall` do daemon, justificado e isolado. Zero dependências externas
(`std` + FFI).

## Fluxo de execução

1. **`up`** valida a ordem de prioridade e a rede A1, sobe o `zram`, sobe o daemon e
   conecta `/dev/nbd0` via `nbd-client -unix` — **o daemon não faz ioctl nem `unsafe`**
   (o `nbd-client` faz a fiação do kernel). `mkswap` + `swapon -p` montam os tiers.
2. **Daemon** aloca e zera a VRAM, trava memória (`mlockall`) e se protege do OOM
   (`oom_score_adj=-1000`, Disciplina 3), e serve NBD: cada READ/WRITE vira
   `cuMemcpyDtoH/HtoD` na VRAM.
3. **Canário §9 (inline):** o daemon cronometra a latência do I/O; após a baseline,
   arma o `Canary`; sob spike (latência > N× baseline por M amostras) dispara o
   **DEMOTE** numa thread (`swapoff <nbd>`) e segue servindo o read-back. Só desarma
   se o `swapoff` confirmar; senão re-arma.
4. **`down`** faz `swapoff` do NBD **antes** de desconectar (senão: kernel panic),
   depois desmonta o zram; espera o daemon zerar a VRAM (§11) antes de qualquer
   `pkill`.

## Decisões-chave

- **NBD, não ublk** (Fase A): `CONFIG_BLK_DEV_NBD=m` existe; `ublk` exigiria kernel
  custom. Mantém o daemon sem `unsafe`.
- **`dlopen` em runtime**, não link-time: o WSL2 só tem a stub `libcuda` do host, sem
  toolkit → sem `build.rs`.
- **Cascata por `swapon -p`**, não writeback: `CONFIG_ZRAM_WRITEBACK` não está setado
  neste kernel; o writeback direto (zram grava frio na VRAM) fica para a Fase B.
- **Decisão pura vs. efeito:** a lógica do canário (`residency.rs`) e da cascata
  (`tier`) é pura e testável sem GPU/root; só o daemon executa CUDA e `swapoff`.

## Metodologia

- **SSDV3** — Spec-Driven Development: PRD → SPEC → IMPL, com auditoria Passo 2.5
  (go/no-go) entre versões. Ver [`docs/methodology/`](docs/methodology/) e
  [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md).
- **Disciplinas Kahneman** — toda decisão de lock/DMA/arquitetura registra
  counterfactual e *rollback trigger* numérico ([`docs/methodology/KAHNEMAN-DISCIPLINES.md`](docs/methodology/KAHNEMAN-DISCIPLINES.md)).
- **Day-0** e **governança** (template de PR, sync rule) em [`.claude/rules/`](.claude/rules/).

## Validação

Aceitação §14 no sistema vivo (cgroup-confined): spill de **511 MiB** para a VRAM
(332.800 páginas íntegras) e DEMOTE de **481 MiB** vivos migrados sem corrupção. Ver
[`docs/vram-as-ram/VALIDATION-CASCADE.md`](docs/vram-as-ram/VALIDATION-CASCADE.md).

## Limitações conhecidas (rastreadas)

- Canário inline usa **só latência** (WDDM é data-safe). Conteúdo/free-floor exigem o
  canário dedicado §9.4 — issue #3 (C1).
- Serve loop **serial** (1 conexão = a vida do swap); o read-back do DEMOTE não tem
  leitor dedicado — issue #3 (H1).
