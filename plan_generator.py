import subprocess

def get_diff():
    diff = subprocess.check_output(['git', 'diff', 'HEAD~1..HEAD']).decode('utf-8')
    diff_escaped = diff.replace('\n', '\\n').replace('"', '\\"').replace("'", "\\'")
    return diff_escaped

diff_text = get_diff()

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
main_diff = f"MAIN_DIFF: {diff_text}"

step1 = f"1. Retrieve git diff. PR_BODY: \"{pr_body}\" {rules} {main_diff}"

plan = f"""{step1}
2. Execute `shellcheck scripts/safety/cascade-pressure-probe.sh` to validate the bash script statically.
3. Complete pre commit steps to ensure proper testing, verification, review, and reflection are done.
4. Submit the task using the submit tool targeting jules/inbox branch."""

with open('plan.md', 'w') as f:
    f.write(plan)
