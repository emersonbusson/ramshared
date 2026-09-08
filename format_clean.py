import sys

with open("clean_plan.txt", "r") as f:
    content = f.read()

# Fix the quotes!
# We want EXACTLY: export PR_BODY="\"## Summary ... error.\" RULES: ..."
content = content.replace('export PR_BODY="\\"## Summary', 'export PR_BODY="\\"## Summary')
content = content.replace('error.\\" RULES:', 'error.\\"" RULES:')

with open("clean_plan.txt", "w") as f:
    f.write(content)
