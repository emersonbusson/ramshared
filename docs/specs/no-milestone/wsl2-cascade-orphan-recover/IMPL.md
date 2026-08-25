# IMPL — Detecção de órfãos e lifecycle fail-closed

Status: **fonte corrigida; validação serial hermética completa; live bloqueado**.

## Implementado

- Parser estrito de `/proc/swaps` com erro explícito e seams temporais.
- Enumeração NBD/ublk/zram somente para detecção.
- Vínculo atômico e selado com identidade completa do daemon, socket, origin e
  devices, além de cardinalidade exata.
- Autorização e revalidação frescas antes de cada ação.
- Cardinalidade live exata por estágio; ausência de um device bound é NO-GO,
  não um motivo para pular a mutação e prosseguir.
- Todos os `swapoff` antes de reset/disconnect/parada do daemon.
- Ausência estrita obrigatória antes de reset/disconnect/delete.
- Preservação de vínculo, registros, daemon e forensics em qualquer NO-GO.
- ublk standalone permanentemente recusado no WSL2 e swapoff-first em Linux
  isolado.
- zram registrado/revalidado antes de `mkswap`; rollback exige o record exato e
  o fallback sysfs não possuído foi removido.

## Evidência atual

Com Rust 1.98 e somente 1 job/thread, a suíte focal de lifecycle passou 49/49.
A validação completa de `ramshared-cli` passou 191 testes unitários e 6 testes
de dispatch, sem falha. `cargo check -p ramshared-cli` também passou. As
fixtures herméticas cobrem device estrangeiro, device bound ausente,
cardinalidade por estágio, swap ativo com uso zero, snapshot ilegível ou
malformado, resultado incerto de swapon/swapoff, rollback zram sem record e
falha no post-check de detach NBD. Nenhum teste chamou `mkswap` em device real.

Os resultados live de 2026-07-10 pertencem à implementação antiga. Eles não
qualificam o lifecycle atual e não autorizam restaurar auto-recover por uso zero.

## Gates live restantes

- Nenhum neste patch de fonte é executado automaticamente.
- Uma futura validação isolada precisa provar identidade real, detach terminal
  e ausência de ghost sem usar o host diário.
- WSL2 ublk standalone permanece NO-GO independentemente de testes QEMU.
