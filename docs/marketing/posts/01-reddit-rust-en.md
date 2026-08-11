# Post 01 — Reddit r/rust (English)

**Yes: almost everything below is for PASTING into Reddit.**
There are **3 things** to paste/attach — not just one block.

| What | ID | Where on Reddit | How to use it |
| --- | --- | --- | --- |
| **Title** | **T-EN-1** | **Title** field | Copy the text between `>>> COPY TITLE START` and `>>> COPY TITLE END` |
| **Body** | **B-EN-1** | **Text** field (post body) | Copy the text between `>>> COPY BODY START` and `>>> COPY BODY END` |
| **Image** | **IMG-1** | image / media button | Download and attach the PNG (it is not text) |

**Do not paste** lines that begin with `>>>` — they are only delimiters.

---

## Steps (in order)

| Step | What to do |
| --- | --- |
| **S1** | Open https://www.reddit.com/r/rust → **Create Post** → **Text** type |
| **S2** | Community = **r/rust** |
| **S3** | In the **Title** field, paste **T-EN-1** (the block below) |
| **S4** | In the **Text** field, paste **B-EN-1** (the large block below) |
| **S5** | Attach **IMG-1** (link at the end) |
| **S6** | Select **Show & Tell** flair if it appears |
| **S7** | Click **Post** |
| **S8** | **Stop.** Do not post elsewhere today |

---

## T-EN-1 — paste into TITLE

>>> COPY TITLE START

[Show & Tell] RamShared — idle GPU memory as a backup cushion on Linux/WSL2 (when RAM is tight, borrow the GPU — give it back if the GPU needs it)

>>> COPY TITLE END

---

## B-EN-1 — paste into TEXT / BODY

Everything between START and END below (including “Numbers”, “Try it”, “Honest
limits”, the repository link, and so on) is **one single text** for the post
body.
Yes: the numbers, `quickstart.sh`, and “Looking for feedback” **are also for
pasting**.

>>> COPY BODY START

I got tired of the machine thrashing the SSD while the GPU sat there with empty memory.

So I wrote **RamShared** (Rust, Linux/WSL2, NVIDIA). When RAM is tight it borrows **idle** GPU memory as a second cushion. If Windows needs the card for a game or render, it **gives that memory back**. Apps keep running.

Important: GPU memory is **not** as safe/fast as main RAM. Under reclaim we saw a tiny read take about **1.2 seconds**. Put that first and the box freezes. So the order is:

```
zram (compressed RAM)  →  idle GPU  →  disk
```

Measured, not vibes:

- ~1.2 s tiny read in the bad reclaim case (why GPU is second)
- ~500 MB on the GPU tier, ~480 MB moved back, **0** corruption in the logged drill

```
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
```

Optional boot on WSL (opt-in, refuses dirty state): `scripts/safety/install-cascade-boot.sh --enable`

Not free RAM for maxed-out games. Not a Windows kernel driver for your daily laptop. Looking for people who’ve fought swap / CUDA / WSL2 and will tell me where this still feels thin.

https://github.com/emersonbusson/ramshared

>>> COPY BODY END

---

## IMG-1 — is NOT text (attach the file)

1. Open: https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png
2. Download the PNG (Download / raw).
3. Attach that image to the Reddit post.

---

## Mental checklist

- [ ] Title = only the **T-EN-1** line
- [ ] Body = **all** of **B-EN-1** (from “When your PC…” through the GitHub link)
- [ ] Image attached
- [ ] Posted and stopped

Next channel (not today): [`02-twitter-en.md`](02-twitter-en.md)
