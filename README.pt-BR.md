# RamShared

Idioma: [English](README.md)

> Esta tradução é informativa e não normativa. O [`README.md`](README.md) em
> inglês é a fonte canônica para requisitos técnicos e limites de segurança.

O RamShared transforma VRAM NVIDIA ociosa em uma camada elástica de memória
para Linux e WSL2. Ele coloca primeiro a RAM comprimida, depois o swap apoiado
pela GPU e, por último, o swap em disco. Quando a GPU precisa recuperar seu
orçamento, o RamShared interrompe a promoção, drena a camada da GPU e libera a
alocação.

Ele não é VRAM adicional para jogos e não inspeciona nomes de aplicativos. Um
jogo, renderizador, navegador, editor de vídeo ou tarefa de computação é apenas
uma carga de trabalho externa da GPU. As decisões de recuperação usam sinais
agregados de orçamento da GPU, memória livre e latência.

![Cascata do RamShared: zram, memória ociosa da GPU e depois disco](docs/marketing/cascade-diagram.png)

<p align="center">
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.9.0-beta.1"><img alt="Versão v0.9.0-beta.1" src="https://img.shields.io/badge/release-v0.9.0--beta.1-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Caminho de produto Linux e WSL2" src="https://img.shields.io/badge/product-Linux%20%7C%20WSL2-2563eb?style=flat-square">
  <img alt="Beta supervisionado do driver Windows" src="https://img.shields.io/badge/Windows%20driver-supervised%20beta-d97706?style=flat-square">
</p>

## Status atual

