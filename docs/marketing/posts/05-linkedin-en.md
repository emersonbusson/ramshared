# Post 05 — LinkedIn (English)

**Post ID:** `POST-05`  
**When:** After **POST-01** (+2 days)  
**Where:** LinkedIn  
**Language:** English  

---

## Steps

| Step | ID | Action |
| --- | --- | --- |
| 1 | **S1** | LinkedIn → **Start a post** |
| 2 | **S2** | Paste body → **LI-EN-1** |
| 3 | **S3** | Optional: attach **IMG-1** |
| 4 | **S4** | Post |
| 5 | **S5** | Stop |

**IMG-1 (optional):** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## LI-EN-1 — Full post (no separate title field)

LinkedIn has no separate “title”; the first line is the hook.

```text
When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.

I open-sourced RamShared (Rust, Linux/WSL2, NVIDIA).

Problem: workstations thrash the SSD when RAM is gone, while graphics memory often sits almost empty.

Why not dump all emergency memory on the GPU? Under host pressure we measured ~1.2s stalls on tiny reads — fine for data, fatal as your *first* cushion. So GPU memory is second; we can hand it back without killing apps.

Order: compressed RAM → idle GPU memory → disk.

Measured: ~241µs vs ~326µs on two paths; ~500MB / ~480MB stress with 0 corruption logged.

https://github.com/emersonbusson/ramshared

Curious how others handle memory pressure + GPU on WSL2 / hybrid hosts.
```

---

## Next

[`06-linkedin-pt.md`](06-linkedin-pt.md)
