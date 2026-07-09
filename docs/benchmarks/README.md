# Benchmark machine-readable results

Append-only JSONL. One object per run. Human log: [`../BENCHMARKS.md`](../BENCHMARKS.md).

Rules: [`.claude/rules/benchmarks.md`](../../.claude/rules/benchmarks.md).

```bash
# example line (do not treat as real data)
# {"run_id":"...","ts":"...","median_us":241,"p99_us":...}
```

Harness scripts under `scripts/p0/` should append here when recording decision-grade numbers.
