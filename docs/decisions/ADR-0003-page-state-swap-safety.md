# ADR-0003 — Modelo de estados de página: herdar o swap do Linux; o daemon garante durabilidade, DEMOTE e atomicidade

**Status:** Accepted (2026-06-05).

## Context

A VRAM é um tier de swap (ADR-0001). Toda plataforma de memória tem uma máquina
de estados de página — no Windows: `Em uso → Modificado (suja, precisa gravar
antes de reusar) → Em espera (cache limpo) → Livre`. Os modos de falha que essa
máquina expõe, no nosso caso:

- **Perda de página suja:** um frame "modificado" reusado antes de gravar = dado perdido.
- **Freeze:** quando o host Windows precisa de VRAM, o conteúdo da nossa alocação
  é "modificado" e o WDDM **grava no pagefile do Windows** antes de reaproveitar —
  é o spike de **1,18 s** medido na Fase 0.
- **Leitura torn:** ler um bloco enquanto uma escrita ao mesmo bloco está em voo.

Pergunta: reimplementar essa máquina de estados no nosso daemon?

## Decision

**Não reimplementar.** Herdar a máquina de estados de página do **subsistema de
swap do Linux** (dirty → writeback → swap-in); nós só fornecemos os tiers
(zram→VRAM→VHDX). O daemon garante **três invariantes** no tier VRAM:

1. **Durabilidade antes do ACK (§8):** completa o I/O do block layer **somente**
   após o dado estar durável na VRAM. O kernel só libera o frame de RAM da página
   suja depois disso.
2. **DEMOTE por latência (§9):** sob eviction WDDM, `swapoff` só da VRAM (páginas
   migram pro VHDX) **sem matar processo**.
3. **Atomicidade por bloco em voo (§8.1):** requisição a um bloco com operação em
   voo no mesmo bloco serializa atrás dela — sem leitura torn.

Mais: `swapoff` **antes** de desconectar o NBD (desconectar com páginas de swap
vivas = panic).

## Consequences

- (+) Não duplica o que o kernel já faz; safety concentrada em 3 invariantes testáveis.
- (+) Cada invariante mapeia a um modo de falha real (não a um cenário hipotético).
- (−) Durabilidade síncrona (`cuMemcpy*_v2`) custa latência por op — aceito porque
  a VRAM é tier **frio** (ADR-0001), não o swap quente.

## Alternatives considered

- **Reimplementar modified/standby em userspace:** rejeitado — o kernel já faz; seria superfície de bug e ruído.
- **Confiar que a VRAM nunca é evictada:** rejeitado — a Fase 0 mediu a eviction (1,18 s).

## Kahneman

- #5 worst-case (eviction medida) · #13 ilusão de validade (durabilidade e
  atomicidade exigem teste de integração do modo de falha real, não mock).

## Rollback trigger

Bloquear o tier VRAM se um teste de integração mostrar **(a)** um WRITE já
ACKado que não sobrevive a um swap-in posterior (perda de página suja), **ou**
**(b)** leitura torn sob QD>1 no mesmo bloco.

Links: [ADR-0001](ADR-0001-vram-cascade-tiering.md) ·
[`../specs/no-milestone/wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §8, §9 ·
[`../reliability/wsl2-fase0-final.md`](../reliability/wsl2-fase0-final.md) ·
[`../reliability/DEGRADATION-MATRIX.md`](../reliability/DEGRADATION-MATRIX.md).
