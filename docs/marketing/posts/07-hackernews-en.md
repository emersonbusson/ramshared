# Post 07 — Hacker News (English)

**Three paste targets** (title, URL, optional first comment).  
**When:** optional, after Post 01.  
**Where:** https://news.ycombinator.com/submit  

**Do not paste** the lines that start with `>>>`.

---

## Steps

| Step | Action |
| --- | --- |
| **S1** | Open HN → **Submit** |
| **S2** | **Title** field → paste **T-HN-1** |
| **S3** | **URL** field → paste **U-HN-1** |
| **S4** | Submit |
| **S5** | First comment (recommended) → paste **C-HN-1** |
| **S6** | Stop |

---

## T-HN-1 — paste into Title

>>> COPY TITLE START

RamShared – idle GPU memory as a backup RAM cushion on Linux/WSL2

>>> COPY TITLE END

---

## U-HN-1 — paste into URL

>>> COPY URL START

https://github.com/emersonbusson/ramshared

>>> COPY URL END

---

## C-HN-1 — paste as first comment (optional)

>>> COPY BODY START

One-liner: when system RAM is gone, borrow idle GPU memory as a second cushion; give it back if the host needs the GPU (apps keep running).

Why not first-tier GPU swap: under host reclaim we measured ~1.2s stalls on tiny reads — freezes the machine if that is your primary emergency store.

Order: compressed RAM → idle GPU mem → disk.

Measured (see docs/reliability): ~241µs vs ~326µs medians on two paths; ~500MB/480MB demote drill with 0 corruption logged.

Day-1: Linux/WSL2 + NVIDIA. Not free RAM for maxed games. Happy to hear what you'd break first.

>>> COPY BODY END
