## Resumo
Add `if: hashFiles('scripts/ci/') != ''` guard conditions to check kernel source availability in the `kernel-ci.yml` CI runner before running kernel build steps.

## Commits
| Commit | O que fez | Por que fez | Detalhes |
|---|---|---|---|
| `ci: guard kernel source availability` | Added `if` conditions to kernel steps | Prevent CI failures when kernel scripts are missing | Checked `hashFiles('scripts/ci/')` |

## Issue
OpsInfra100/2026-09-01/ci-workflows/060

## Responsavel
Jules

## Labels
type:ci

## Validacao
`actionlint .github/workflows/kernel-ci.yml`

## Rollback trigger
CI jobs failing due to syntax errors in YAML or logic errors preventing legitimate builds.

---

1. Use the `run_in_bash_session` tool to execute `curl -sL https://github.com/rhysd/actionlint/releases/download/v1.7.1/actionlint_1.7.1_linux_amd64.tar.gz | tar xz actionlint` to install `actionlint` if it's missing in the execution environment.
2. Use the `replace_with_git_merge_diff` tool to modify `.github/workflows/kernel-ci.yml`. I will add an `if: hashFiles('scripts/ci/') != ''` condition to the jobs/steps that run kernel scripts, or as early as possible in the jobs to skip them if the source is unavailable.
3. Use the `run_in_bash_session` tool to execute `./actionlint .github/workflows/kernel-ci.yml` to validate the YAML syntax.
4. Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.
5. Use the `submit` tool to create a PR against `jules/inbox`.
