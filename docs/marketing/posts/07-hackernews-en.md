# Post 07 — Hacker News (English)

**Post ID:** `POST-07`  
**When:** Optional — after **POST-01**, when you want a harsher technical audience  
**Where:** https://news.ycombinator.com/submit  
**Language:** English  

---

## Steps

| Step | ID | Action |
| --- | --- | --- |
| 1 | **S1** | Open HN → **Submit** |
| 2 | **S2** | **Title** → **T-HN-1** |
| 3 | **S3** | **URL** → `https://github.com/emersonbusson/ramshared` |
| 4 | **S4** | Submit (link post — no body on submit form) |
| 5 | **S5** | As first comment, paste **C-HN-1** (limits first) |
| 6 | **S6** | Stop — expect tough questions |

No image on HN submit. Diagram is optional in a comment.

---

## T-HN-1 — Title

```text
RamShared – idle GPU memory as a backup RAM cushion on Linux/WSL2
```

---

## URL

```text
https://github.com/emersonbusson/ramshared
```

---

## C-HN-1 — First comment (optional but recommended)

```text
One-liner: when system RAM is gone, borrow idle GPU memory as a second cushion; give it back if the host needs the GPU (apps keep running).

Why not first-tier GPU swap: under host reclaim we measured ~1.2s stalls on tiny reads — freezes the machine if that is your primary emergency store.

Order: compressed RAM → idle GPU mem → disk.

Measured (see docs/reliability): ~241µs vs ~326µs medians on two paths; ~500MB/480MB demote drill with 0 corruption logged.

Day-1: Linux/WSL2 + NVIDIA. Not free RAM for maxed games. Happy to hear what you'd break first.
```
