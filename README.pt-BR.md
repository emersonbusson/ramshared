# RamShared

Idioma: [English](README.md)

> Esta tradução é informativa e não normativa. O [`README.md`](README.md) em
> inglês é a fonte canônica para requisitos técnicos e limites de segurança.

**O RamShared é um projeto estável de camadas de memória para Linux e WSL2. Ele pode usar VRAM ociosa como cache revogável, junto com ZRAM e disco.**

O projeto é destinado a quem quer operar ou estudar camadas de memória aceleradas por GPU na própria máquina. Ele observa pressão, mantém uma origem em disco e devolve VRAM quando a GPU precisa dela. O resultado ainda depende do hardware, do driver e da carga ativa; execute a verificação de prontidão antes de ativar qualquer camada.

![Cascata do RamShared: zram, memória ociosa da GPU e depois disco](docs/marketing/cascade-diagram-pt.svg)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.13.4"><img alt="Versão v0.13.4" src="https://img.shields.io/badge/release-v0.13.4-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Linux e WSL2" src="https://img.shields.io/badge/Linux%20%7C%20WSL2-estável-2f855a?style=flat-square">
</p>

```bash
# 1. Compilação do checkout atual (CLI + serviço em background)
./scripts/quickstart.sh

# 2. Verificação de prontidão do ambiente e topologia GPU/NUMA
./target/release/ramshared check

# 3. Inicialização do painel interativo em tempo real
./target/release/ramshared top
```

## Por que o RamShared?

> **"Todo computador precisa de GPU? Por que usar a memória da GPU como RAM em vez de apenas ZRAM ou swap no SSD?"**

- **Use VRAM ociosa com critério:** Em uma GPU compatível e com memória livre, a VRAM pode servir como cache. A quantidade disponível muda conforme jogos, desktop e cargas de IA.
- **Mantenha uma origem durável:** A VRAM não é a fonte definitiva dos dados. O desenho usa armazenamento de origem para permitir uma liberação segura do cache.
- **Trabalhe junto com ZRAM e disco:** A camada de GPU é uma opção na cascata, não substitui todas as formas de swap ou de gerenciamento de memória.
- **Mantenha o controle:** Ativação e desligamento exigem comando explícito. O projeto não inicia cargas de pressão de memória silenciosamente.
- **Leia os limites antes:** Consulte as [Perguntas Frequentes](docs/FAQ.md) e o [registro de gaps de confiabilidade](docs/reliability/GAP-REGISTER.md) antes de usar o driver Windows ou as superfícies de laboratório do kernel.

## Status atual

Última release publicada: **[v0.13.4](https://github.com/emersonbusson/ramshared/releases/tag/v0.13.4)**. Este checkout compila a versão **0.13.4**, a manutenção estável atual.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Userspace Linux e WSL2 | **Estável e qualificado** | A CLI, o daemon, as verificações e o desligamento ordenado são cobertos pela CI. Ative-os após `check` e o preflight documentado. |
| Cache de GPU | **Estável em hardware qualificado** | Os backends CUDA e Vulkan existem, mas a capacidade e o comportamento dependem do driver, GPU, desktop e pressão atual do host. |
| Origem em disco e integridade | **Estáveis e testadas** | Há verificações de integridade e desligamento; cada instalação ainda precisa validar seu próprio antes/depois. |
| Driver Windows StorPort | **Ainda não distribuível publicamente** | O driver permanece uma superfície de laboratório supervisionada até que exista assinatura confiável para produção e qualificação completa. |
| Kernel customizado e transporte ublk | **Adiados** | São superfícies de desenvolvimento e laboratório, não o transporte WSL2 padrão do primeiro dia. |


As medições históricas estão em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md). Entradas sem envelope público de evidência são registros históricos, não baselines atuais de release. Limites e qualificações em aberto estão em [`docs/reliability/`](docs/reliability/).

### Snapshot de qualificação da v0.13

A qualificação da v0.13 alcançou **19.777 MB** entre Tier 0 (ZRAM), Tier 1 (cache de VRAM da GPU) e Tier 3 (origem SSD), com veredito `PASS_ZERO_PANIC` no hardware qualificado. Isso registra a evidência da release; não é promessa de capacidade ou desempenho para outra máquina.

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

### Transição única de uma cascata legada

Se uma instalação anterior do RamShared ainda estiver em execução sem o binding
de ciclo de vida atual, não desconecte manualmente os dispositivos de swap nem
encerre o daemon. Use a transição assistida:

```bash
sudo ./target/release/ramshared migrate-cascade --from-legacy
```

