# Demo — 40 seconds anyone can follow

**Goal:** a stranger gets the idea **without** reading architecture docs.

**Thumbnail / first frame:** [`cascade-diagram.png`](cascade-diagram.png)

---

## What you say (EN)

| Time | Show on screen | Say |
| --- | --- | --- |
| 0–5s | Diagram | “Your GPU often sits idle while your laptop crawls on disk swap.” |
| 5–12s | Terminal | “RamShared borrows idle GPU memory as a *backup cushion* when RAM is tight.” |
| 12–22s | `up` command | “One command turns it on: compressed RAM first, then GPU, disk last.” |
| 22–32s | `swapon --show` | “Success looks like three lines — fast, middle, slow.” |
| 32–40s | Diagram “give back” box | “If the PC needs the GPU, we give that cushion back. Apps keep running.” |
| 40–45s | Repo URL | “Open source — github.com/emersonbusson/ramshared” |

## O que você fala (PT)

| Tempo | Tela | Fala |
| --- | --- | --- |
| 0–5s | Diagrama | “Sua GPU fica parada enquanto o PC engasga no SSD.” |
| 5–12s | Terminal | “O RamShared usa memória ociosa da placa quando a RAM aperta.” |
| 12–22s | `up` | “Um comando liga: memória comprimida primeiro, depois GPU, disco por último.” |
| 22–32s | `swapon --show` | “Sucesso: três linhas — rápido, meio, lento.” |
| 32–40s | Caixa de devolver | “Se o PC precisar da GPU, devolvemos essa memória. Os apps continuam.” |
| 40–45s | URL | “Código aberto — github.com/emersonbusson/ramshared” |

---

## Terminal (grave isto)

```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
# pause 5 seconds
sudo ./target/release/ramshared down
swapon --show
```

**On camera, success =** first `swapon --show` has ~3 emergency-memory lines; after `down`, the GPU line is gone.

---

## How to record

| Format | Tool idea |
| --- | --- |
| GIF | Peek, Kap, or asciinema + agg |
| Short video | OBS, large terminal font, 720p is fine |
| No recording | Post the diagram alone + one sentence |

## Don’t show

- Endless error walls  
- Passwords or tokens  
- Anything that freezes the host on purpose  

## Caption (X / LinkedIn)

```text
Idle GPU → backup memory cushion when RAM is tight. We give it back if the GPU needs it.
https://github.com/emersonbusson/ramshared
```

```text
GPU ociosa → ajuda quando a RAM aperta. Devolve se a placa precisar.
https://github.com/emersonbusson/ramshared
```
