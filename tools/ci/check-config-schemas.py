#!/usr/bin/env python3

import json
import sys
import os

try:
    import jsonschema
except ImportError:
    print("jsonschema is unavailable")
    sys.exit(69) # EX_UNAVAILABLE

try:
    import tomli
except ImportError:
    try:
        import tomllib as tomli
    except ImportError:
        print("tomli/tomllib is unavailable")
        sys.exit(69) # EX_UNAVAILABLE

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))

JSON_MAPPINGS = [
    ('docs/benchmarks/public-claims.schema.json', 'docs/benchmarks/public-claims.json'),
    ('docs/governance/remote-controls-observation.schema.json', 'docs/governance/remote-controls-observation.json')
]

TOML_CONFIGS = [
    ('crates/ramshared-winbroker/broker.example.toml', None),
    ('crates/ramshared-winsvc/winsvc.example.toml', None)
]

def main():
    failed = False

    for toml_rel, schema_rel in TOML_CONFIGS:
        toml_path = os.path.join(ROOT, toml_rel)
        if not os.path.isfile(toml_path):
            print(f"Target file {toml_rel} missing", file=sys.stderr)
            failed = True
            continue

        try:
            with open(toml_path, 'rb') as f:
                toml_data = tomli.load(f)

            if schema_rel:
                schema_path = os.path.join(ROOT, schema_rel)
                if not os.path.isfile(schema_path):
                    print(f"Target schema {schema_rel} missing", file=sys.stderr)
                    failed = True
                    continue
                with open(schema_path, 'r', encoding='utf-8') as fs:
                    schema_data = json.load(fs)
                jsonschema.validate(instance=toml_data, schema=schema_data)

            print(f"✓ {toml_rel} OK")
        except Exception as e:
            print(f"✗ {toml_rel} validation failed: {e}", file=sys.stderr)
            failed = True

    for schema_rel, config_rel in JSON_MAPPINGS:
        schema_path = os.path.join(ROOT, schema_rel)
        config_path = os.path.join(ROOT, config_rel)

        if not os.path.isfile(schema_path):
            print(f"Target file {schema_rel} missing", file=sys.stderr)
            failed = True
            continue

        if not os.path.isfile(config_path):
            print(f"Target file {config_rel} missing", file=sys.stderr)
            failed = True
            continue

        try:
            with open(schema_path, 'r', encoding='utf-8') as fs:
                schema_data = json.load(fs)
            with open(config_path, 'r', encoding='utf-8') as fc:
                config_data = json.load(fc)

            jsonschema.validate(instance=config_data, schema=schema_data)
            print(f"✓ {config_rel} OK")
        except Exception as e:
            print(f"✗ {config_rel} validation failed: {e}", file=sys.stderr)
            failed = True

    if failed:
        sys.exit(78) # EX_CONFIG

if __name__ == "__main__":
    if "--check" in sys.argv:
        print("Validating configuration schema compliance...")
        main()
        print("✓ RamShared configurations TOML/JSON matched against schema.")
