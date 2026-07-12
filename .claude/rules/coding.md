# General Coding Rules — RamShared

## Session Memory

`MEMORY.md` at the repository root is the single shared **local** memory file for agent sessions. It is listed in `.gitignore` (not shipped in clones). Create it on first write if missing.

Rules:
- Read `MEMORY.md` from bottom to top at the start of every session **when the file exists**.
- Start from the most recent entry at the end of the file.
- Before ending a session, append a new entry to the end of `MEMORY.md` (create the file if needed).
- `MEMORY.md` is append-only by default: never delete or rewrite older entries.
- Never store secrets or addresses that leak KASLR in `MEMORY.md`.

## Language

- All code, variable names, function names, macros, and filenames: **English**
- Code comments (inline, block, doc): **English** — strictly enforced. No Portuguese comments are allowed in new code.
- Root documentation files (`README.md`, `ROADMAP.md`, `CLAUDE.md`, etc.) and docs: **English** (optionally with localized summaries in `<details>` blocks).
- Commit messages and PR titles: **English** (Conventional Commits format)
- PR descriptions (body) and Issues: **English** or **Portuguese** (as long as it meets PR template guidelines)

## Commit Conventions

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): short description
```

**Valid types:** `feat`, `fix`, `chore`, `docs`, `style`, `refactor`, `test`, `ci`, `perf`, `build`
**Valid scopes:** `mm`, `drm`, `cxl`, `pci`, `dma`, `core`, `docs`, `ci`, `scripts`

**Examples:**
- `feat(mm): export VRAM as NUMA node`
- `fix(drm): resolve memory leak in buffer allocation`
- `chore(ci): update kselftest runner config`

## Commit Checkpoints

Treat commits as review checkpoints, not as time-based snapshots.
- Create a commit when one responsibility is complete and reviewable.
- Prefer atomic slices. A good checkpoint usually means: code changes are done, and `checkpatch.pl` passes.
- Do not commit broken code (that causes Kernel Panic or OOPs) just to "save work".

## RamShared Day-0 Policy

RamShared is an R&D and hardware acceleration project. Every modification and every new file must be the clean, definitive Day-0 solution.
- Do not create shims, compatibility layers, or dual-paths for old hardware unless strictly necessary and documented.
- Rewrite or remove dead paths when a clean Day-0 implementation replaces them.

## Kernel Naming Convention (snake_case Everywhere)

**Single convention: snake_case for ALL code surface.** No exceptions.

- C Structs: `struct ramshared_device`
- Functions: `ramshared_init_device()`
- Macros: `RAMSHARED_MAX_DEVICES` (UPPER_SNAKE_CASE)
- Rust modules: `mod ramshared_core`

## Error Handling

- **C:** Never ignore return codes. Always propagate negative errno codes (e.g., `-ENOMEM`, `-EINVAL`). Clean up resources using the `goto out_err;` idiom to prevent memory leaks.
- **Rust:** Use `Result<T, Error>`. Never use `.unwrap()` or `.expect()` in production kernel code.

## Code Quality

- Never use raw `printk` without a level. Use `pr_info`, `pr_debug`, `pr_err`, or `dev_info` for device-specific logs.
- Functions exceeding ~80 lines should be broken into smaller, focused functions.
- Keep cognitive complexity low. Avoid deep nesting (max 3-4 levels of indentation).

## Pre-task Completion Checklist

**Every task that touches C/Rust files MUST pass validation before being considered done:**

```bash
# C formatting and linting
./scripts/checkpatch.pl -f path/to/file.c

# Static Analysis (if Sparse is enabled)
make C=1 M=drivers/ramshared

# Rust Validation (if applicable)
cargo clippy
rustfmt

# Build
make modules
```

**These checks are mandatory.** Never mark a task as complete without ensuring the kernel module compiles and passes `checkpatch.pl`.

## Issue & PR Lifecycle

1. **Search before creating** — `gh issue list --search "<keywords>"` to find existing issues.
2. **Create BEFORE the PR** — if work was done but no issue exists, create the issue first.
3. **Labels** — must reflect the code actually changed, not just the topic. Example: `fix`, `mm`, `test`.

**Language:**
- Issue title: **Portuguese (BR)**
- Issue body: **Portuguese (BR)**
