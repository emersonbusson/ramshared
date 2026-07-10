# AUDIT-2.5 — kernel-native-language (C vs app Rust)

> Adversarial review of language policy for “native for real in the kernel”.  
> Date: 2026-07-10.  
> Inputs: ADR-0002, ADR-0007 draft, `wsl2-native-vram-tier` §8, `.claude/rules/kernel.md`, FASE0 latency evidence.

## Scope of audit

**In:** which language for Linux **kernel-context** RamShared work.  
**Out:** whether P2 device-memory is feasible on GPU-PV (already NO-GO Day-0 in other PRDs); Windows StorPort C (already decided).

## Findings

| Sev | Finding | Disposition |
| --- | --- | --- |
| **HIGH** | Implementing “kernel native” as **userspace Rust** (new daemon paths only) **re-labels P0** and does not meet “nativo de verdade no kernel” | **Policy:** kernel context → **C**; userspace → **Rust** |
| **HIGH** | Claiming native mm success on **WSL GPU-PV** without device-memory evidence is **#13 theater** regardless of C or Rust | Language ADR **does not** approve P2 on WSL alone |
| **MED** | **Rust for Linux** as silent default creates dual-path and review risk | **Exception only** with SPEC + this-class audit |
| **MED** | Custom WSL `bzImage` + C module still needs boot-safe rollback (`boot-kernel-safe`) | Reference existing scripts; not a language issue |
| **LOW** | Two languages in monorepo | Already true (Rust + WDK C); acceptable |
| **LOW** | Agents may paste app-Rust idioms into LKM | ADR-0007 + coding/kernel rules; checkpatch gate |

## Alternatives (attacked)

| Alternative | Attack | Result |
| --- | --- | --- |
| Rust-only everything | Cannot merge/review as normal LKM; does not put code in kernel context | Reject as native path |
| RfL default | Toolchain/WSL uncertainty; mm still C; false sense of safety | Exception only |
| C userspace cascade rewrite | Burns ADR-0002; no product win for P0 | Reject |
| Defer any written policy | Agents will invent language per chat | Reject — write ADR |

## Kahneman checks

| # | Question | Evidence | Abort if |
| --- | --- | --- | --- |
| #11 | Are we picking C only because “kernel culture halo”? | Need C for **ABI/review/run-in-kernel**; Rust remains for measured P0 product | N/A — split is intentional |
| #13 | Does “we chose C” equal “native VRAM works on WSL”? | **No** — FASE0 + GPU-PV inventory | Any doc that equates the two |
| #2 | Counterfactual: C LKM on WSL that treats VRAM hot | Stall class ~1 s → still need cold tier + demote | Hot-path design in SPEC |
| #18 | If C native never ships on WSL | Keep Rust cascade; document sunset of P2-for-WSL | Endless C prototype without Gate B |

## Open questions (non-blocking)

1. Exact WSL kernel version / CONFIG set for a future P1 ublk SPEC.  
2. Whether a **single** RfL crate ever appears for a narrow helper (defer).  

## Verdict

### **go**

- Accept **ADR-0007**.  
- Keep **PRD** as policy record; **no IMPL** of LKM in this folder.  
- Future kernel SPECs **must** cite ADR-0007.  
- Do **not** create `SPECv2` language forks; amend ADR in-place if RfL exception is ever granted.

## Blockers fixed in policy

- Explicit split P0 Rust vs kernel C.  
- Explicit non-claim: language ≠ GPU-PV device memory.  
- Explicit RfL exception process.
