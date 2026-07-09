# Post 01 — Reddit r/rust (English) — DO THIS FIRST

**Post ID:** `POST-01`  
**When:** Day 0 (today)  
**Where:** https://www.reddit.com/r/rust  
**Language:** English only  

---

## Steps

| Step | ID | Action |
| --- | --- | --- |
| 1 | **S1** | Open https://www.reddit.com/r/rust → **Create Post** → type **Text** |
| 2 | **S2** | Confirm community = **r/rust** |
| 3 | **S3** | Paste title → **T-EN-1** (below) |
| 4 | **S4** | Paste body → **B-EN-1** (below) |
| 5 | **S5** | Attach image → **IMG-1** |
| 6 | **S6** | Flair (if available) → **FLAIR-1** = Show & Tell |
| 7 | **S7** | Click **Post** |
| 8 | **S8** | **Stop.** No X, no LinkedIn, no other subreddit today. |

---

## T-EN-1 — Title

Copy everything inside the fence into Reddit’s **Title** field:

```text
[Show & Tell] RamShared — idle GPU memory as a backup cushion on Linux/WSL2 (when RAM is tight, borrow the GPU — give it back if the GPU needs it)
```

---

## B-EN-1 — Body

Copy everything inside the fence into Reddit’s **Text** field:

```markdown
**When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.**

I built **RamShared** (Rust, Linux/WSL2, NVIDIA): a practical way to borrow **idle graphics memory** when system RAM is tight, without pretending GPU memory is as safe/fast as main RAM.

## Problem (human)
You’re compiling / running containers / drowning in tabs. RAM is gone. The machine starts thrashing the **SSD**. Meanwhile the **GPU memory** is often almost empty. You already paid for that silicon.

## Why not “just put all swap on the GPU”?
When Windows reclaims graphics memory under pressure, that memory can get **very slow**. We measured about **1.2 seconds** for a tiny read in the bad case. If that were your *first* emergency store, the whole machine freezes. So GPU memory is only a **second** cushion — and we can **give it back**.

## Design (still short)
```text
Need memory?  →  1) compressed RAM (zram)     — first, fast
              →  2) idle GPU memory           — second, colder
              →  3) disk (SSD / VHDX)         — last resort
```

If latency spikes / host pressure: **stop using the GPU cushion**, data slides to disk, **apps keep running**.

## Numbers (measured)
- Bad case under host GPU reclaim: up to **~1.2 s** for a small read (why GPU is second, not first).
- Faster path **~241 µs** median vs older path **~326 µs** (same window, multi-run).
- Stress drill: **~500 MB** on GPU tier, **~480 MB** moved back, **0 corruption**.

## Try it
```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show   # success ≈ three lines: zram + GPU + disk
```

## Honest limits
- Day-1 path is **Linux/WSL2 + NVIDIA**, not “every GPU / every OS.”
- Not free RAM for maxed-out games.
- We don’t thrash live WSL2 on purpose; heavy tests use isolated VMs.
- Not bare-metal CXL magic — practical workstation tool.

## Looking for feedback
Especially from people who’ve fought **swap, block devices, CUDA, or WSL2**:
1. Second-cushion + give-back vs other APIs under Windows GPU reclaim.
2. What you’d want in a “it just works” install.
3. Where the safety story still feels thin.

Repo + plain FAQ: https://github.com/emersonbusson/ramshared
```

---

## IMG-1 — Image

| | |
| --- | --- |
| **File** | `docs/marketing/cascade-diagram.png` |
| **Download** | https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png |
| **How** | Download → upload on the Reddit post |

---

## FLAIR-1

`Show & Tell` — skip if the subreddit has no flair list.

---

## After this post

Next file (not today): [`02-twitter-en.md`](02-twitter-en.md)
