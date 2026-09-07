import json

with open("docs/governance/document-lifecycle-policy.json", "r") as f:
    data = json.load(f)

new_route = {
    "id": "finding-ipc-resilience-005",
    "pattern": "docs/jules/findings/ipc-resilience-005.md",
    "owner": "reliability",
    "canonicalSource": "docs/jules/findings/ipc-resilience-005.md",
    "lifecycle": "reviewable",
    "freshnessDays": 90,
    "verification": { "state": "unverified" },
    "registeredAt": "2026-09-06T00:00:00Z"
}

data["routes"].append(new_route)

with open("docs/governance/document-lifecycle-policy.json", "w") as f:
    json.dump(data, f, indent=2)

print("Updated document-lifecycle-policy.json")
