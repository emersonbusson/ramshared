# FAQ — plain answers

No essay required. Technical sources are at the bottom if you care.

## In one sentence

| | |
| --- | --- |
| **EN** | When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy. |
| **PT** | Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar. |

---

## Will this break my PC?

**We designed it so it shouldn’t.**

Linux already uses the **disk** as emergency memory when RAM is full. RamShared only:

1. Adds a **middle cushion** (idle GPU memory), and  
2. Adds a **way out** if the GPU is needed again.

| You might fear… | Plain answer |
| --- | --- |
| Freeze / blue screen | When Windows is reclaiming graphics memory, that memory can get **very slow** (we measured about **1.2 seconds** for a tiny read). So we never treat GPU memory as the *first* place for “hot” data. If pressure rises, we **stop using the GPU cushion** and data goes to disk. Your apps keep running. |
| Corruption | In a logged stress run we put about **500 MB** on the GPU cushion and moved about **480 MB** back to disk with **no corruption**. |
| Can’t undo | `sudo ./target/release/ramshared down` turns everything off. |
| WSL2 freezes forever | We **don’t** run “thrash until it dies” tests on your daily WSL2. Heavy tests use a **separate VM**. |

### “Give it back” in four lines (engineers call this DEMOTE)

1. We watch whether emergency memory is getting **too slow**.  
2. If the host is reclaiming GPU memory (or latency spikes), we **stop using the GPU as swap**.  
3. Linux moves that data to the **next place** (usually disk).  
4. Your processes **keep running** — we don’t kill them to free the GPU.

---

## What do I need?

- **Linux** or **WSL2** (Windows Subsystem for Linux)  
- An **NVIDIA** graphics card  
- Drivers that make `nvidia-smi` work  
- **Rust** to build ([rustup.rs](https://rustup.rs/))  
- **sudo** (admin) to change swap  

```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check     # is this machine ready?
sudo ./target/release/ramshared doctor    # if not: fix list
```

---

## How do I know it worked?

```bash
swapon --show
```

You should see about **three** lines of emergency memory:

| Priority | Everyday name | What it is |
| --- | --- | --- |
| First (high) | Compressed RAM | Often shows as **zram** |
| Second | GPU cushion | A device backed by **idle graphics memory** |
| Last (low) | Disk | Normal swap file / VHDX |

Names differ by machine. **Order** (fast → slow) matters more than the exact labels.

---

## Is this free RAM for games?

**No.**

If a game already fills the GPU, there is little **idle** memory to borrow. RamShared is for **workstation pressure** — compiling, containers, too many browser tabs — when the GPU is often sitting around.

---

## Why not only use compressed RAM (zram)?

Compressed RAM helps a lot, but it still uses **system RAM**.  
RamShared adds **idle graphics memory** as an **extra** cushion when system RAM + compression are not enough.

We don’t claim to replace every other tool. We claim a **useful middle layer**.

---

## Does it work on AMD or Intel GPUs?

**Day one: NVIDIA only** (CUDA path).  
Other brands are future work. We won’t pretend multi-vendor support exists today.

---

## What about “real servers” / fancy hardware (CXL, etc.)?

Interesting long-term. **Today’s shippable path** is a normal developer machine: Linux or WSL2 + NVIDIA GPU. See `ROADMAP.md` if you like roadmaps.

---

## Is the multi-machine broker / Windows driver included?

**Not in the first install path.**  

Day one is: **one PC**, three commands (`check` / `up` / `down`).  
Other tracks live in design docs for later.

---

## Where do the numbers come from?

| Claim in plain words | Rough number | Written up in |
| --- | --- | --- |
| Tiny read while host steals GPU memory | up to **~1.2 s** | `docs/reliability/wsl2-fase0-final.md` |
| Faster GPU swap path | **~241 µs** median | `docs/reliability/memory-broker-p0-results.md` |
| Older GPU swap path | **~326 µs** median | same |
| Stress: put data on GPU tier / move it back | **~500 / ~480 MB**, **0 corruption** | `docs/reliability/wsl2-cascade-validation.md` |

If we publish a new public number, it has to be written down there first.

---

## Something went wrong

1. `sudo ./target/release/ramshared down`  
2. `swapon --show` — GPU cushion should be gone  
3. `sudo ./target/release/ramshared doctor`  
4. Open a GitHub issue with the **doctor** text (no passwords, no secrets)

---

## Glossary (only if you want it)

| Word you may see | Plain meaning |
| --- | --- |
| **Swap** | Disk (or other store) used as emergency RAM |
| **VRAM** | Memory on the graphics card |
| **zram** | Compressed system RAM used as a fast cushion |
| **WSL2** | Linux inside Windows |
| **WDDM** | Windows graphics driver model (can reclaim GPU memory) |
| **DEMOTE** | Our “give the GPU cushion back” action |
| **NBD / ublk** | Ways Linux talks to a block device in userspace (plumbing) |
