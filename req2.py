import urllib.request
import json

with open('plan.md', 'r') as f:
    plan = f.read()

print("Sending Request!")
