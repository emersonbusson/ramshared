## Resumo

Este PR melhora a performance da função `swapoff_all` no crate `ramshared-cli` removendo clonagens desnecessárias de `String` no acúmulo de falhas, reduzindo as alocações de memória.

## Commits

| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| `HEAD` | Alterada assinatura e iteração de `swapoff_all` e testes | Para evitar clonagens de `String` desnecessárias (`p.clone()`) no acúmulo de erros | <details><summary>detalhes</summary>**Arquivos:** `crates/ramshared-cli/src/cascade/mod.rs`<br>**Validacao:** `cargo test -p ramshared-cli` OK, clippy OK.<br>**Risco/rollback:** Baixo. Rollback se problemas na cascata de swap.</details> |

## Issue

Closes #

## Responsavel

@jules

## Labels

type:perf, area:cli

## Validacao

- [x] Gates de build/test do escopo tocado
- [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
- [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)

## Rollback trigger

Falhas nos comandos de down/swapoff da CLI `ramshared` ou regressão de runtime (latência).

💡 **What:** The optimization implemented was changing the `swapoff_all` signature to `fn swapoff_all<'a>(paths: &'a [String], entries: &[SwapEntry]) -> Vec<(&'a str, String)>` instead of taking and returning full owned strings.
🎯 **Why:** To prevent unnecessary `.clone()` operations on strings containing device paths each time a `swapoff` operation yields an error that must be accumulated.
📊 **Measured Improvement:** Replaced memory allocations per failed entry in the failure array accumulation logic with zero-copy references. Though no micro-benchmark exists, avoiding O(N) heap allocations when collecting items in a `Vec` is a fundamental performance improvement with O(1) space complexity on the string paths.
