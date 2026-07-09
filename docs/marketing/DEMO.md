# Demo script — 30–45 seconds (record as GIF / Short / clip)

**Goal:** A stranger understands RamShared without reading the SPEC.

**Asset:** attach [`cascade-diagram.png`](cascade-diagram.png) as the first frame or thumbnail.

## Spoken / subtitle script (EN, ~40s)

| t | Visual | Line |
| --- | --- | --- |
| 0–5s | Diagram PNG | “Your GPU often sits idle while your laptop swaps to disk.” |
| 5–12s | Terminal: before or empty | “RamShared adds idle VRAM as a *cold* swap cushion.” |
| 12–22s | `sudo ramshared up …` | “One command starts zram, then VRAM, then keeps disk as last resort.” |
| 22–32s | `swapon --show` | “Success: three lines — hot, cold, last.” |
| 32–40s | Diagram DEMOTE box | “If the host steals GPU memory, we DEMOTE — pages go to disk, apps keep running.” |
| 40–45s | Repo URL | “Open source. github.com/emersonbusson/ramshared” |

## Spoken / subtitle script (PT, ~40s)

| t | Visual | Line |
| --- | --- | --- |
| 0–5s | Diagrama | “Sua GPU fica ociosa enquanto o PC troca memória no SSD.” |
| 5–12s | Terminal | “RamShared usa VRAM ociosa como colchão *frio* de swap.” |
| 12–22s | `up` | “Um comando sobe zram, depois VRAM, disco por último.” |
| 22–32s | `swapon --show` | “Sucesso: três linhas — quente, frio, último.” |
| 32–40s | DEMOTE | “Se o host cobrir a GPU, fazemos DEMOTE — páginas vão pro disco, apps seguem.” |
| 40–45s | URL | “Open source. github.com/emersonbusson/ramshared” |

## Terminal sequence (record this)

```bash
# After ./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
# leave up 5s, then:
sudo ./target/release/ramshared down
swapon --show
```

**Success on camera:** first `swapon --show` shows zram + VRAM device + disk; after `down`, VRAM tier is gone.

## Tools (pick one)

- **GIF:** [asciinema](https://asciinema.org/) + `agg`, or Peek / Kap  
- **Short video:** OBS 720p, terminal font large, no secrets on screen  
- **Static only:** post `cascade-diagram.png` alone (better than nothing)

## Do not show on camera

- Full `doctor` walls of text (cut to 2s max)  
- Kernel addresses / tokens  
- Thrash / stress that freezes the host  

## Optional caption (X / LinkedIn)

```text
Idle GPU → cold swap cushion. DEMOTE under WDDM pressure.
https://github.com/emersonbusson/ramshared
```
