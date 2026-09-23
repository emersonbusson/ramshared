# RamShared

Idioma: [English](README.md)

> Esta tradução é informativa e não normativa. O [`README.md`](README.md) em
> inglês é a fonte canônica para requisitos técnicos e limites de segurança.

**O RamShared é um projeto estável de camadas de memória para Linux e WSL2. Ele pode usar VRAM ociosa como cache revogável, junto com ZRAM e disco.**

O projeto é destinado a quem quer operar ou estudar camadas de memória aceleradas por GPU na própria máquina. Ele observa pressão, mantém uma origem em disco e devolve VRAM quando a GPU precisa dela. O resultado ainda depende do hardware, do driver e da carga ativa; execute a verificação de prontidão antes de ativar qualquer camada.

![Cascata do RamShared: zram, memória ociosa da GPU e depois disco](docs/marketing/cascade-diagram-pt.svg)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.14.1"><img alt="Versão v0.14.1" src="https://img.shields.io/badge/release-v0.14.1-2f855a?style=flat-square"></a>
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

Última release publicada: **[v0.14.1](https://github.com/emersonbusson/ramshared/releases/tag/v0.14.1)**. Este checkout compila a versão **0.14.1**, a manutenção estável atual.

O WSL2 padrão usa **NBD como transporte base**. `ublk`/`io_uring` é qualificado
no Linux nativo ou no WSL2 com kernel customizado compatível; não é uma base
universal para kernels WSL2 padrão.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Userspace Linux e WSL2 | **Estável e qualificado** | A CLI, o daemon, as verificações e o desligamento ordenado são cobertos pela CI. Ative-os após `check` e o preflight documentado. |
| Cache de GPU | **Estável em hardware qualificado** | Os backends CUDA e Vulkan existem, mas a capacidade e o comportamento dependem do driver, GPU, desktop e pressão atual do host. |
| Origem em disco e integridade | **Estáveis e testadas** | Há verificações de integridade e desligamento; cada instalação ainda precisa validar seu próprio antes/depois. |
| Driver Windows StorPort | **Ainda não distribuível publicamente** | O driver permanece uma superfície de laboratório supervisionada até que exista assinatura confiável para produção e qualificação completa. |
| Kernel customizado e transporte `ublk` | **Qualificados em superfície limitada; promoção de produto adiada** | EVD-0039 cobre Linux nativo e uma superfície WSL2 com kernel customizado compatível. O WSL2 padrão continua usando NBD enquanto a qualificação de ciclo de vida permanece aberta. |


As medições históricas estão em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md). Entradas sem envelope público de evidência são registros históricos, não baselines atuais de release. Limites e qualificações em aberto estão em [`docs/reliability/`](docs/reliability/).

### Snapshot de qualificação da v0.13

A qualificação da v0.13 alcançou **19.777 MB** entre Tier 0 (ZRAM), Tier 1 (cache de VRAM da GPU) e Tier 3 (origem SSD), com veredito `PASS_ZERO_PANIC` no hardware qualificado. Isso registra a evidência da release; não é promessa de capacidade ou desempenho para outra máquina.

## Operação Segura e Guia de Início Rápido
<a id="safe-operation"></a><a id="quick-start"></a>

O RamShared usa padrões rígidos de segurança e não ativa a cascata sem comando explícito do operador.

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
substituído, de root, ainda ligado ao listener NBD esperado, ou o caminho
legado fixo de root com o mesmo SHA-256), margem de memória e um guardião do
host saudável. Ele drena o swap antes de resetar, desconectar
ou encerrar o daemon e então cria o binding normal de ciclo de vida selado. Uma
recusa mantém os dispositivos e as evidências existentes intactos.

O perfil padrão define 4 GiB de capacidade lógica com um teto de cache físico de 1 GiB. Você pode ajustar a capacidade de 1 a 24 GiB sob demanda, sem precisar pré-alocar essa quantia na VRAM física.

### Nota de Arquitetura: Alocação Dinâmica Apenas

Toda a organização de memória opera através de blocos revogáveis sob demanda respaldados pelo SSD. A pré-alocação estática antiga foi removida; a capacidade disponível ainda depende da GPU, do driver e da carga ativa.

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
      │ Tier 1: dispositivo lógico RamShared (Prioridade 50)        │
      │   cache VRAM limpo e revogável + origem SSD autoritativa    │
      └──────────────────────────────┬──────────────────────────────┘
                                     │
                                     ▼
                    ┌─────────────────────────────────┐
                    │ Tier 3: Origem no SSD do Host   │ (Prioridade -2 - Spillover em Cascata)
                    │ Armazenamento Durável de Origem │
                    └─────────────────────────────────┘
```

Como os níveis trabalham juntos:

- **Tier 0: ZRAM:** A memória comprimida do host é a primeira proteção sob pressão.
- **Tier 1: dispositivo lógico RamShared:** Um cache VRAM limpo e revogável pode acelerar páginas cuja cópia autoritativa está na origem SSD.
- **Tier 3: SSD do host e swap do WSL:** Os níveis inferiores recebem tráfego quando o cache não consegue admitir ou reter uma página.
- **Contrato write-through:** Uma escrita confirmada pelo cache de origem é persistida na origem autoritativa antes da mutação do cache. Falhas operacionais continuam possíveis e são registradas no registro de gaps.

A reserva varia deliberadamente por superfície. O broker/NBD mantém
`max(1536 MiB, 20% da VRAM física)` como reserva de capacidade e preserva,
separadamente, `768 MiB` da VRAM livre reportada como buffer de runtime. O
cache de origem usa `max(2 GiB, 20%)`; o StorPort usa
`max(reserva configurada, 512 MiB, 10%)`. Os valores não são intercambiáveis:
a reserva de capacidade limita o alvo do cache, enquanto o buffer de runtime
protege novas alocações contra mudanças no uso externo da GPU.

### Proteção Automática da GPU para Jogos e Windows

Quando o Windows, jogos ou aplicativos 3D solicitam memória de vídeo, o governador do RamShared tenta reduzir a pressão do cache:

1. Interrompe novas admissões no cache quando o orçamento medido cruza o limite configurado.
2. Descarta blocos limpos e atende falhas de cache pela origem autoritativa.
3. Aplica a reserva de capacidade do broker/NBD e o buffer de runtime descritos acima.
4. Usa desligamento ordenado (`swapoff-first`); timeout ou estado incerto falha de modo fechado e permanece visível ao operador.

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
- **Alocação dinâmica:** O RamShared aloca blocos de cache sob demanda e libera blocos limpos quando a pressão medida exige; a latência depende da carga e do driver.
- **Margem para o desktop:** A capacidade do broker/NBD é limitada por `max(1536 MiB, 20%)`, com buffer livre de runtime separado de `768 MiB` quando há telemetria ao vivo.
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

As versões oficiais para Linux (incluindo v0.14.1 e marcos anteriores) e
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
