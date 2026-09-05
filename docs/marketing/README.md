# Marketing & Architectural Visual Assets

This directory contains public architecture diagrams and benchmark comparison visual assets for RamShared.

## Visual Assets

| Asset | Format | Purpose | References |
| :--- | :--- | :--- | :--- |
| `cascade-diagram.png` | PNG (1920x1080) | Primary tiering architecture diagram (English) | `README.md` |
| `cascade-diagram-pt.png` | PNG (1920x1080) | Primary tiering architecture diagram (Portuguese) | `README.pt-BR.md` |
| `cascade-diagram.svg` | Vector SVG | High-resolution scalable cascade architecture | `docs/benchmarks/public-claims.json` |
| `cascade-diagram-pt.svg` | Vector SVG | High-resolution scalable cascade architecture (PT) | `docs/benchmarks/public-claims.json` |
| `benchmark-comparison.svg` | Vector SVG | Direct I/O and latency comparison graph | `docs/benchmarks/public-claims.json` |
| `benchmark-comparison.jpg` | JPEG | Benchmark throughput comparison raster artifact | `docs/governance/public-binary-digests.json` |
| `benchmark-wsl2-vs-storport.jpg` | JPEG | WSL2 vs StorPort benchmark comparison artifact | `docs/governance/public-binary-digests.json` |

## Integrity

All binary and vector assets in this directory are governed by:
- `docs/benchmarks/public-claims.json`
- `docs/governance/public-binary-digests.json`
- `tools/ci/check-benchmark-evidence.mjs`
