import json
with open('docs/governance/document-lifecycle-policy.json', 'r') as f:
    data = json.load(f)

if 'exclusions' not in data:
    data['exclusions'] = []

data['exclusions'].append({
    "id": "jules-findings",
    "pattern": "docs/jules/findings/**/*.md",
    "owner": "agent-orchestration",
    "reason": "Agent execution findings are ephemeral logs, not core documentation.",
    "registeredAt": "2026-08-29T03:22:00Z"
})

with open('docs/governance/document-lifecycle-policy.json', 'w') as f:
    json.dump(data, f, indent=2)
