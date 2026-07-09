# Post 02 — Twitter / X (English thread)

**Post ID:** `POST-02`  
**When:** After **POST-01** is live (+2–6 hours or next day)  
**Where:** https://twitter.com / https://x.com  
**Language:** English  

---

## Steps

| Step | ID | Action |
| --- | --- | --- |
| 1 | **S1** | Open X → New post → start a **thread** |
| 2 | **S2** | Tweet 1 → paste **X-EN-1** + attach **IMG-1** |
| 3 | **S3** | Add tweet → **X-EN-2** |
| 4 | **S4** | Add tweet → **X-EN-3** |
| 5 | **S5** | Add tweet → **X-EN-4** |
| 6 | **S6** | Add tweet → **X-EN-5** |
| 7 | **S7** | Publish thread |
| 8 | **S8** | Stop (optional: reply with link to Reddit post if it has traction) |

**IMG-1:** `docs/marketing/cascade-diagram.png`  
https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## X-EN-1 — Title / first tweet (hook)

```text
Your GPU sits ~90% idle while your laptop swaps compile jobs to SSD.

I open-sourced RamShared: when RAM is tight, borrow idle GPU memory as a backup cushion — and give it back if the GPU needs it.

https://github.com/emersonbusson/ramshared
```

---

## X-EN-2

```text
Why not “just swapon the GPU”?

Under host GPU pressure we measured ~1.2s stalls on tiny reads. Slow emergency memory as your *first* store freezes the machine. So GPU is second cushion only.
```

---

## X-EN-3

```text
Order of cushions:

1) compressed RAM  — first, fast
2) idle GPU memory — second
3) disk            — last

If the PC needs the GPU → we give that cushion back → data goes to disk → apps keep running.
```

---

## X-EN-4

```text
Measured:
• ~241µs vs ~326µs median on two plumbing paths (multi-run)
• ~500 MB on GPU tier · ~480 MB moved back · 0 corruption

Rust. Linux/WSL2. NVIDIA day one.
```

---

## X-EN-5

```text
Limits: not free RAM for maxed games. No thrash on live WSL2. Not every GPU/OS yet.

What would you challenge first—the second-cushion idea or the give-it-back path?
```

---

## Next

- PT thread: [`03-twitter-pt.md`](03-twitter-pt.md)  
- Or BR Reddit: [`04-reddit-brdev-pt.md`](04-reddit-brdev-pt.md)
