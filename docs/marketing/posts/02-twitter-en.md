# Post 02 — Twitter / X (English thread)

**Each tweet below is FOR PASTING** (one tweet per block).  
**When:** after Post 01 (r/rust), not the same minute.  
**Where:** https://x.com  

**Do not paste** the lines that start with `>>>`.

---

## Steps

| Step | Action |
| --- | --- |
| **S1** | X → New post → make a **thread** |
| **S2** | Tweet 1 = **X-EN-1** + attach **IMG-1** |
| **S3** | + Tweet 2 = **X-EN-2** |
| **S4** | + Tweet 3 = **X-EN-3** |
| **S5** | + Tweet 4 = **X-EN-4** |
| **S6** | + Tweet 5 = **X-EN-5** |
| **S7** | Publish |
| **S8** | Stop |

**IMG-1 (attach, not paste as text):**  
https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## X-EN-1 — paste as tweet 1

>>> COPY TWEET START

Your GPU sits ~90% idle while your laptop swaps compile jobs to SSD.

I open-sourced RamShared: when RAM is tight, borrow idle GPU memory as a backup cushion — and give it back if the GPU needs it.

https://github.com/emersonbusson/ramshared

>>> COPY TWEET END

---

## X-EN-2 — paste as tweet 2

>>> COPY TWEET START

Why not “just swapon the GPU”?

Under host GPU pressure we measured ~1.2s stalls on tiny reads. Slow emergency memory as your *first* store freezes the machine. So GPU is second cushion only.

>>> COPY TWEET END

---

## X-EN-3 — paste as tweet 3

>>> COPY TWEET START

Order of cushions:

1) compressed RAM  — first, fast
2) idle GPU memory — second
3) disk            — last

If the PC needs the GPU → we give that cushion back → data goes to disk → apps keep running.

>>> COPY TWEET END

---

## X-EN-4 — paste as tweet 4

>>> COPY TWEET START

Measured:
• ~241µs vs ~326µs median on two plumbing paths (multi-run)
• ~500 MB on GPU tier · ~480 MB moved back · 0 corruption

Rust. Linux/WSL2. NVIDIA day one.

>>> COPY TWEET END

---

## X-EN-5 — paste as tweet 5

>>> COPY TWEET START

Limits: not free RAM for maxed games. Live WSL2 pressure is supervised and bounded. Not every GPU/OS yet.

What would you challenge first—the second-cushion idea or the give-it-back path?

>>> COPY TWEET END

---

Next: [`03-twitter-pt.md`](03-twitter-pt.md)
