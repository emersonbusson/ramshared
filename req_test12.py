# The CI failed because the PR description is missing "Issue" and "Responsavel".
# Wait, let me check the exact required sections.
# "PR description is missing required sections: Resumo, Commits, Issue, Responsavel, Labels, Validacao, Rollback trigger. Please check .github/pull_request_template.md."
# But wait, my PR description was:
# refactor: semantic error returns for connection framing
#
# ## Resumo
# Refactored `spawn_reader` to return semantic errors `ConnectionError::VersionMismatch` and `ConnectionError::PayloadTooLarge` instead of masking them or using generic I/O errors.
#
# ## Commits table
# | Commit | O que fez | Por que fez | Detalhes |
# |---|---|---|---|
# | HEAD | Added semantic errors in conn.rs | To follow the specific & semantic errors principle | Refactored `spawn_reader` to return `Result<(), ConnectionError>`. |
#
# ## Labels
# type:refactor
#
# ## Validacao
# `cargo test -p ramshared-wsl2d` and `cargo clippy -p ramshared-wsl2d -- -D warnings`
#
# ## Rollback trigger
# Revert if functional regressions are observed or CI tests fail.
