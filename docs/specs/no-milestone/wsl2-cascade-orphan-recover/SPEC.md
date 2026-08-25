# SPEC — Detecção de órfãos e lifecycle fail-closed

Esta revisão invalida o antigo plano de auto-recover por `used_kb == 0`.
Enumeração é somente detecção; propriedade vem exclusivamente do vínculo
selado e fresco.

## Decisões técnicas

| ID | Decisão | Motivo |
| --- | --- | --- |
| DT-R1 | `parse_proc_swaps` retorna `Result`, exige schema completo e rejeita duplicatas. | Incerteza nunca significa ausência. |
| DT-R2 | `OrphanPlan::DetectedUnboundZeroUsed` retorna recusa sem side effect. | Uso zero ainda é swap ativo. |
| DT-R3 | `LifecycleBinding` schema 1 contém boot, daemon PID+start, InvocationID, socket, origin e devices exatos; exige 1 NBD, 0 ublk e no máximo 1 zram. | Nome ou forma do device não prova ownership. |
| DT-R4 | Enumeração de `/sys/class/block` apenas produz observações. Qualquer foreign/duplicata/mismatch bloqueia a operação inteira. | Não tocar em devices de terceiros. |
| DT-R5 | Cada executor relê swaps, enumera, autoriza e revalida o device imediatamente antes da ação. Reset/disconnect e delete exigem ausência estrita fresca. | Fecha TOCTOU entre plano e mutação. |
| DT-R6 | Falha de `swapoff` só é tratada como ausência quando uma nova leitura estrita prova ausência; caso contrário retorna `UnsafeContainment`. | Saída de comando é ambígua. |
| DT-R7 | Standalone ublk não possui override WSL2; em Linux isolado, TERM/INT chama swapoff-first e só então STOP/DELETE. | Evita backend morto sob swap ativo. |
| DT-R8 | O conjunto live deve ser exatamente igual ao conjunto esperado em cada estágio: binding completo antes de swapoff/reset, somente NBD depois do reset e vazio depois do disconnect. Device bound ausente também bloqueia. | Fecha a lacuna em que cardinalidade parcial podia pular detach e ainda parar o daemon. |
| DT-R9 | zram só nasce por `zramctl --find`; identidade é selada e revalidada antes de `mkswap`. Rollback sem record exato e fallback sysfs em `zram0` são recusados. | Evita formatar/resetar device estrangeiro ou retargeted. |

## Ordem obrigatória

1. snapshot estrito;
2. vínculo selado e cardinalidade;
3. daemon/InvocationID/socket/origin/registros;
4. enumeração detection-only e igualdade exata;
5. swapoff de todos os devices ativos;
6. prova fresca de ausência por device;
7. reset zram;
8. disconnect NBD;
9. prova de desaparecimento;
10. parada do daemon;
11. remoção dos registros.

O primeiro erro interrompe a sequência e preserva o estado recuperável.

## Testes obrigatórios

- `down_refuses_foreign_live_device_without_running_a_command`
- `down_refuses_missing_bound_live_device_without_running_a_command`
- `down_refuses_ambiguous_live_identity_without_running_a_command`
- `down_refuses_foreign_managed_swap_without_running_a_command`
- `down_refuses_mismatched_runtime_record_without_running_a_command`
- `down_refuses_unreadable_or_malformed_swap_snapshot_before_mutation`
- `active_zero_use_swapoff_failure_preserves_backend_and_evidence`
- `uncertain_swapoff_absence_proof_preserves_backend_and_evidence`
- `lifecycle_binding_rejects_ambiguous_device_cardinality`
- `swapoff_completes_before_nbd_disconnect`
- `zram_rollback_requires_recorded_identity_before_any_command`
- `zram_setup_never_mutates_unbound_sysfs_fallback`
- `zram_reset_stage_mismatch_stops_before_nbd_disconnect`
- `nbd_startup_disconnect_postcheck_preserves_daemon_and_evidence`
- standalone ublk: active-zero, used swap, parser failure e `swapoff` failure
  preservam STOP/DELETE/backend.

Todos são unitários/herméticos. Nenhum teste executa swapoff, NBD, ublk, zram
ou device real.

## Rollback trigger

Qualquer comando emitido para device estrangeiro/ambíguo, qualquer
reset/disconnect/delete sem snapshot fresco ou qualquer remoção de evidência
após resultado incerto mantém ativação bloqueada e exige reauditoria.
