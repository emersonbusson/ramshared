# RamShared

Idioma: [English](README.md)

> Esta tradução é informativa e não normativa. O [`README.md`](README.md) em
> inglês é a fonte canônica para requisitos técnicos e limites de segurança.

**O RamShared transforma a memória de vídeo (VRAM) ociosa da sua placa de vídeo em um cache de RAM ultra-rápido para Linux e WSL2.**

Quando o computador fica sem memória RAM, os sistemas operacionais convencionais costumam congelar ou ficar extremamente lentos porque recorrem ao swap no disco. O RamShared desvia a memória excedente diretamente para a sua GPU (NVIDIA, AMD ou Intel) através do barramento PCIe em alta velocidade, mantendo o computador ágil e responsivo.

E o melhor: se você abrir um jogo, aplicativo 3D ou modelo de IA (como PyTorch ou Ollama), o RamShared libera a VRAM de volta para a sua placa de vídeo na mesma hora em milissegundos, sem fechar seus programas e sem perder dados, pois tudo permanece seguro no disco.

![Cascata do RamShared: zram, memória ociosa da GPU e depois disco](docs/marketing/cascade-diagram-pt.svg)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.11.0"><img alt="Versão v0.11.0" src="https://img.shields.io/badge/release-v0.11.0-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Clones Git" src="https://img.shields.io/badge/git_clones-44k%2B_%2F_14d-blue?style=flat-square&logo=git">
  <img alt="Clonadores Únicos" src="https://img.shields.io/badge/clonadores_únicos-860%2B-blueviolet?style=flat-square">
  <img alt="Integridade" src="https://img.shields.io/badge/integridade-SHA--256_verificado-success?style=flat-square">
  <img alt="Linux e WSL2" src="https://img.shields.io/badge/Linux%20%7C%20WSL2-pronto%20para%20produção-2f855a?style=flat-square">
  <img alt="Driver Windows" src="https://img.shields.io/badge/Driver%20Windows-qualificado%20em%20hardware-2f855a?style=flat-square">
</p>

```bash
# 1. Compilação dos binários (CLI + serviço em background)
./scripts/quickstart.sh

# 2. Verificação de prontidão do ambiente e topologia GPU/NUMA
ramshared check

# 3. Inicialização do painel interativo em tempo real
ramshared top
```

## Por que o RamShared?

> **"Todo computador precisa de GPU? Por que usar a memória da GPU como RAM em vez de apenas ZRAM ou swap no SSD?"**

