# AUDIT-2.5 — Lifecycle fail-closed do cascade

> Passo 2.5 SSDV3 + revisão de segurança da superfície privilegiada.
> Revisão: 2026-08-23.
> Escopo: enumeração, swapoff, reset, disconnect e parada de backend.

## Decisão

| Caminho | Veredito |
| --- | --- |
| Enumeração live de NBD/ublk/zram | **GO somente para detecção** |
| Mutação por nome, forma, allowlist ou `used_kb == 0` | **NO-GO** |
| Mutação com lifecycle binding exato, selado e revalidado | **GO de fonte; gates live pendentes** |
| Swapoff com snapshot ilegível, malformado ou incerto | **NO-GO** |
| Reset/disconnect/delete sem prova fresca de ausência | **NO-GO** |
| Standalone ublk no WSL2, com ou sem variável de override | **NO-GO permanente** |

O audit de 2026-07-10 foi supersedido. Uso zero não significa inatividade:
uma linha presente em `/proc/swaps` continua sendo swap ativo.

## Modelo de ameaça

| ID | Falha | Risco | Controle obrigatório |
| --- | --- | --- | --- |
| A1 | Device estrangeiro reutiliza um nome esperado | Crítico | Binding exato + cardinalidade + identidade de kernel |
| A2 | Parser interpreta incerteza como ausência | Crítico | Parser estrito retorna `Result`; erro preserva tudo |
| A3 | Estado muda entre plano e ação | Crítico | Reautorização imediatamente antes de cada mutação |
| A4 | `swapoff` falha ou tem resultado ambíguo | Crítico | Novo snapshot estrito; ausência não presumida |
| A5 | Backend morre com swap ativo, inclusive uso zero | Crítico | Swapoff-first sob responsabilidade do dono do lifecycle |
| A6 | Registro auxiliar diverge do binding | Alto | Divergência bloqueia a operação inteira |
| A7 | Duplicata ou cardinalidade inesperada | Alto | Refusa tudo e executa zero comandos |
| A8 | Evidência é apagada em falha parcial | Alto | Binding, registros e backend permanecem recuperáveis |

## Autoridade mínima para mutação

Uma ação exige, simultaneamente:

1. binding schema exato e arquivo selado;
2. boot ID, InvocationID, PID e start identity do daemon;
3. identidade do socket/export;
4. PARTUUID, PTUUID, `dev_t`, UUID de swap e hashes do origin;
5. conjunto e cardinalidade exatos dos devices;
6. igualdade com registros auxiliares estáveis;
7. enumeração live sem device estrangeiro ou ambíguo;
8. revalidação fresca imediatamente antes da ação;
9. antes de reset/disconnect/delete, snapshot estrito provando ausência exata.

A falha de qualquer item invalida a autoridade inteira. Não há recuperação
parcial baseada apenas nos itens que passaram.

## Kahneman

| # | Aplicação |
| --- | --- |
| #13 | Fixtures provam recusa e zero comandos, não só o caminho feliz |
| #15 | Primeiro erro encerra a sequência; não há retry que esconda incerteza |
| #16 | Default seguro é preservar foreign, ambíguo e swap ativo de uso zero |
| #17 | O estado só é removido depois do sucesso terminal completo |
| #18 | O controlador que possui o lifecycle também possui swapoff-first |

## Evidência hermética obrigatória

- foreign, duplicata, cardinalidade ambígua e registro divergente executam zero comandos;
- snapshot ilegível ou malformado recusa antes de mutação;
- swap ativo com uso zero e falha de swapoff preserva backend e evidência;
- resultado incerto de swapoff exige nova prova estrita de ausência;
- ordem válida é swapoff, ausência fresca, reset/disconnect/delete;
- TERM/Ctrl-C standalone ublk preserva backend quando swapoff não é comprovado;
- nenhum teste chama mkswap, swapoff, NBD, ublk ou zram real.

## Risco residual e gate

A revisão de fonte não prova comportamento de kernel nem hot-unplug real. O
gate live só pode ocorrer em ambiente descartável e isolado, com device
fabricado para o ensaio e evidência de detach terminal. O host diário e o WSL2
de produção permanecem fora de escopo. Até esse gate, o status é
**GO de fonte / NO-GO para ativação live**.
