---
slug: kernel-native-language
title: Language policy for kernel-native VRAM work (C vs Rust)
milestone: —
issues: []
---

# PRD — Language for “truly native in the kernel”

> **Type:** **policy / architecture** PRD (not a swap feature).
> **Canonical decision:** [ADR-0007](../../../decisions/ADR-0007-kernel-native-language-c.md).
> **Audit:** [AUDIT-2.5.md](./AUDIT-2.5.md) → **go**.

## 1. Summary

Formally define that:

> **“Truly native in the kernel” → C (mainline style / custom WSL kernel), not application Rust.**

The Day-1 WSL product (**cascade**) remains in **Rust** (userspace).
This PRD does **not** replace `wsl2-native-vram-tier` (P0–P3 phases); it
locks the language for the kernel portion of those phases.

## 2. Technical context

| Layer | Current stack | Class |
| --- | --- | --- |
| CLI / daemon / CUDA FFI userspace | Rust (ADR-0002) | Confirmed in codebase |
| Windows StorPort | C/WDK | Confirmed |
| Kernel Linux rules in repository | C style + optional RfL mention | Confirmed in `.claude/rules/kernel.md` |
| GPU-PV limits | VRAM not bare device memory in WSL guest | Confirmed FASE0 / PASSO0 |

**Inference (limited):** RfL may appear in some mainline drivers later; it is
still not the default for RamShared memory-management work.

## 3. Recommended option

**ADR-0007:** C is the default for kernel-native work; Rust userspace is
unchanged; RfL is permitted only as an audited exception.

**Not recommended:** a full PRD/SPEC/IMPL for a C LKM in this document — that
remains under `kernel-vram-as-memory` / future P1–P2 SPECs **using this
language policy**.

## 4. Functional requirements

| ID | Requirement |
| --- | --- |
| RF-L1 | Kernel-context RamShared code is C11 kernel style unless RfL exception SPEC exists |
| RF-L2 | Userspace cascade remains Rust |
| RF-L3 | Any RfL exception documents tree, crate boundary, and non-goals (no mm rewrite) |
| RF-L4 | CI/docs state the split so agents do not implement “native kernel” in app Rust |
| RF-L5 | Test matrix language-agnostic: WSL P0 Rust; kernel builds may use VM; P2 needs real GPU path |

## 5. Non-functional

| ID | Requirement |
| --- | --- |
| NFR-L1 | checkpatch / sparse discipline on C |
| NFR-L2 | No new `unsafe` surface in userspace outside `ramshared-cuda` without review |
| NFR-L3 | Audit trail: ADR + this PRD + AUDIT-2.5 |

## 6–7. Flows / data

N/A beyond “new kernel file → C” and “new userspace crate → Rust”.

## 8. API

N/A (policy). When designed, kernel uAPI uses C headers / sysfs as usual.

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| Dual default (C and RfL) | Exception requires SPEC + AUDIT |
| Ignoring GPU-PV limits | Language ADR does not grant P2 go |
| Agents implement LKM in Rust userspace style | RF-L4 + ADR-0007 in INDEX/README pointers |

## 10. Strategy

1. Accept ADR-0007 + AUDIT go.
2. Point `wsl2-native-vram-tier` §8 at ADR-0007.
3. Future P1/P2 SPEC must cite ADR-0007 in the Kahneman/language block.

## 11. Documents

- ADR-0007
- This PRD + AUDIT-2.5
- Cross-link `wsl2-native-vram-tier`
- `docs/decisions/README.md` if present

## 12. Out of scope

- Implementing LKM
- Changing cascade language
- Dual-boot install

## 13. Acceptance

- [x] ADR written
- [x] AUDIT go/no-go
- [x] PRD policy IDs
- [x] Cross-links

## 14. Validation

- `node tools/generate-docs-index.mjs`
- Human: language split readable in ADR §Decision

## 15. Kahneman

| # | Note |
| --- | --- |
| #11 | Rust success on cascade ≠ Rust for kernel mm |
| #13 | RfL exists upstream ≠ our WSL tree supports it |
| #18 | If C native path dead on GPU-PV, sunset P2 for WSL; keep Rust P0 |
