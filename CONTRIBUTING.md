# Contributing to RamShared

Thank you for your interest in contributing to RamShared! We welcome contributions from developers, hardware researchers, and kernel hackers to help accelerate software stacks using GPU VRAM.

---

## Code of Conduct

By participating in this project, you agree to abide by our standards of professional and respectful collaboration.

## How to Contribute

### 1. Reporting Issues
*   Search existing issues to check if your problem is already reported.
*   Provide a clear and concise description of the bug, including steps to reproduce it.
*   Include your system environment details (kernel version, WSL2 build, GPU model, and driver version).

### 2. Submitting Pull Requests
*   **Fork the repository** and create your branch from `main`.
*   Keep your changes focused. Do not mix unrelated refactorings or feature updates in a single PR.
*   Ensure that all tests pass: `cargo test --workspace`.
*   Ensure that the code compiles warning-free under cargo clippy: `cargo clippy --workspace --all-targets -- -D warnings`.

---

## Coding Standards

### Commit Messages
We follow the **Conventional Commits** standard in English.
*   Format: `type(scope): imperative title` (e.g., `feat(cuda): add support for windows driver API`).
*   Keep the subject line under 72 characters.
*   For any structural changes or changes that modify MMU, memory locking (`mlock`), or DRM subsystems, include a `Rollback trigger:` line in the commit body specifying a measurable metric or condition that warrants reverting the patch.

### System Safety
RamShared interacts with live hardware, GPU paging drivers, and operating system swap mechanisms.
*   **Fail-Closed Design:** Any hardware failure or connection loss must immediately complete block requests with error codes (e.g., `NBD_EINVAL` or `STATUS_DEVICE_NOT_READY`), rather than stalling the I/O queue.
*   **Manual/Supervised Rollouts:** Background services must remain disabled by default. Live testing on hardware must be supervised.
*   **Testing Gating:** High-risk code (especially kernel-mode components) must be fully validated in isolated virtual machines (QEMU/Hyper-V) before execution on the host machine.

---

## Project Structure

*   `/crates/`: Userspace Rust crates (CLI, broker daemon, CUDA wrappers, etc.).
*   `/scripts/`: Provisioning, testing harnesses, and QEMU virtualization drills.
*   `/docs/specs/no-milestone/{slug}/`: SSDV3 artifacts (`PRD.md`, `SPEC.md`, `IMPL.md`, optional `AUDIT-2.5.md`). Index: [`docs/INDEX.md`](docs/INDEX.md).
*   `/docs/`: Methodology (Kahneman), ADRs, runbooks, reliability, benchmarks.
*   `/docs/marketing/`: Launch kit (EN/PT social copy) + cascade diagram for Show & Tell posts.
*   `/tools/`: Docs hygiene (`generate-docs-index.mjs`, `check-broken-links.mjs`).

### Specs & docs checks

```bash
node tools/generate-docs-index.mjs          # regenerate docs/INDEX.md
./scripts/docs-check.sh                    # index --check + broken links under docs/
```

Structural work (locks, DMA, uAPI, mm, new driver surface) follows [`.claude/rules/ssdv3.md`](.claude/rules/ssdv3.md) and [`docs/SSDV3-PROMPTS.md`](docs/SSDV3-PROMPTS.md).
