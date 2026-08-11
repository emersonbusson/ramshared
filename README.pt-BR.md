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
  <a href="https://github.com/emersonbusson/ramshared/releases/tag/v0.7.4"><img alt="Versão v0.7.4" src="https://img.shields.io/badge/release-v0.7.4-2f855a?style=flat-square"></a>
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-black?style=flat-square&logo=rust&logoColor=white">
  <img alt="Caminho de produto Linux e WSL2" src="https://img.shields.io/badge/product-Linux%20%7C%20WSL2-2563eb?style=flat-square">
  <img alt="Beta supervisionado do driver Windows" src="https://img.shields.io/badge/Windows%20driver-supervised%20beta-d97706?style=flat-square">
</p>

## Status atual

Versão: **v0.7.4**, publicada em 24/07/2026.

| Superfície | Status | O que isso significa |
| --- | --- | --- |
| Cascata Linux/WSL2 | **Caminho de produto** | CLI, camada CUDA/NBD, cascata zram/disco, diagnósticos e integração opcional de boot com systemd estão implementados. |
| Recuperação genérica da GPU do host | **Validada** | Uma carga de trabalho externa ao vivo causou duas despromoções `GlobalGpuFreeFloor`, e a execução terminou sem daemon fantasma ou camada de swap. |
| Campanha de congelamento do WSL2 | **Validada** | Duas rodadas supervisionadas antes/ação/depois terminaram com watchdog, correspondência binária, telemetria de integridade e estado final limpo. |
| Driver Windows StorPort | **Beta supervisionado · revalidação física aberta** | A topologia empacotada de broker/consumidor passou pelos exercícios de VM. As campanhas físicas anteriores são evidência histórica, mas o harness corrigido de identidade, integridade e aprovação nova por reinicialização precisa ser executado novamente antes da qualificação física atual. Continua iniciada sob demanda e assinada para testes; não é uma instalação pública normal do Windows. |
| Matriz de recuperação em GiB | **Validada** | As linhas WSL2 de 1 GiB e 4 GiB e a linha calibrada Windows de 1 GiB + WSL2 de 3 GiB passaram pelos gates de integridade, recuperação e desmontagem limpa. |
| Transporte ublk de kernel personalizado | **Pesquisa adiada** | NBD continua sendo o transporte WSL2 do primeiro dia. |