O comando não aceita substituição de dispositivo ou capacidade. Ele recusa a
operação se não puder comprovar a origem selada, uma única topologia legada
ZRAM → NBD elegível, o binário correspondente do daemon (ou um daemon
substituído, de root, ainda ligado ao listener NBD esperado), margem de memória
e um guardião do host saudável. Ele drena o swap antes de resetar, desconectar
ou encerrar o daemon e então cria o binding normal de ciclo de vida selado. Uma
recusa mantém os dispositivos e as evidências existentes intactos.

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
      │ Tier 1: RamShared Cache Direto na VRAM via DMA              │ (Prioridade 50 - acesso em 0,85 µs)
      │                                                             │
      │   ┌──────────────────────────┐   ┌───────────────────────┐  │
      │   │ VRAM da GPU (Cache Tier) │   │ Spillway Quente       │  │
      │   │ 4 GiB Ativos na GPU      │──►│ 15,6x - 21,5x Rápido  │  │
      │   │ (Até 429,6 MB/s via DMA) │   │ Zero Fome no Host     │  │
      │   └──────────────────────────┘   └───────────────────────┘  │
      └──────────────────────────────┬──────────────────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 3: Origem no SSD do Host   │ (Prioridade -2 - Spillover em Cascata)
                    │ Armazenamento Durável de Origem │
                    └─────────────────────────────────┘
```

Como os níveis trabalham juntos:

- **Tier 0: ZRAM (Nível CPU, 1024 MiB):** Compressão ultra-rápida de memória em nível de microssegundos feita diretamente pelo processador.
- **Tier 1: Cache em VRAM da GPU (4 GiB Ativos na GPU):** Cache de altíssima velocidade via PCIe para as páginas ativas, configurado com capacidade total de 4.096 MB preservando a estabilidade do display.
- **Tier 3: Origem no SSD do Host:** Armazenamento seguro e permanente no disco que absorve o overflow de memória para o sistema nunca travar.
- **Sempre Seguro (Write-Through):** Toda escrita confirmada pelo RamShared é guardada com segurança no armazenamento durável. Se a GPU for solicitada por outro aplicativo, seus dados continuam 100% salvos.

### Proteção Automática da GPU para Jogos e Windows

Quando o Windows, jogos ou aplicativos 3D solicitam memória de vídeo, o RamShared libera espaço imediatamente:

1. Interrompe na hora novas alocações na VRAM e libera os blocos limpos de cache em milissegundos.
2. Continua as operações de memória suavemente direto pelo armazenamento de origem sem interromper seus programas abertos.
3. Reserva automaticamente pelo menos `max(1,5 GiB, 20% da VRAM física)` exclusivamente para o Windows e tarefas visuais (Princípio 11 do SSDV3), assegurando estabilidade ao Gerenciador de Janelas (DWM) enquanto libera 4 GiB completos em GPUs de 6GB+.
4. Faz o desligamento ordenado (`swapoff-first`) para que o sistema operacional nunca congele.

### Evidência, sem atalho de marketing

O desempenho depende da GPU, driver, carga do desktop, caminho de disco e pressão de memória da máquina testada. Um número obtido em uma RTX 2060 ou em um host WSL2 não é promessa para outro computador.

O projeto mantém registros históricos de benchmark para auditoria. Somente um resultado com envelope público atual de evidência pode ser usado como baseline de release ou alegação de regressão. Consulte [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) para o contexto das medições e [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) para os limites de qualificação restantes.

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

- **Sempre use `ramshared down` para desligar:** Nunca encerre o daemon `ramsharedd` à força com o swap montado. O desmonte ordenado (`swapoff`) mantém o Linux estável e evita corrupção de sistema de arquivos.
- **Alocação dinâmica, sem desperdício:** O RamShared só aloca memória de vídeo sob demanda. Se jogos, navegadores ou aplicativos 3D precisarem de VRAM, o RamShared devolve o espaço na hora.
- **Proteção do Gerenciador de Janelas (DWM):** Pelo menos 1,5 GB (ou 20% da VRAM) fica sempre reservado para a interface do Windows, garantindo que suas telas, janelas e cursor continuem perfeitamente fluidos.
- **Segurança total de armazenamento:** As operações em disco vinculam-se estritamente ao identificador único do volume (UUID), nunca a letras voláteis de unidade.
- **Transição legada assistida:** `migrate-cascade --from-legacy` é o único caminho suportado para sair de uma cascata anterior sem binding; não é recuperação automática.

## Integração de Sistema e Segurança

O RamShared opera como um serviço limpo e independente no espaço de usuário com integração ao systemd:
- Nenhuma ação em segundo plano é executada sem o seu comando (`ramshared up` / `ramshared down`).
- Partições de armazenamento são validadas por UUID exato, nunca por letras voláteis de drive.
- Reinicializações ou desligamentos de máquina nunca são disparados automaticamente. Você está sempre no controle total do seu computador.

## Empacotamento de Releases

O repositório fornece um gerador automatizado de pacotes para distribuições verificadas:

```bash
scripts/package/build-linux-bundle.sh
```

A saída em `artifacts/packages/` contém binários compilados de release, scripts de
segurança, modelos de serviços systemd, documentação e assinaturas criptográficas `SHA256SUMS`.
Caches de compilação, credenciais e artefatos de ambientes transitórios são estritamente excluídos. Consulte
[`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

As versões oficiais para Linux (incluindo v0.13.4 e marcos anteriores) e
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
| [`drivers/block/ramshared`](drivers/block/ramshared/README.md) | Driver de bloco nativo para Linux upstream |
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
