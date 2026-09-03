# Let me look at the PR description from the submission tool:
#
# ## Resumo
# Added `if: steps.checkout.outcome == 'success'` to all execution steps in `kernel-build-matrix` job to ensure early exit and fail-fast when kernel source is unavailable, adhering to infrastructure hardening principles.
#
# ## Commits
# | Commit | O que fez | Por que fez | Detalhes |
# |---|---|---|---|
# | ci: add guard conditions for kernel source checkout | Added if condition in kernel-ci.yml | To fail-fast if source code is missing | <details><summary>detalhes</summary>**Arquivos:** .github/workflows/kernel-ci.yml<br>**Validacao:** actionlint<br>**Risco/rollback:** skipped erroneously</details> |
#
# ## Issue
# Closes #862
#
# ## Responsavel
# @emersonbusson
#
# ## Labels
# type:ci, area:scripts
#
# ## Validacao
# - [x] Gates de build/test do escopo tocado
# - [ ] `./scripts/docs-check.sh` (se tocou docs/specs ou gerou PRD/SPEC/IMPL)
# - [ ] SSDV3: SPEC/IMPL atualizados e citados (ou N/A — mudança não estrutural / só scripts)
#
# ## Rollback trigger
# If the workflow gets skipped erroneously or builds fail to trigger on valid checkouts.
#
