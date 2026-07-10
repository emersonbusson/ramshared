---
slug: kernel-native-language
title: Language policy for kernel-native VRAM work (C vs Rust)
milestone: —
issues: []
---

# PRD — Linguagem para “nativo de verdade no kernel”

> **Tipo:** PRD de **política / arquitetura** (não é feature de swap).  
> **Decisão canônica:** [ADR-0007](../../../decisions/ADR-0007-kernel-native-language-c.md).  
> **Auditoria:** [AUDIT-2.5.md](./AUDIT-2.5.md) → **go**.

## 1. Summary

Definir **oficialmente** que:

> **“Nativo de verdade no kernel” → C (estilo mainline / custom WSL kernel), não Rust de aplicação.**

O produto Day-1 no WSL (**cascade**) continua em **Rust** (userspace).  
Este PRD **não** substitui `wsl2-native-vram-tier` (fases P0–P3); ele **trava a linguagem** da parte kernel dessas fases.

## 2. Technical context

| Camada | Stack atual | Class |
| --- | --- | --- |
| CLI / daemon / CUDA FFI userspace | Rust (ADR-0002) | Confirmed in codebase |
| Windows StorPort | C/WDK | Confirmed |
| Kernel Linux rules no repo | C style + optional RfL mention | Confirmed in `.claude/rules/kernel.md` |
| GPU-PV limits | VRAM not bare device memory in WSL guest | Confirmed FASE0 / PASSO0 |

**Inference (scarce):** RfL may appear in some mainline drivers later; still not default for RamShared mm work.

## 3. Recommended option

**ADR-0007:** C default for kernel-native; Rust userspace unchanged; RfL only as audited exception.

**Not recommended:** full PRD/SPEC/IMPL of a C LKM in this document — that stays under `kernel-vram-as-memory` / future P1–P2 SPECs **using this language policy**.

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

N/A (policy). Kernel uAPI when designed uses C headers / sysfs as usual.

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| Dual default (C and RfL) | Exception requires SPEC + AUDIT |
| Ignoring GPU-PV limits | Language ADR does not grant P2 go |
| Agents implement LKM in Rust userspace style | RF-L4 + ADR-0007 in INDEX/README pointers |

## 10. Strategy

1. Accept ADR-0007 + AUDIT go.  
2. Point `wsl2-native-vram-tier` §8 at ADR-0007.  
3. Future P1/P2 SPEC must cite ADR-0007 in Kahneman/language block.

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
