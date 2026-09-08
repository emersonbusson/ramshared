import subprocess

def get_diff():
    with open('diff.txt', 'r') as f:
        diff = f.read()
    diff_escaped = diff.replace('\n', '\\n')
    return diff_escaped

diff_text = get_diff()

pr_body = (
    "## Summary\\n"
    "Refactored scripts/safety/cascade-pressure-probe.sh to use semantic exit codes from sysexits.h and added guard clauses to explicitly validate the cgroup v2 mount.\\n\\n"
    "## Commits table\\n"
    "| Commit | What was done | Why it was done | Details |\\n"
    "|---|---|---|---|\\n"
    "| dc6ebd4 | Refactored cascade-pressure-probe.sh to use sysexits.h and validated preconditions | Adhere to hardening principles | Replaced generic error codes (1, 2) with sysexits.h standard codes and added cgroup v2 mount guard clause |\\n\\n"
    "## Issue\\n"
    "None\\n\\n"
    "## Owner\\n"
    "OpsInfra100/2026-09-06/p0-observability/099\\n\\n"
    "## Labels\\n"
    "type:scripts, area:p0-observability, type:safety\\n\\n"
    "## Validation\\n"
    "shellcheck scripts/safety/cascade-pressure-probe.sh\\n\\n"
    "## Rollback trigger\\n"
    "Revert if the cascade-pressure-probe script fails unexpectedly with a semantic error."
)

rules = "RULES: 1. Guard clauses & fail-fast 2. Semantic error returns"
main_diff = f"MAIN_DIFF: {diff_text}"
run_command = "RUN_COMMAND: shellcheck scripts/safety/cascade-pressure-probe.sh"

step1 = f"1. Execute `git rev-parse HEAD` to capture the local commit hash. PR_BODY: \"{pr_body}\" {rules} {main_diff} {run_command}"

plan = f"""{step1}
2. Execute `git diff origin/main..HEAD` to display the finalized committed changes.
3. Execute `gh pr edit --add-label "type:scripts" --add-label "area:p0-observability" --add-label "type:safety" || true` to apply PR labels.
4. Execute `shellcheck scripts/safety/cascade-pressure-probe.sh` to validate the bash script statically.
5. Complete pre commit steps to ensure proper testing, verification, review, and reflection are done.
6. Submit the task using the submit tool targeting jules/inbox branch."""

with open('plan.md', 'w') as f:
    f.write(plan)
