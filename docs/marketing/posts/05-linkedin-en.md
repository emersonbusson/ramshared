# Post 05 — LinkedIn (English)

**The block below is FOR PASTING** into LinkedIn (one post, no separate title field).  
**When:** after Post 01 (+2 days).  
**Where:** LinkedIn  

**Do not paste** the lines that start with `>>>`.

---

## Steps

| Step | Action |
| --- | --- |
| **S1** | LinkedIn → **Start a post** |
| **S2** | Paste **LI-EN-1** (body below) |
| **S3** | Optional: attach **IMG-1** |
| **S4** | Post |
| **S5** | Stop |

**IMG-1 (optional):** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## LI-EN-1 — paste into LinkedIn post box

>>> COPY BODY START

When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.

I open-sourced RamShared (Rust, Linux/WSL2, NVIDIA).

Problem: workstations thrash the SSD when RAM is gone, while graphics memory often sits almost empty.

Why not dump all emergency memory on the GPU? Under host pressure we measured ~1.2s stalls on tiny reads — fine for data, fatal as your *first* cushion. So GPU memory is second; we can hand it back without killing apps.

Order: compressed RAM → idle GPU memory → disk.

Measured: ~241µs vs ~326µs on two paths; ~500MB / ~480MB stress with 0 corruption logged.

https://github.com/emersonbusson/ramshared

Curious how others handle memory pressure + GPU on WSL2 / hybrid hosts.

>>> COPY BODY END

---

Next: [`06-linkedin-pt.md`](06-linkedin-pt.md)
