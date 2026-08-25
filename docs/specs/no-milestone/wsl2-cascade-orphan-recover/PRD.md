---
slug: wsl2-cascade-orphan-recover
title: "WSL2 cascade orphan detection and bound recovery"
milestone: —
issues: []
---

# PRD — Detecção de órfãos e recuperação vinculada

Status: **supersedes o auto-recover de 2026-07-10**. A enumeração live é
somente detecção. Nenhum dispositivo sem vínculo exato é alterado, inclusive
quando `/proc/swaps` informa `Used=0`.

## Problema

Um término abrupto do WSL pode deixar NBD, ublk ou zram visível sem os registros
de userspace esperados. O contrato antigo tratava `used_kb == 0` como autorização
para `swapoff`, reset e disconnect. Isso é insuficiente: a entrada ainda está
ativa, o nome pode ter sido reutilizado e a enumeração não prova propriedade.

## Requisitos

- **RF-R1:** Ler `/proc/swaps` com parser estrito que retorna `Result`. Cabeçalho,
  todas as linhas, tipos, números e unicidade são obrigatórios.
- **RF-R2:** Tratar qualquer entrada presente como swap ativo, mesmo com uso
  zero. Leitura ausente, malformada ou ambígua preserva backend e evidências.
- **RF-R3:** NBD/ublk/zram descoberto sem vínculo RamShared exato gera recusa e
  zero comandos de mutação.
- **RF-R4:** Uma mutação de lifecycle exige vínculo selado com boot ID, PID e
  início do daemon, InvocationID, socket/export, identidade do origin
  (PARTUUID/PTUUID/`dev_t`/UUID/hashes), identidade de cada device e cardinalidade
  exata.
- **RF-R5:** Antes de cada `swapoff`, reset, disconnect, delete ou parada do
  backend, reler e revalidar toda a autoridade. Reset/disconnect/delete exigem
  uma nova prova estrita de ausência do device em `/proc/swaps`.
- **RF-R6:** Device estrangeiro, duplicado, retargeted ou registro auxiliar
  divergente permanece intocado e bloqueia todo o teardown.
- **RF-R7:** Falha ou resultado incerto de `swapoff` preserva daemon, backend,
  vínculo e marcadores forenses. Mensagens “not found” só podem avançar após
  nova prova estrita de ausência.
- **RF-R8:** O caminho standalone ublk é proibido no WSL2 sem qualquer override. Em Linux
  isolado ele próprio executa swapoff-first e preserva tudo em NO-GO recuperável.

## Fluxos

### Órfão sem vínculo

1. Ler snapshot estrito.
2. Detectar device managed-looking sem vínculo selado.
3. Retornar recusa; não chamar `swapoff`, `zramctl`, `nbd-client`, STOP_DEV,
   DELETE_DEV ou sinal de daemon.

### Lifecycle vinculado

1. Abrir vínculo selado e revalidar daemon, InvocationID, socket, origin,
   registros e devices live.
2. Executar todos os `swapoff` necessários.
3. Antes de cada reset/disconnect, obter snapshot estrito fresco e provar a
   ausência exata.
4. Comprovar desaparecimento do device antes de parar o daemon.
5. Remover evidência somente depois do sucesso terminal completo.

## Fora de escopo

- Recuperação automática por forma/nome do device.
- `wsl --shutdown`, terminate ou restart como efeito do CLI.
- ublk standalone no WSL2.
- Testes em devices reais; as provas de fonte são herméticas.

## Aceitação

- Fixtures estrangeira, ambígua, retargeted, duplicada e sem vínculo executam
  zero comandos.
- Swap ativo com uso zero e falha de `swapoff` preserva backend/evidência.
- Snapshot ilegível/malformado recusa antes de qualquer mutação.
- Teardown válido prova swapoff-first e revalidação fresca por ação.
- Nenhum override de ublk no WSL2 permanece em fonte, template ou teste.
