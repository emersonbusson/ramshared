import urllib.request
import json
import os

with open('plan.md', 'r') as f:
    plan = f.read()

req = urllib.request.Request(
    "http://127.0.0.1:8000/review_plan",
    data=json.dumps({"plan": plan}).encode('utf-8'),
    headers={"Content-Type": "application/json"}
)

try:
    with urllib.request.urlopen(req) as f:
        print(f.read().decode('utf-8'))
except urllib.error.URLError as e:
    print(e.read().decode('utf-8'))
