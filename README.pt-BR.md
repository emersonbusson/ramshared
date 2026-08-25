# RamShared

Idioma: [English](README.md)

> Esta tradução é informativa e não normativa. O [`README.md`](README.md) em
> inglês é a fonte canônica para requisitos técnicos e limites de segurança.

O RamShared é uma candidata de P&D para usar VRAM NVIDIA ociosa como cache
revogável em uma camada de memória para Linux e WSL2. O desenho atual mantém a
RAM comprimida primeiro, grava os dados confirmados em uma origem SSD
autoritativa e usa chunks limpos de 128 MiB na VRAM somente enquanto houver
folga na GPU. O swap VHDX existente do WSL continua sendo o último fallback. O
projeto está sob qualificação beta ativa para Linux e WSL2: os gates ao vivo de
guardião, write-through na origem e pressão de memória foram empiricamente
validados no hardware da estação de trabalho (EVD-0037, EVD-0038). Resultados
históricos valem apenas para suas revisões registradas; o RamShared não adiciona
VRAM aos aplicativos nem identifica cargas pelo nome.

![Cascata do RamShared: zram, memória ociosa da GPU e depois disco](docs/marketing/cascade-diagram-pt.png)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.9.0-beta.1"><img alt="Versão v0.9.0-beta.1" src="https://img.shields.io/badge/release-v0.9.0--beta.1-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Clones Git" src="https://img.shields.io/badge/git_clones-1.5k%2B-blue?style=flat-square&logo=git">
  <img alt="Integridade" src="https://img.shields.io/badge/integridade-SHA--256_verificado-success?style=flat-square">
  <img alt="Beta Linux e WSL2" src="https://img.shields.io/badge/Linux%20%7C%20WSL2-incident%20gate-d97706?style=flat-square">
  <img alt="Beta supervisionado do driver Windows" src="https://img.shields.io/badge/Windows%20driver-supervised%20beta-d97706?style=flat-square">
</p>

## Status atual