Versão: **v0.9.0-beta.1**, validada em WSL2 Linux 6.6 / 6.18 e Windows 11.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Cascata Linux/WSL2 | **Caminho de produto (v0.9.0-beta.1)** | CLI, camada CUDA/NBD, cascata zram/disco, diagnósticos e integração opcional de boot com systemd estão validados. |
| Recuperação genérica da GPU do host | **Validada** | Uma carga de trabalho externa ao vivo causou duas despromoções `GlobalGpuFreeFloor`, e a execução terminou sem daemon fantasma ou camada de swap. |
| Campanha de congelamento do WSL2 | **Validada** | Rodadas supervisionadas antes/ação/depois terminaram com watchdog, correspondência binária, telemetria de integridade e estado final limpo. |
| Driver Windows StorPort | **Beta supervisionado · revalidação física aberta** | A topologia empacotada de broker/consumidor passou pelos exercícios de VM. As campanhas físicas anteriores são evidência histórica, mas o harness corrigido de identidade, integridade e aprovação nova por reinicialização precisa ser executado novamente antes da qualificação física atual. Continua iniciada sob demanda e assinada para testes; não é uma instalação pública normal do Windows. |
| Matriz de recuperação em GiB | **Validada** | As linhas WSL2 de 1 GiB e 4 GiB e a linha calibrada Windows de 1 GiB + WSL2 de 3 GiB passaram pelos gates de integridade, recuperação e desmontagem limpa. |
| Transporte ublk de kernel personalizado | **Validado no upstream ([#41054](https://github.com/microsoft/WSL/issues/41054))** | Contribuição oficial para o kernel WSL submetida com compilações bi-arquitetura (x86_64 / ARM64), zero diagnósticos W=1, validação Sparse C=2, provas em QEMU e branch de fork testada. |

O status acima é intencionalmente mais restrito que a arquitetura. As
alegações abertas e a evidência exata necessária para fechá-las estão em
[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md).
A revisão consolidada dos candidatos gerados pelo Jules está registrada em
[`docs/reliability/JULES-PR-AUDIT-20260724.md`](docs/reliability/JULES-PR-AUDIT-20260724.md).

## Por que VRAM como Swap no WSL2?

No WSL2, o swap em disco virtual (`ext4 → VHDX → Hyper-V → Windows NTFS`) introduz
alto overhead de virtualização:

- **Swap em Disco VHDX do WSL2 (4KB QD1 randread p50):** **~2.114 µs (~2,1 ms)**
- **Swap em VRAM RamShared NBD (4KB QD1 randread p50):** **~326 µs** (6,5× mais rápido)
- **Swap em VRAM RamShared ublk direto io_uring (4KB QD1 randread p50):** **~8 µs ± 2 µs** (264× mais rápido)

Como as faltas de página (*page faults*) de swap-in são síncronas, essa **redução de 3× a 10× na latência**
elimina os travamentos severos de desktop e terminal que ocorrem quando cargas pesadas de memória
ultrapassam a RAM física do computador.

## Acelerando IA Local e Cargas Pesadas

O RamShared fornece um colchão elástico de memória para fluxos de desenvolvimento intensivos:

- **LLMs Locais e Ollama:** Execute modelos com maior número de parâmetros (8B / 14B / 32B) sem erros de falta de memória (OOM) quando o contexto se expande.
- **Cargas em PyTorch e CUDA:** Mantenha tensores e estados de gradiente em memória GPU com recuperação instantânea.
- **Stacks Multi-container no Docker:** Evite que containers sejam mortos por OOM-killer durante compilações paralelas e orquestrações de microsserviços.
- **Frameworks de Agentes de IA:** Integração nativa e camadas de armazenamento de baixíssima latência para ferramentas como AgentENV e OverlayBD.

## Início rápido

Requisitos:

- Linux ou WSL2 com uma GPU NVIDIA visível por meio de `nvidia-smi`
- Toolchain Rust
- Acesso `sudo` para operações de ciclo de vida do dispositivo de bloco e do swap

```bash
./scripts/quickstart.sh

sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
./target/release/ramshared status
```

Comece com uma alocação limitada, como 1024 MiB. Mantenha VRAM suficiente
disponível para o desktop e para outras cargas de trabalho da GPU.

Pare pelo ciclo de vida do produto, nunca matando o daemon:

```bash
sudo ./target/release/ramshared down
```

`down` desativa o swap apoiado pela GPU antes de parar o daemon. Essa ordem é
uma fronteira de integridade dos dados.

Se o preflight bloquear a inicialização:

```bash
sudo ./target/release/ramshared doctor
./target/release/ramshared status --json
```

A telemetria JSONL capturada pode ser explicada localmente sem ser enviada a
um serviço externo:

```bash
./target/release/ramshared diagnose --events /path/to/telemetry.jsonl
./target/release/ramshared diagnose --events /path/to/telemetry.jsonl --json
```

## Cascata de memória

```text
pressão de memória
    |
    v
zram (RAM do sistema comprimida)
    |
    v
memória ociosa da GPU (camada elástica: NBD ou ublk)
    |
    v
swap em disco (fallback durável)
```

O plano de controle observa a folga da GPU e a latência das operações. Quando
o host Windows ou outra carga de trabalho da GPU reduz o orçamento disponível,
o RamShared:

1. interrompe a promoção de páginas para a camada da GPU;
2. executa uma drenagem limitada do swap apoiado pela GPU;
3. deixa as páginas em zram ou no swap em disco;
4. libera a alocação CUDA;
5. registra a transição e o motivo na telemetria.

O WDDM do Windows continua sendo a autoridade no WSL2. O RamShared reage à
pressão visível no host; não promete que abrir um aplicativo específico libere
instantânea ou seguramente uma quantidade fixa de VRAM.

## Operação segura

- Use `ramshared up` e `ramshared down`; não force o encerramento de
  `ramsharedd` enquanto o dispositivo de swap estiver ativo.
- Não aloque toda a capacidade física da GPU. Uma placa de 6 GiB não pode
  hospedar com segurança proprietários de 4 GiB + 1 GiB, mais uma reserva de
  1 GiB e o uso normal do desktop.
- Execute campanhas de pressão destrutivas somente pelos harnesses de
  watchdog supervisionados, com aprovação explícita e captura de artefatos.
- Trate `PARTIAL` como um estado de evidência, não como falha de teste nem como
  alegação de release.
- Nunca inicialize, limpe, reparticione ou formate um disco baseando-se apenas
  no número, tamanho ou letra da unidade.

## Controle do desktop

No WSLg ou no Linux desktop:

```bash
bash scripts/safety/install-cascade-app.sh
./scripts/safety/cascade-app.sh --gui
```

O mesmo ciclo de vida está disponível sem a interface gráfica:

```bash
./scripts/safety/cascade-app.sh status
sudo ./scripts/safety/cascade-app.sh start
sudo ./scripts/safety/cascade-app.sh stop
```

A autorização root é necessária apenas na fronteira do dispositivo e do swap.

## Integração opcional no boot

O WSL2 precisa do systemd habilitado em `/etc/wsl.conf`. Depois de alterar essa
configuração, execute `wsl --shutdown` uma vez a partir do Windows.

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

A unidade executa o preflight antes da inicialização e usa o caminho ordenado
`down` na parada. Remova-a com:

```bash
sudo bash scripts/safety/uninstall-cascade-boot.sh
```

## Pacote instalável

Gere o pacote de release com:

```bash
scripts/package/build-linux-bundle.sh
```

A saída em `artifacts/packages/` contém os binários de release, scripts de
segurança, modelos do systemd, documentação e `SHA256SUMS`. Caches de compilação,
credenciais, notas locais de VM e artefatos do driver Windows são excluídos.
Consulte [`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

O pacote Linux oficial da v0.9.0-beta.1 e seu checksum desanexado são qualificados
pelo fluxo de promoção de release.

## Beta do driver Windows

O caminho do Windows é um miniport virtual StorPort apoiado pela memória da GPU.
Seus exercícios em VM passam; a qualificação corrigida no host físico depende de
uma campanha recém-aprovada. A implantação continua sendo um fluxo de trabalho
beta elevado e supervisionado.

A topologia instalada tem dois serviços SCM:

- `RamSharedBroker` é executado como `NT SERVICE\RamSharedBroker` e possui apenas
  a arbitragem lógica de concessões;
- `RamSharedWinSvc` é executado como LocalSystem, depende do broker e possui
  CUDA, fila, LUN e desmontagem segura;
- sua fronteira diária é o pipe nomeado local autenticado
  `\\.\pipe\RamSharedBroker.v1`; nenhum listener TCP diário é instalado;
- ambos são iniciados sob demanda por padrão e são comutados como um único
  manifesto de produto imutável e validado por SHA-256.

Fronteiras importantes:

- use uma VM descartável para o desenvolvimento rotineiro de drivers;
- use um host físico apenas para uma campanha explicitamente aprovada;
- verifique a correspondência entre o pacote assinado e o binário em execução
  antes de coletar provas;
- recuse a instalação se a letra do volume temporário pertencente ao manifesto
  já estiver presente; nunca remapeie um volume existente do host;
- monte o LUN temporário em um diretório privado quando possível, não em uma
  letra de unidade persistente do Explorer;
- formate apenas uma identidade exata `RAMSHARE VRAMDISK` que também coincida com
  o tamanho esperado e com o proprietário da campanha atual;
- nunca use `Clear-Disk`, seleção ampla por número de disco ou lógica de
  fallback para disco físico;
- drene qualquer pagefile antes da desmontagem do backend; a remoção surpresa
  pode causar o bugcheck do Windows `0x7A`.

A matriz calibrada de recuperação em GiB está fechada no host RTX 2060 testado.
A distribuição pública para Windows continua dependendo de um pacote confiável
de produção ou com atestação da Microsoft. Pacotes de laboratório assinados para
teste não são releases públicas; consulte
[`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md).
As etapas operacionais de instalação, reversão e recuperação estão em
[`docs/runbooks/windows-autonomous-broker.md`](docs/runbooks/windows-autonomous-broker.md).

## Evidência de desempenho

O desempenho depende do transporte, da carga de trabalho, da profundidade de
fila, da contenção do host e da pressão da GPU. O projeto registra essas
condições em cada resultado, em vez de publicar uma velocidade única universal.

![Benchmarks de Desempenho e Latência do RamShared no WSL2](docs/marketing/benchmark-comparison.svg)

Medições representativas na estação de trabalho do projeto (NVIDIA RTX 2060 6GB, WSL2 Linux):

| Transporte / Caminho | Latência de Page Fault 4KB (p50) | Throughput (Sequencial) | Overhead de CPU por Core |
| --- | ---: | ---: | ---: |
| **Disco Virtual do WSL2 (VHDX)** | ~2.114 µs | ~3.200 MB/s (NVMe) | Baixo (DMA) |
| **RamShared NBD (MVP Dia 1)** | ~326 µs | ~2.100 MB/s | ~22% (Pilha de Sockets) |
| **RamShared ublk (io_uring)** | **~8 µs ± 2 µs** | **~9.600 MB/s** | **~4% (Ring Buffer)** |

Estas são observações específicas do ambiente, não garantias mínimas. O
contexto de origem e as ressalvas estão em
[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) e [`validation.md`](validation.md).

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
| Instalação e perguntas comuns | [`docs/FAQ.md`](docs/FAQ.md) |
| Arquitetura | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Roadmap atual | [`ROADMAP.md`](ROADMAP.md) |
| Registro de validação empírica | [`validation.md`](validation.md) |
| Alegações de confiabilidade abertas e fechadas | [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) |
| Contexto dos benchmarks | [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) |
| Acesso a VMs de laboratório e política de inventário | [`docs/labs/HYPERV-VM-ACCESS.md`](docs/labs/HYPERV-VM-ACCESS.md) |
| Regras de contribuição | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
