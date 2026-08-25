# RamShared — portal em português (Brasil)

Idioma: [English README](../../README.md) · [README completo em português](../../README.pt-BR.md)

> Este portal é informativo e não substitui a documentação técnica em inglês.
> Ele encaminha para as fontes do projeto; não é um documento normativo.

Use este portal para encontrar rapidamente a orientação adequada. Comandos,
limites de segurança, estados de evidência e decisões técnicas continuam nos
documentos em inglês indicados pelos links.

## Início rápido

Para requisitos, pré-requisitos e o fluxo `check` → `status` → `monitor`,
consulte o [início rápido do README](../../README.md#quick-start) ou leia a
[tradução completa do README](../../README.pt-BR.md).

Para distinguir RAM, capacidade lógica, VRAM física em cache e a origem SSD
autoritativa, execute `ramshared monitor --compact`. O painel é somente leitura.

## Instalação

O [guia do pacote instalável](../packaging/INSTALLABLES.md) explica como
montar e verificar o bundle Linux/WSL2. Para o fluxo de desenvolvimento do
driver Windows, siga somente os procedimentos supervisionados apontados no
[README em inglês](../../README.md#windows-driver-beta).

## Operação segura

Consulte a seção de [operação segura](../../README.md#safe-operation) antes de
iniciar ou parar a cascata. O ciclo de vida deve usar os comandos do produto;
campanhas de pressão precisam do harness supervisionado e da evidência
correspondente.

## Solução de problemas

As respostas práticas e os sintomas conhecidos estão no [FAQ do
projeto](../FAQ.md). Para um diagnóstico inicial, compare o estado observado
com o [início rápido](../../README.md#quick-start) e os limites de operação
segura acima.

## Arquitetura

O [README em inglês](../../README.md#architecture) apresenta os componentes e
encaminha para a [arquitetura de baixo nível](../../ARCHITECTURE.md). Use as
especificações SSDV3 em inglês quando a mudança envolver locks, DMA,
propriedade de alocação ou contratos de kernel.

## Estado e evidência

Para alegações abertas e fechadas, consulte o [registro de lacunas de
confiabilidade](../reliability/GAP-REGISTER.md). Este portal não transforma
uma descrição em evidência de runtime nem em garantia de release.
