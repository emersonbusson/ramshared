import json

with open("docs/governance/document-lifecycle-policy.json", "r") as f:
    data = json.load(f)

# Keep only the first occurrence of finding-ipc-resilience-005
seen = False
new_routes = []
for route in data["routes"]:
    if route["id"] == "finding-ipc-resilience-005":
        if not seen:
            seen = True
            new_routes.append(route)
    else:
        new_routes.append(route)

data["routes"] = new_routes

with open("docs/governance/document-lifecycle-policy.json", "w") as f:
    json.dump(data, f, indent=2)

print("Fixed document-lifecycle-policy.json")