Versão: **v0.9.0-beta.1**. A cascata instalada no WSL2 está temporariamente
bloqueada após o incidente de timeout do plano de controle de 20/08/2026.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Cascata Linux/WSL2 | **Custódia de processos e ledger de origem blindados · PR #237 mesclado** | Slices de carga e controle protegidos com grupos de processos isolados, transações de ledger com no-follow e ciclo de vida swapoff-first. Cobertura de linhas do slice da CLI em 91,6% (1.506/1.645 linhas). |
| Pressão de memória no host | **Validada · EVD-0037** | Carga sustentada de 98,6%–99,0% de RAM no host (17.280 MiB alocados em host de 20.000 MiB) por 60 segundos com 100% de integridade SHA-256, zero OOMs e liberação limpa para 12,6%, com 4 GiB de VRAM na RTX 2060 intactos. |
| Cache VRAM write-through e origem SSD | **Qualificado ao vivo · EVD-0038** | Qualificação ao vivo na RTX 2060 e origem VHDX em Samsung SSD 850 EVO. Verificada durabilidade de escrita síncrona, aceleração de cache na VRAM via PCIe e recuperação de 100% dos bytes direto do SSD sem corrupção após revogação da GPU. |
| Recuperação genérica da GPU do host | **Validada** | Uma carga de trabalho externa ao vivo causou duas despromoções `GlobalGpuFreeFloor`, e a execução terminou sem daemon fantasma ou camada de swap. |
| Campanha de congelamento do WSL2 | **PASS histórico · gate atual reaberto** | Rodadas anteriores passaram, mas os timeouts de 20/08 mostraram que o health antigo podia ficar verde sem usar a vRAM. |
| Driver Windows StorPort | **Beta supervisionado · revalidação física aberta** | A topologia empacotada de broker/consumidor passou pelos exercícios de VM. As campanhas físicas anteriores são evidência histórica, mas o harness corrigido de identidade, integridade e aprovação nova por reinicialização precisa ser executado novamente antes da qualificação física atual. Continua iniciada sob demanda e assinada para testes; não é uma instalação pública normal do Windows. |
| Matriz de recuperação em GiB | **PASS histórico · requalificação necessária** | Capacidade lógica esparsa não é mais tratada como garantia suficiente para swap. |
| Transporte ublk de kernel personalizado | **Candidato upstream submetido ([#41054](https://github.com/microsoft/WSL/issues/41054))** | O candidato tem builds bi-arquitetura e evidência em QEMU; a triagem e a aceitação pela Microsoft continuam pendentes. |


O status acima é intencionalmente mais restrito que a arquitetura. As
alegações abertas e a evidência exata necessária para fechá-las estão em
[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md).
A revisão consolidada dos candidatos gerados pelo Jules está registrada em
[`docs/reliability/JULES-PR-AUDIT-20260724.md`](docs/reliability/JULES-PR-AUDIT-20260724.md).

## Por que investigar VRAM como camada no WSL2?

O caminho de disco do WSL2 atravessa ext4, VHDX, Hyper-V e NTFS. As medições empíricas no host em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) mostram que gravações diretas na VRAM via PCIe evitam a sobrecarga de virtualização do swap em disco, prevenindo os congelamentos de sistema e travamentos que ocorrem quando o swap fica saturado.

## Limite atual — staging somente desabilitado

Não há início rápido para a candidata atual. Ela não autoriza instalação de
pacote ou boot, transição de ciclo de vida, configuração/aplicação de WSL,
operação de VM, ação de armazenamento/VHDX/GPU/dispositivo ou campanha de
pressão. Resultados de testes fonte/estáticos e medições históricas não são
aprovação de ativação.

O padrão proposto pela candidata é 4 GiB de capacidade lógica com cap físico
inicial de 1 GiB. A identidade canônica futura da origem é
`/dev/disk/by-partuuid/<uuid>`; o placeholder não é uma instrução para
provisionar ou abrir um dispositivo. A capacidade lógica pode variar de 1 a
24 GiB sem pré-alocar esse valor de VRAM.

### Pré-requisito de código fechado: pré-alocação legada removida

O seletor `RAMSHARED_VRAM_PREALLOC_LEGACY` e sua composição NBD de VRAM completa
foram removidos do código executável. O teste nomeado
`legacy_preallocation_removed_before_day0_deadline`, a varredura de fonte
ativa, a cobertura com limiares do checker e a checagem de governança fecham
somente esse pré-requisito de governança do código. Os testes Rust focados, o
rustfmt e o Clippy desta worktree exata aguardam o reparo externo do Guard. O
`VramBackend` genérico continua necessário para broker, ublk e Windows; ele não
é mais selecionável como backend único do NBD.

Qualificação ao vivo, promoção de release e ativação continuam `BLOCKED` pelas
matrizes específicas do incidente e pelo rollout assistido. Todos os
gerenciadores permanecem desabilitados/somente-plano. Registros históricos de
validação podem descrever o caminho removido, mas não o tornam disponível.
Restaurá-lo não é opção de rollback.

O material histórico de monitor e telemetria somente leitura é mantido apenas
como descrição de interface: o schema v4 separa capacidade lógica, VRAM em
cache, folga da GPU, escritas SSD autoritativas, uso de swap fallback, pressão
de memória e estados do controle/guardião. Ele não autoriza coletar, escrever
ou encaminhar telemetria em um host.

## Cascata de memória

```text
pressão de memória
    |
    v
zram (RAM do sistema comprimida)
    |
    v
dispositivo lógico RamShared
    |-- origem SSD autoritativa
    `-- cache VRAM limpo e revogável
    |
    v
swap VHDX existente do WSL (último recurso)
```

O plano de controle observa a folga da GPU e a latência das operações. Quando
o host Windows ou outra carga de trabalho da GPU reduz o orçamento disponível,
o RamShared:

1. interrompe novos commits na VRAM;
2. invalida ou libera imediatamente chunks limpos;
3. continua o I/O pela origem SSD quando a GPU falha ou não pode ser medida;
4. reserva `max(2 GiB, 20% da VRAM física)` para a GPU;
5. exige `swapoff` antes de desconectar a origem, não para liberar cache limpo.

O WDDM do Windows continua sendo a autoridade no WSL2. O RamShared reage à
pressão visível no host; não promete que abrir um aplicativo específico libere
instantânea ou seguramente uma quantidade fixa de VRAM.

## Operação segura

- Mantenha a candidata atual desligada. O contrato de ciclo de vida mantido
  exige uma desconexão ordenada e verificada por identidade; nunca force o
  encerramento de `ramsharedd` enquanto um dispositivo de swap puder estar ativo.
- Capacidade lógica não é reserva física. O alvo de cache respeita o cap selado
  e a reserva da GPU; medição desconhecida reduz o alvo físico a zero.
- A evidência histórica de pressão usou harnesses de watchdog supervisionados,
  com aprovação explícita e captura de artefatos. Isso é um registro não atual,
  não um caminho executável de campanha para esta candidata desabilitada.
- Trate `PARTIAL` como um estado de evidência, não como falha de teste nem como
  alegação de release.
- Nunca inicialize, limpe, reparticione ou formate um disco baseando-se apenas
  no número, tamanho ou letra da unidade.

## Staging de desktop e boot

Nenhuma ação de controle de desktop, instalação de pacote, integração de boot,
retomada de recuperação ou desinstalação é documentada como executável agora.
A candidata pode manter definições desabilitadas para slice de controle
protegida, slices agregadas de workloads, supervisor, host gate, drop-ins de
Docker/containerd/cron, guardião e manifesto de origem, mas nada é instalado,
habilitado ou aplicado por este documento.

O pré-requisito de remoção em código acima está fechado. Uma futura aprovação
ainda exigiria qualificação nova específica do incidente, identidade de origem
selada, heartbeat novo do watchdog e autorização assistida exata. Ela deve
permanecer direcionada e fail-closed: desligamento amplo do WSL e reboot
automático do Windows não fazem parte do escopo.

## Pacote de release histórico

O repositório mantém o gerador usado pelo beta publicado:

```bash
scripts/package/build-linux-bundle.sh
```

A saída em `artifacts/packages/` contém os binários de release, scripts de
segurança, modelos do systemd, documentação e `SHA256SUMS`. Executar o gerador
não qualifica nem instala a worktree atual. Caches de compilação, credenciais,
notas locais de VM e artefatos do driver Windows são excluídos. Consulte
[`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

O pacote Linux oficial da v0.9.0-beta.1 e seu checksum desanexado são qualificados
pelo fluxo de promoção de release.

## Candidata do driver Windows

A candidata Windows é um miniport virtual StorPort apoiado pela memória da
GPU. Os exercícios históricos em VM passaram; a qualificação corrigida no host
físico continua aberta. A candidata está desabilitada e este documento não
fornece fluxo de implantação.

A topologia candidata modela dois serviços SCM:

- um broker de privilégio mínimo possui apenas a arbitragem lógica de concessões;
- um consumidor depende do broker e possui CUDA, fila, LUN e desmontagem segura;
- sua fronteira é um pipe nomeado local autenticado; nenhum listener TCP faz
  parte da candidata;
- ambos ficam desabilitados até um único manifesto de produto imutável,
  validado por SHA-256, e a qualificação atual.

Fronteiras importantes:

- a evidência de laboratório descartável é somente histórica;
- uma futura campanha em host físico exige aprovação explícita e correspondência
  exata entre binário/pacote assinado e manifesto;
- toda futura operação de armazenamento deve vincular propriedade exata, nunca
  letra de unidade, número de disco, tamanho isolado ou fallback de disco físico;
- um pagefile ativo deve bloquear a desmontagem do backend; remoção surpresa
  pode causar o bugcheck do Windows `0x7A`.

A matriz calibrada de recuperação em GiB é evidência histórica de uma estação
de trabalho sanitizada do projeto.
A distribuição pública para Windows continua dependendo de um pacote confiável
de produção ou com atestação da Microsoft. Pacotes de laboratório assinados para
teste não são releases públicas; consulte
[`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md).
Instalação, reversão e recuperação operacionais não são autorizadas enquanto a
candidata permanecer desabilitada.

## Evidência de desempenho
 
As medições empíricas de desempenho são registradas sob envelopes rigorosos de evidência pública em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) e registradas em [`validation.md`](validation.md).
 
Qualificações recentes ao vivo no hardware físico da estação de trabalho incluem:
 
- **Pressão de memória no host (EVD-0037):** Carga sustentada de 98,6%–99,0% de RAM por 60 segundos com zero encerramentos por OOM e 100% de integridade SHA-256 verificada.
- **Cache VRAM write-through e origem SSD (EVD-0038):** Qualificação ao vivo da cascata de armazenamento na RTX 2060, demonstrando leituras aceleradas via PCIe na VRAM e recuperação exata direto do SSD após revogação de contexto da GPU com zero bytes corrompidos.
- **Comparação entre camadas de armazenamento:** Avaliação empírica comparando persistência em SSD com buffer DRAM vs DRAM-less, demonstrando por que o cache em VRAM elimina travamentos de swap em ambas as classes de armazenamento.
 
Para distribuições estatísticas completas, histogramas de latência e comandos de reprodução, consulte [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

## Arquitetura

| Componente | Responsabilidade |
| --- | --- |
| `ramshared` | CLI: preflight, ciclo de vida, status, doctor e diagnóstico |
| `ramsharedd` | serviço de bloco apoiado por GPU (motor NBD e ublk) |
| `ramshared-tier` | política de camadas e segurança de despromoção |
| `ramshared-cuda` | wrapper seguro ao redor da fronteira NVIDIA/CUDA |
| `ramshared-wsl2d` | coordenação da pressão do host WSL2 e telemetria |
| `ramshared-agent` | observações locais do host e explicações |
| `drivers/windows/ramshared` | beta supervisionado do StorPort do Windows |

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
| Acesso a VMs de laboratório e política de inventário | [`docs/labs/HYPERV-VM-ACCESS.md`](docs/labs/HYPERV-VM-ACCESS.md) |
| Regras de contribuição | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
