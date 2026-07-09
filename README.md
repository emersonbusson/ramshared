# RamShared

GPU VRAM as a **swap** tier in Linux/WSL2. In non-graphical workloads, GPU VRAM remains ~90% idle while system RAM is exhausted, leading to swapping on slow SSDs — which are dozens of times slower than VRAM. RamShared bridges this gap by inserting VRAM into the swap hierarchy.

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Platform" src="https://img.shields.io/badge/Platform-Linux%20%7C%20WSL2-blue?style=flat-square&logo=linux&logoColor=white">
  <img alt="CUDA" src="https://img.shields.io/badge/CUDA-Enabled-green?style=flat-square&logo=nvidia&logoColor=white">
</p>

---

<details>
<summary><strong>PT-BR</strong> — clique para ler em português</summary>

# RamShared (Português)

A memória RAM física em workstations e servidores de alta performance é cara. O custo por gigabyte de RAM de alta velocidade (como DDR5/ECC) em servidores de produção ou workstations de desenvolvimento pode ser de 5 a 10 vezes maior do que a VRAM ultra-rápida (GDDR6) que já está instalada e paga nas placas de vídeo, mas que permanece mais de 90% do tempo ociosa durante tarefas que não sejam de renderização 3D ou IA local. 

Quando o sistema esgota a RAM principal compilando código ou rodando containers, ele é forçado a fazer swap em SSDs. O RamShared resolve essa ineficiência financeira e física inserindo a VRAM ociosa como um tier de swap de alta largura de banda.

## A Hierarquia e o Modelo de Segurança

VRAM sob pressão extrema de desalocação pelo sistema operacional hospedeiro (via WDDM/GPU-PV) é **latency-unsafe**: uma leitura de 4KB pode demorar até **1.18 s** para retornar enquanto o host recupera memória — o que congelaria o sistema de swap quente. 

Por isso, o RamShared organiza o swap em uma cascata estrita de prioridades via kernel:
1.  **zram** (prio 200, HOT): RAM comprimida que absorve o working set quente de baixa latência.
2.  **VRAM** (prio 100, COLD): A VRAM ociosa atuando como swap frio para dados raramente acessados.
3.  **VHDX/SSD** (prio -2, LAST): O swap padrão do disco virtual de último recurso.

Um daemon watchdog de baixa prioridade monitora a latência e o nível de preenchimento. Sob spike de latência do barramento PCIe, ele executa um **DEMOTE** instantâneo via `swapoff`, migrando de forma transparente as páginas da VRAM para o tier inferior do SSD sem interromper processos.

## Contribuição com Disciplina (Kahneman Sistema 2)

Manipular tabelas de swap, I/O de bloco de baixo nível e chamadas CUDA exige raciocínio analítico lento e deliberado (Sistema 2), rejeitando atalhos de intuição impulsiva (Sistema 1). Erros no Ring 0 ou em subsistemas adjacentes resultam em kernel panics destrutivos ou BugChecks de tela azul no Windows.