O status acima é intencionalmente mais restrito que a arquitetura. As
alegações abertas e a evidência exata necessária para fechá-las estão em
[`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md).
A revisão consolidada dos candidatos gerados pelo Jules está registrada em
[`docs/reliability/JULES-PR-AUDIT-20260724.md`](docs/reliability/JULES-PR-AUDIT-20260724.md).

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
memória ociosa da GPU (camada elástica)
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

A autorização root é necessária somente na fronteira do dispositivo e do swap.

## Integração de boot opcional

O WSL2 precisa do systemd habilitado em `/etc/wsl.conf`. Depois de alterar essa
configuração, execute `wsl --shutdown` uma vez no Windows.

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

A unidade executa o preflight antes da inicialização e usa o caminho ordenado
`down` ao parar. Remova-a com:

```bash
sudo bash scripts/safety/uninstall-cascade-boot.sh
```

## Pacote instalável

Monte o pacote de release com:

```bash
scripts/package/build-linux-bundle.sh
```

A saída em `artifacts/packages/` contém binários de release, scripts de
segurança, templates do systemd, documentação e `SHA256SUMS`. Caches de build,
credenciais, notas locais de VM e artefatos do driver Windows são excluídos.
Consulte [`docs/packaging/INSTALLABLES.md`](docs/packaging/INSTALLABLES.md).

O pacote Linux oficial v0.7.4 e seu checksum separado estão disponíveis na
[página de release v0.7.4](https://github.com/emersonbusson/ramshared/releases/tag/v0.7.4).

## Beta do driver Windows

O caminho Windows é um miniport virtual StorPort apoiado por memória da GPU.
Seus exercícios em VM passam; a qualificação corrigida no host físico aguarda
uma nova campanha aprovada. A implantação continua sendo um fluxo beta elevado
e supervisionado.

A topologia instalada tem dois serviços SCM:

- `RamSharedBroker` é executado como `NT SERVICE\RamSharedBroker` e cuida
  somente da arbitragem de leases lógicos;
- `RamSharedWinSvc` é executado como LocalSystem, depende do broker e cuida de
  CUDA, fila, LUN e desmontagem segura;
- a fronteira diária entre eles é o named pipe local autenticado
  `\\.\pipe\RamSharedBroker.v1`; não há listener TCP diário instalado;
- ambos começam sob demanda por padrão e são alternados como um único
  manifesto de produto imutável e validado por SHA-256.

Limites importantes:

- use uma VM descartável para o desenvolvimento rotineiro do driver;
- use um host físico somente para uma campanha aprovada explicitamente;
- verifique a correspondência entre o pacote assinado e o binário em execução
  antes de coletar evidências;
- recuse a instalação se a letra de volume temporário pertencente ao manifesto
  já estiver presente; nunca remapeie um volume existente do host;
- monte o LUN temporário em um diretório privado quando possível, em vez de
  usar uma letra de unidade persistente do Explorer;
- formate somente uma identidade exata `RAMSHARE VRAMDISK` que também coincida
  com o tamanho esperado e o proprietário da campanha atual;
- nunca use `Clear-Disk`, seleção ampla por número de disco ou fallback para
  disco físico;
- drene qualquer pagefile antes da desmontagem do backend; a remoção surpresa
  pode causar o bugcheck `0x7A` do Windows.

A matriz calibrada de recuperação em GiB está fechada no host RTX 2060 testado.
A distribuição pública para Windows continua condicionada a um pacote com
assinatura confiável para produção ou atestada pela Microsoft. Pacotes de
laboratório assinados para testes não são releases públicos; consulte
[`docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md`](docs/packaging/WINDOWS-DRIVER-DISTRIBUTION.md).
As etapas operacionais de instalação, rollback e recuperação estão em
[`docs/runbooks/windows-autonomous-broker.md`](docs/runbooks/windows-autonomous-broker.md).

## Evidências de desempenho

O desempenho depende do transporte, da carga de trabalho, da profundidade da
fila, da contenção do host e da pressão da GPU. O projeto registra essas
condições com cada resultado, em vez de publicar uma velocidade universal.

Medições históricas representativas na estação de trabalho do projeto:

| Caminho | Leitura | Escrita | Escopo |
| --- | ---: | ---: | --- |
| Windows StorPort, LUN de 64 MiB | ~1942 MB/s | ~420 MB/s | Carga direta de 50 MiB, host ocioso, julho de 2026 |
| Loopback NBD do WSL2 | ~714 MB/s | ~597 MB/s | Comparação histórica direta de I/O de bloco |

![Comparação histórica de I/O direto de bloco](docs/marketing/benchmark-wsl2-vs-storport.jpg)

Estas são observações específicas do ambiente, não garantias mínimas. O
contexto e as ressalvas da fonte estão em [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md)
e em [`validation.md`](validation.md).

## Arquitetura

| Componente | Responsabilidade |
| --- | --- |
| `ramshared` | CLI: preflight, ciclo de vida, status, doctor e diagnóstico |
| `ramsharedd` | Serviço de bloco apoiado pela GPU |
| `ramshared-tier` | Política de camadas e segurança de despromoção |
| `ramshared-cuda` | Wrapper seguro na fronteira NVIDIA/CUDA |
| `ramshared-wsl2d` | Coordenação da pressão do host WSL2 e telemetria |
| `ramshared-agent` | Observações e explicações locais do host |
| `drivers/windows/ramshared` | Beta supervisionado do StorPort Windows |

A arquitetura de baixo nível está documentada em [`ARCHITECTURE.md`](ARCHITECTURE.md).
Alterações em locks, DMA, propriedade de alocações ou contratos de kernel
exigem especificação SSDV3 e evidência nomeada em `docs/specs/`.

## Documentação

| Necessidade | Documento |
| --- | --- |
| Instalação e perguntas comuns | [`docs/FAQ.md`](docs/FAQ.md) |
| Arquitetura | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Roadmap atual | [`ROADMAP.md`](ROADMAP.md) |
| Log de validação empírica | [`validation.md`](validation.md) |
| Alegações de confiabilidade abertas e fechadas | [`docs/reliability/GAP-REGISTER.md`](docs/reliability/GAP-REGISTER.md) |
| Contexto de benchmarks | [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) |
| Acesso a VMs de laboratório e política de inventário | [`docs/labs/HYPERV-VM-ACCESS.md`](docs/labs/HYPERV-VM-ACCESS.md) |
| Regras de contribuição | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
