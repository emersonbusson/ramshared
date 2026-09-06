# Marketing & Architectural Visual Assets

This directory contains public architecture diagrams and benchmark comparison visual assets for RamShared.

## Visual Assets

| Asset | Format | Purpose | References |
| :--- | :--- | :--- | :--- |
| `cascade-diagram.svg` | Vector SVG | Primary tiering architecture diagram (English) | `README.md` |
| `cascade-diagram-pt.svg` | Vector SVG | Primary tiering architecture diagram (Portuguese) | `README.pt-BR.md` |
| `ramshared-top.png` | PNG (1920x1080) | CLI monitoring interface (`ramshared top`) screenshot | Documentation |
| `benchmark-comparison.svg` | Vector SVG | Direct I/O and latency comparison graph | `docs/benchmarks/public-claims.json` |
| `benchmark-comparison.jpg` | JPEG | Benchmark throughput comparison raster artifact | `docs/governance/public-binary-digests.json` |
| `benchmark-wsl2-vs-storport.jpg` | JPEG | WSL2 vs StorPort benchmark comparison artifact | `docs/governance/public-binary-digests.json` |
| `social-preview.png` | PNG (1200x630) | OpenGraph social preview and repository preview card | GitHub / social |

High-resolution raytraced 3D PNG renders (`cascade-diagram.png`, `cascade-diagram-pt.png`) are maintained locally in `local/marketing/` to preserve repository clone efficiency while keeping SVGs as the sharp, scalable public source of truth.

## Integrity

All binary and vector assets in this directory are governed by:
- `docs/benchmarks/public-claims.json`
- `docs/governance/public-binary-digests.json`
- `tools/ci/check-benchmark-evidence.mjs`