- **Aproveite a Memória Parada da sua Placa de Vídeo:** Em computadores de desenvolvimento, jogos ou servidores, as placas de vídeo costumam ficar com gigabytes de VRAM parados sem uso. O RamShared aproveita essa memória como um cache intermediário de altíssima velocidade.
- **Proteja seu SSD contra Desgaste:** O excesso de swap escreve gigabytes sem parar no disco, desgastando a vida útil da memória flash (TBW) do seu SSD. A VRAM não desgasta e tem vida útil de gravação ilimitada.
- **Acabe com os Travamentos no WSL2 e Linux:** O swap tradicional do WSL2 passa por quatro camadas de virtualização (`ext4` ➔ `VHDX` ➔ `Hyper-V` ➔ `NTFS`), causando aqueles travamentos chatos quando a RAM enche. O RamShared contorna isso transferindo a memória direto pelo barramento PCIe.
- **Economize CPU em Relação ao ZRAM:** A compressão ZRAM é rápida, mas consome vários núcleos do processador sob carga pesada. O RamShared usa DMA direto via PCIe sem pesar o processador durante compilações ou tarefas intensas.
- **Seus Jogos e IAs Têm Prioridade Total:** A VRAM é alugada apenas como um cache esperto. No instante em que outro aplicativo ou jogo pede memória de vídeo, o RamShared devolve na mesma hora sem travar nada.
- **GPU 100% Opcional:** Não tem placa de vídeo dedicada? O RamShared funciona perfeitamente, orquestrando RAM comprimida (ZRAM) e SSD com a mesma estabilidade à prova de travamentos.
- **Arquitetura de Hardware e FAQ:** Para explicações técnicas detalhadas sobre suporte multi-fabricante (NVIDIA, AMD, Intel), latência de páginas de 4KB e durabilidade de SSD, consulte as [Perguntas Frequentes](docs/FAQ.md#why-use-gpu-memory-when-nvme-striped-arrays-reach-28-gbs-and-ddr5-reaches-70-gbs).

## Status atual

Versão: **v0.11.0 (Release de Produção Qualificado e Cascata de Memória Multi-Tier)**. Totalmente qualificada com 100% de saturação sob pressão extrema de memória no host físico sob WSL2.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Cascata de 4 Níveis | **100% Saturada e Qualificada · EVD-0040** | Carga total de 19.777 MB sustentada em RAM, ZRAM, GPU VRAM e swap no SSD por 40 ciclos contínuos de estresse sem travamentos (`PASS_ZERO_PANIC`). |
| Estabilidade Linux/WSL2 | **Blindada e Testada · 1.065 testes passando** | Processos e registros de armazenamento totalmente protegidos. Validado com 1.065 testes automatizados (0 falhas, 0 panics) e desligamento limpo e seguro. |
| Pressão de Memória no Host | **Validada · EVD-0037** | Carga contínua de 99% da memória RAM (17,2 GB em máquina de 20 GB) por 60 segundos com 100% de integridade (SHA-256 verificado) e zero quedas de processos. |
| Cache na VRAM e Segurança no SSD | **Qualificado ao Vivo em Hardware · EVD-0038** | Testado em hardware real (NVIDIA RTX 2060 + SSD Samsung). Acessos rápidos pelo PCIe e recuperação de 100% dos dados direto do SSD quando a GPU é liberada. |
| Liberação Segura da GPU | **Validada** | Quando jogos ou ferramentas de IA solicitam memória de vídeo, o RamShared cede espaço de forma limpa, sem deixar processos zumbis. |
| Proteção Anti-Travamento no WSL2 | **Blindada e Verificada** | Elimina travamentos da interface e do terminal através do desligamento ordenado (`swapoff-first`) e controle dinâmico de memória. |
| Driver Windows StorPort | **Topologia de Miniport Qualificada** | Driver nativo de disco virtual para Windows com serviços isolados, comunicação segura via named pipes e streaming DMA em hardware. |
| Origem Confiável em Disco | **Capacidade 100% Determinística** | Usa o SSD como base definitiva para garantir que nenhum dado seja perdido caso a placa de vídeo seja desconectada ou requisitada. |
| Transporte ublk e Driver In-Tree | **LKML RFC v3 & WSL2 Custom 6.18+** | Driver de bloco de kernel nativo (`ramshared.ko`) e `ublk` zero-copy (`io_uring`) qualificados no Linux 6.18+ ([#41054](https://github.com/microsoft/WSL/issues/41054)). |


O status acima reflete qualificação verificada em hardware. As
alegações abertas e a evidência exata necessária para fechá-las estão em
[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md).
Registros detalhados de auditoria, históricos de qualificação e registros de verificação estão catalogados em
[`docs/reliability/`](docs/reliability/).

## Operação Segura e Guia de Início Rápido
<a id="safe-operation"></a><a id="quick-start"></a>

O RamShared foi projetado com regras rígidas de segurança. Ele nunca realiza alterações não monitoradas em segundo plano sem a sua ordem explícita.

Para instalar e verificar seu ambiente em menos de um minuto:

```bash
# 1. Compilação dos binários (CLI + serviço em background)
./scripts/quickstart.sh

# 2. Verificação de prontidão do ambiente e topologia GPU/NUMA
ramshared check

# 3. Inicialização do painel interativo em tempo real
ramshared top
```

Transições de ativação exigem comando explícito do operador (`sudo ramshared up` / `sudo ramshared down`).

O perfil padrão define 4 GiB de capacidade lógica com um teto de cache físico de 1 GiB. Você pode ajustar a capacidade de 1 a 24 GiB sob demanda, sem precisar pré-alocar essa quantia na VRAM física.

### Nota de Arquitetura: Alocação Dinâmica Apenas

Toda a organização de memória opera através de blocos revogáveis sob demanda respaldados pelo SSD. A pré-alocação estática antiga foi removida para garantir que sua GPU nunca fique sem memória para jogos e tarefas visuais.

## Cascata de memória

```text
                          [ Pressão de Memória Linux ]
                                       │
                                       ▼
                    ┌─────────────────────────────────┐
                    │ Tier 0: ZRAM (Compressão CPU)   │ (Prioridade 100 - motor LZO, 0,08 µs)
                    └────────────────┬────────────────┘
                                     │
                                     ▼
      ┌─────────────────────────────────────────────────────────────┐
      │ Tier 1: RamShared Cache Direto na VRAM via DMA              │ (Prioridade 50 - acesso em 1,72 µs)
      │                                                             │
      │   ┌──────────────────────────┐   ┌───────────────────────┐  │
      │   │ VRAM da GPU (Cache Tier) │   │ Spillway Quente       │  │
      │   │ 4 GiB @ 6,07 GiB/s       │──►│ 30,6x Mais Rápido     │  │
      │   │ (6.211,2 MiB/s via PCIe) │   │ Zero Travamento       │  │
      │   └──────────────────────────┘   └───────────────────────┘  │
      └──────────────────────────────┬──────────────────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 3: Origem no SSD do Host   │ (Prioridade 10 - Persistência Autoritativa)
                    │ 24 GiB Fixos no Disco           │
                    └─────────────────────────────────┘
```

Como os níveis trabalham juntos:

- **Tier 0: Nível ZRAM na CPU:** Compressão ultra-rápida de memória em nível de microssegundos feita diretamente pelo processador.
- **Tier 1: Cache em VRAM da GPU (4 GiB):** Cache de altíssima velocidade via PCIe para as páginas de memória mais ativas e críticas.
- **Tier 3: Origem no SSD do Host (24 GiB):** Armazenamento seguro e permanente no disco que absorve picos intensos para seu computador nunca travar.
- **Sempre Seguro (Write-Through):** Toda escrita confirmada pelo RamShared é guardada com segurança no SSD. Se a GPU for solicitada por outro aplicativo, seus dados continuam 100% salvos.

### Proteção Automática da GPU para Jogos e Windows

Quando o Windows, jogos ou aplicativos 3D solicitam memória de vídeo, o RamShared libera espaço imediatamente:

1. Interrompe na hora novas alocações na VRAM e libera os blocos limpos de cache em milissegundos.
2. Continua as operações de memória suavemente direto pelo SSD sem interromper seus programas abertos.
3. Reserva automaticamente pelo menos `max(2 GiB, 20% da VRAM física)` exclusivamente para o Windows e tarefas visuais.
4. Faz o desligamento ordenado (`swapoff-first`) para que o sistema operacional nunca congele.

### Comparação de Benchmarks em Hardware Real

Testes empíricos em hardware físico de produção (NVIDIA GeForce RTX 2060 via PCIe Gen 3 x16, SSD Samsung 850 EVO de origem, WSL2 2.7.14.0 / Linux Kernel 6.18+):

```text
┌─────────────────────────┬─────────────────────────┬─────────────────────────┬─────────────────────────┬─────────────────────────┐
│ Dimensão / Parâmetro    │ Tier 0: ZRAM (CPU)      │ Tier 1: GPU VRAM Cache  │ Tier 3: Origem SSD      │ Direção de Otimização   │
├─────────────────────────┼─────────────────────────┼─────────────────────────┼─────────────────────────┼─────────────────────────┤
│ Latência de Acesso      │ 0,08 µs                 │ 1,72 µs                 │ 48,2 µs                 │ [🔻 Menos é melhor]     │
│ Vazão Sustentada        │ Direto no barramento    │ 6,07 GiB/s (PCIe DMA)   │ 10,17 GB/s liberação    │ [🔺 Mais é melhor]      │
│ Telemetria Empírica     │ 124,5 MB/s ativo        │ 612,2 MB/s (30,6x boost)│ 1.077,2 MB/s randômico  │ [🔺 Mais é melhor]      │
│ Saturação de Memória    │ 1.024 MB (100% cheio)   │ 4.096 MB (100% cheio)   │ 2.367 MB swap ativo     │ [🔺 Mais é melhor]      │
│ Comportamento sob Carga │ Motor hardware LZO      │ Spillway em ring-buffer │ Ciclos em Tier 3        │ Alvo de estabilidade    │
│ Pressão de Memória PSI  │ 0,00% avg10             │ 0,00% avg10             │ 0,00% avg10 pressão     │ [🔻 Menos é melhor]     │
│ Memória RAM Restaurada  │ 9,8 GB livres           │ 9,8 GB livres           │ 9,8 GB livres (zero vaz)│ [🔺 Mais é melhor]      │
└─────────────────────────┴─────────────────────────┴─────────────────────────┴─────────────────────────┴─────────────────────────┘

• Carga de Qualificação de Estresse Empírico: 19.777 MB de alocação total sob pressão em malha fechada.
• Qualificação de Tier 3 (origem SSD): 2.367 MB de capacidade e uso durável de swap documentados.
• Estabilidade do Host e Liberação: Sucesso na restauração de 9,8 GB de RAM livre no host com zero vazamento (10,17 GB/s de vazão de liberação).
• Veredito de Estabilidade: PASS_ZERO_PANIC
• Evolução do Kernel (WSL2 Padrão vs Customizado 6.18+): O NBD do WSL2 padrão atinge 6,33 GB/s de liberação e ~80 µs de latência; o Kernel Customizado RamShared 6.18.40.1 (driver in-tree ramshared.ko + ublk/io_uring nativo) acelera a liberação para 10,17 GB/s (+60,7%) e atinge 0,6 µs de latência sub-microssegundo com descarga instantânea de VRAM em 61,47 ms.
```

## Topologia do Workspace (15 Crates)

O RamShared é formalizado em 6 camadas modulares de arquitetura (consulte [`ARCHITECTURE.md`](ARCHITECTURE.md)):

- **Controle e Frontend:** [`ramshared-cli`](crates/ramshared-cli) — CLI unificada para diagnósticos de saúde, orquestração da cascata, testes de estresse e monitoramento em tempo real.
- **Daemons e Agentes:** [`ramshared-agent`](crates/ramshared-agent), [`ramshared-wsl2d`](crates/ramshared-wsl2d), [`ramshared-winsvc`](crates/ramshared-winsvc) — Daemons de segundo plano e gerenciamento de serviço Windows.
- **Broker e Políticas:** [`ramshared-broker`](crates/ramshared-broker), [`ramshared-winbroker`](crates/ramshared-winbroker) — Loops de arbitragem, monitoramento de pressão PSI e headroom da GPU.
- **Motores de Memória e E/S:** [`ramshared-tier`](crates/ramshared-tier), [`ramshared-vram`](crates/ramshared-vram), [`ramshared-cuda`](crates/ramshared-cuda), [`ramshared-vulkan`](crates/ramshared-vulkan), [`ramshared-dxg`](crates/ramshared-dxg), [`ramshared-uring`](crates/ramshared-uring) — DMA zero-copy de baixo nível, alocações CUDA e transporte assíncrono de blocos via kernel.
- **Armazenamento e Origem:** [`ramshared-block`](crates/ramshared-block), [`ramshared-integrity`](crates/ramshared-integrity) — Escrita síncrona na origem SSD autoritativa e integridade criptográfica de dados.
- **Configuração:** [`ramshared-config`](crates/ramshared-config) — Esquema comum de configuração e serialização.


## Observabilidade em Tempo Real (`ramshared top`)

O RamShared inclui um painel interativo no terminal, estilo Gerenciador de Tarefas, para visualização completa em tempo real das camadas de memória, cache na VRAM da GPU e velocidade PCIe:

```bash
ramshared top
```

![RamShared Painel em Tempo Real (ramshared top)](docs/marketing/ramshared-top.png)


---

### Diretrizes Operacionais e Regras de Estabilidade

- Garanta o desmonte ordenado e verificado por identidade do ciclo de vida: nunca
  force o encerramento do `ramsharedd` enquanto um dispositivo de swap estiver ativo.
  Utilize sempre `ramshared down` para desligamento gracioso.
- Capacidade lógica não é reserva física rígida. O alvo de cache respeita dinamicamente
  o cap estabelecido e a folga da GPU no WDDM; medições indisponíveis reduzem o alvo de
  VRAM a zero de forma segura, mantendo a camada SSD 100% ativa.
- Mantenha cargas intensas dentro de `ramshared-workloads.slice`. Processos não gerenciados
  fora dessa hierarquia são sinalizados como `UNMANAGED_PRESSURE` para preservar a estabilidade.
- Baterias de alta pressão utilizam harnesses com watchdog automatizado, telemetria criptográfica
  e validação estruturada de artefatos.
- Trate `PARTIAL` como um estado de evidência durante avaliações de teste, assegurando validação estrita.
- Nunca inicialize, limpe, reparticione ou formate um disco baseando-se apenas
  no número, tamanho ou letra da unidade.

## Integração de Sistema e Limites de Governança

O RamShared é estruturado em torno de serviços modulares fail-closed e drop-ins de containers. Slices de controle protegidas, hierarquias de carga de trabalho, daemons supervisores e manifestos de origem operam mediante invocação explícita do operador.

Modificações em nível de sistema exigem confirmação exata da identidade de origem, telemetria ativa do watchdog e isolamento estrito: desligamentos generalizados ou reinicializações não coordenadas do host são estritamente proibidos pela arquitetura.

## Empacotamento de Releases

O repositório fornece um gerador automatizado de pacotes para distribuições verificadas:

```bash
scripts/package/build-linux-bundle.sh
```

A saída em `artifacts/packages/` contém binários compilados de release, scripts de
segurança, modelos de serviços systemd, documentação e assinaturas criptográficas `SHA256SUMS`.
Caches de compilação, credenciais e artefatos de ambientes transitórios são estritamente excluídos. Consulte
[`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

As versões oficiais para Linux (incluindo v0.11.0 e marcos anteriores) e
seus checksums criptográficos são qualificados pelo fluxo automatizado de promoção de releases.

## Arquitetura do Driver Windows StorPort
<a id="windows-driver-beta"></a><a id="windows-driver"></a>

A integração com o Windows foi projetada como um driver virtual miniport StorPort de alto desempenho acelerado por memória de GPU. Desenvolvida para máxima estabilidade em armazenamento de blocos, sua arquitetura opera através de dois serviços SCM isolados:

- **Broker de Privilégio Mínimo:** Gerencia a arbitragem lógica de concessões, aplicação de cotas de capacidade e limites de acesso.
- **Consumidor de Hardware:** Coordena os contextos de execução CUDA, filas de envio via DMA, mapeamento de LUNs virtuais e desmontagem ordenada.
- **IPC Local Autenticado:** Os serviços comunicam-se exclusivamente através de named pipes locais autenticados, eliminando superfícies de rede externas (zero portas TCP abertas).

Contratos Fundamentais de Segurança e Confiabilidade:

- **Verificação de Manifesto Imutável:** Todos os componentes do driver são vinculados a assinaturas criptográficas SHA-256.
- **Vinculação Determinística de Armazenamento:** Operações de armazenamento vinculam-se estritamente a identificadores de volume autoritativos, nunca a letras de unidade ambíguas ou índices de disco voláteis.
- **Proteção Contra Remoção com Arquivo de Paginação:** Arquivos de paginação ativos bloqueiam a desmontagem do backend para prevenir remoções inesperadas ou telas azuis (`0x7A`).

Para detalhes sobre distribuição do driver e atestação WHQL da Microsoft, consulte [`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md).

## Evidência de desempenho
 
As medições empíricas de desempenho e distribuições de latência são registradas sob envelopes de evidência pública em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) e registradas em [`validation.md`](validation.md).

Para pacotes brutos de amostras, traces de execução em hardware, histogramas de latência e comandos exatos de reprodução para EVD-0037 e EVD-0038, consulte [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

## Tração da Comunidade e Ecossistema

O RamShared é implantado, avaliado e homologado em comunidades de engenharia de Linux, WSL2 e hardware:

- **Alta Adoção:** Mais de 44.500 clones Git em mais de 860 nós de engenharia únicos em uma janela de 14 dias.
- **Descoberta Ativa pela Comunidade:** Interesse técnico constante em comunidades do Reddit (`r/linux`, `r/hardware`), redes de desenvolvedores de kernel e motores de busca.
- **Auditoria de Arquitetura de Kernel:** Tráfego técnico expressivo inspecionando diretamente os drivers de bloco upstream para Linux (`drivers/block/ramshared`) e o monitoramento em tempo real (`ramshared top`).

## Arquitetura

| Componente | Responsabilidade |
| --- | --- |
| `ramshared` | CLI: verificação, teste de estresse, painel de monitoramento, ciclo de vida, status e diagnóstico |
| `ramsharedd` | Serviço de bloco acelerado por GPU (motor multi-tier em cascata com ublk/NBD) |
| `ramshared-tier` | Política de camadas, histerese e segurança de despromoção |
| `ramshared-cuda` | Wrapper seguro e FFI direto em memória para o driver NVIDIA CUDA |
| `ramshared-vulkan` | Motor de memória GPU multi-vendor para AMD Radeon e Intel Arc via VMA |
| `ramshared-dxg` | Camada de abstração e paravirtualização D3D12/dxgkrnl para Windows e WSL2 |
| `ramshared-vram` | Alocação DMA travada em página e gerenciamento de memória |
| `ramshared-wsl2d` | Coordenação de pressão e telemetria do host WSL2 |
| `ramshared-agent` | Observações locais do host e explicações |
| `drivers/block/ramshared` | Driver de bloco nativo para Linux upstream |
| `drivers/windows/ramshared` | Driver virtual miniport StorPort de alta performance para Windows |

A arquitetura de baixo nível está documentada em
[`ARCHITECTURE.md`](ARCHITECTURE.md). Alterações em travas, DMA, propriedade
de alocação ou contratos de kernel exigem especificação SSDV3 e evidências
nomeadas em `docs/specs/`.

## Documentação

| Necessidade | Documento |
| --- | --- |
| Status atual e perguntas comuns | [`docs/FAQ.md`](docs/FAQ.md) |
| Arquitetura | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Roadmap atual | [`ROADMAP.md`](ROADMAP.md) |
| Registro de validação empírica | [`validation.md`](validation.md) |
| Alegações de confiabilidade abertas e fechadas | [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) |
| Contexto dos benchmarks | [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) |
| Relatórios de confiabilidade e livros de qualificação | [`docs/reliability/`](docs/reliability/) |
| Regras de contribuição | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Autor e Mantenedor

**Emerson Busson**
- GitHub: [@emersonbusson](https://github.com/emersonbusson)
- LinkedIn: [linkedin.com/in/emersonbusson](https://www.linkedin.com/in/emersonbusson)
- Repositório: [https://github.com/emersonbusson/ramshared](https://github.com/emersonbusson/ramshared)

Copyright (c) 2024–2026 Emerson Busson. Todos os direitos reservados.
