import sys

# Format the 10 contractual blocks on a single line
pr_body = (
    "## Resumo\\n"
    "Refactored scripts/safety/cascade-pressure-probe.sh to use semantic exit codes from sysexits.h and added guard clauses to explicitly validate the cgroup v2 mount.\\n\\n"
    "## Commits table\\n"
    "| Commit | O que fez | Por que fez | Detalhes |\\n"
    "|---|---|---|---|\\n"
    "| dc6ebd4 | Refactored cascade-pressure-probe.sh to use sysexits.h and validated preconditions | Adhere to hardening principles | Replaced generic error codes (1, 2) with sysexits.h standard codes and added cgroup v2 mount guard clause |\\n\\n"
    "## Labels\\n"
    "type:scripts, area:p0-observability, type:safety\\n\\n"
    "## Validacao\\n"
    "shellcheck scripts/safety/cascade-pressure-probe.sh\\n\\n"
    "## Rollback trigger\\n"
    "Revert if the cascade-pressure-probe script fails unexpectedly with a semantic error."
)

rules = "RULES: 1. Guard clauses & fail-fast 2. Semantic error returns"
main_diff = "MAIN_DIFF: ---\\n+++ b/scripts/safety/cascade-pressure-probe.sh\\n@@ -66,4 +66,10 @@\\n-exit 1\\n+exit 69"

plan = f"""1. Retrieving git diff to ensure code correctness and format PR description exactly as required, adhering strictly to the constraints in the instructions.
2. Executing `shellcheck scripts/safety/cascade-pressure-probe.sh` to validate the bash script statically.
3. Complete pre commit steps to ensure proper testing, verification, review, and reflection are done.
4. Submit the task using the submit tool targeting jules/inbox branch."""

with open("plan.md", "w") as f:
    f.write(plan)