Se você deseja colaborar com o projeto, exigimos adesão estrita ao rigor científico:
*   **Decisões com Counterfactuals:** Cada alteração na gerência de locks, DMA ou concorrência deve vir acompanhada da justificativa de qual cenário alternativo pior seria gerado se a lógica não existisse.
*   **Rollback Triggers Numéricos:** Todo commit estrutural deve conter um gatilho de reversão baseado em métrica numérica (ex: *"Rollback se a latência exceder 50us sob concorrência de 8 threads"*).
*   **Verificação Adversarial:** Desenvolvemos focados em testar o pior cenário possível (estresse térmico de barramento, resets de GPU, concorrência assimétrica). Nosso objetivo é manter 0 falhas em testes de soak contínuos de 72 horas.
*   **SSDV3** para mudança estrutural: PRD → SPEC → IMPL em [`docs/specs/`](docs/specs/) — índice [`docs/INDEX.md`](docs/INDEX.md); prompts em [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
*   **Higiene de docs:** após criar/alterar specs, `./scripts/docs-check.sh`.

Se você está disposto a trabalhar sob esta disciplina de engenharia de alta confiabilidade, leia o guia de contribuição e envie seu PR.

## Como Usar

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d

sudo ./target/debug/ramshared check
sudo ./target/debug/ramshared up --vram 1024 --zram 1024
swapon --show
sudo ./target/debug/ramshared down
```

</details>

---

## The Economics of Memory & Silicon Efficiency

Physical RAM in high-performance workstations and dedicated production servers is expensive. The cost per gigabyte of high-speed system memory (DDR5/ECC) is often 5 to 10 times higher than that of ultra-fast GPU VRAM (GDDR6/HBM) that has already been purchased but remains ~90% idle during compilation, container execution, or general developer workflows.

When RAM is depleted, systems are forced to swap on SSD storage drives, which are orders of magnitude slower than the GPU's memory bus. RamShared solves this cost and hardware utilization bottleneck by repurposing idle VRAM as a high-bandwidth swap tier.

## Swap Cascade & Safety Architecture

Under extreme physical memory pressure, host OS GPU eviction (via WDDM/GPU-PV) is **data-safe but latency-unsafe**: a 4KB read request can stall for up to **1.18 seconds** while the host driver page-faults. If armed as a primary hot swap space, this latency would lock up the system.

RamShared resolves this limitation by constructing a priority-ordered kernel swap cascade:
```text
Memory Pressure ─► zram  (Compressed RAM)          prio 200  HOT
                ─► VRAM  (CUDA + NBD daemon)       prio 100  COLD
                ─► VHDX  (WSL2 default swap SSD)   prio  -2  LAST
```

*   **zram** handles the hot active working set.
*   **VRAM** acts as a cold buffer, hiding its latency weakness behind the zram compression layer.
*   An active background monitor acts as a safety-net. Upon detecting bus latency spikes or host eviction triggers, it launches a **DEMOTE** thread using `swapoff` to safely migrate resident VRAM pages down to the SSD tier without disrupting running applications.

## Contribution Standards (Kahneman System 2)

Interacting with operating system memory layers, low-level block devices, and hardware bus registers requires slow, analytical, and deliberate reasoning (System 2) rather than intuitive trial-and-error (System 1). A single race condition or dangling pointer in swap paths leads to fatal kernel panics or blue screens (BSOD).

To maintain this codebase for production-grade reliability, contributors must adhere to strict guidelines:
1.  **Counterfactual Decision-Making:** Every PR modifying concurrency, locks, or hardware interaction must document its counterfactual case (why this logic is the only path that prevents worst-case failure).
2.  **Measurable Commit Triggers:** All non-trivial commits must include a `Rollback trigger:` line specifying a measurable performance degradation metric that warrants a reversion.
3.  **Adversarial Quality Gating:** We build for the worst-case scenario. We target 0 regressions, 100% test coverage for critical lock paths, and 0 BugChecks during 72-hour soak tests.
4.  **SSDV3 for structural work:** locks/DMA/uAPI/mm/new driver surfaces use PRD → SPEC → IMPL under [`docs/specs/…`](docs/specs/) — index at [`docs/INDEX.md`](docs/INDEX.md). Process: [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
5.  **Docs hygiene:** after adding/changing specs, run `./scripts/docs-check.sh` (or `node tools/generate-docs-index.mjs`).

If you are committed to this high-reliability engineering discipline, please review our [CONTRIBUTING.md](CONTRIBUTING.md) and join the development.

## Launch kit (social / Show & Tell)

Ready-to-post copy (EN + PT-BR), channel pattern, registered metrics, and cascade diagram:

*   [`docs/marketing/LAUNCH-KIT.md`](docs/marketing/LAUNCH-KIT.md)
*   Diagram: [`docs/marketing/cascade-diagram.png`](docs/marketing/cascade-diagram.png)

## Getting Started

```bash
cargo build -p ramshared-cli -p ramshared-wsl2d

# Validate environment capabilities (WSL2, CUDA, tiers)
sudo ./target/debug/ramshared check

# Mount swap cascade (1 GiB zram, 1 GiB VRAM)
sudo ./target/debug/ramshared up --vram 1024 --zram 1024

# Verify cascade order (zram prio 200 > VRAM prio 100 > VHDX prio -2)
swapon --show

# Gracefully dismantle swap tiers before GPU disconnect
sudo ./target/debug/ramshared down
```

## Crate Layout

| Crate | Role |
|---|---|
| `ramshared-tier` | Cascade priority management + DEMOTE safety net invariants |
| `ramshared-cuda` | CUDA Driver API dynamically loaded wrappers (**only unsafe block boundary**) |
| `ramshared-block` | NBD fixed-newstyle protocol state machine & I/O library |
| `ramshared-integrity` | Block checksum FNV-1a calculations and validation patterns |
| `ramshared-uring` | Clean wrapper around asynchronous `io-uring` |
| `ramshared-wsl2d` | Daemon: `VramBackend` mapping, `mlockall` system overrides, and latency canary |
| `ramshared-cli` | Command-line management: `check`/`doctor`/`up`/`down` |
