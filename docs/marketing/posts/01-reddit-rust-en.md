# Post 01 — Reddit r/rust (English)

**Sim: quase tudo abaixo é para COLAR no Reddit.**  
Há **3 coisas** para colar/anexar — não 1 bloco só.

| O quê | ID | Onde no Reddit | Como usar |
| --- | --- | --- | --- |
| **Título** | **T-EN-1** | campo **Title** | Copia o texto entre as linhas `>>> COPY TITLE START` e `>>> COPY TITLE END` |
| **Corpo** | **B-EN-1** | campo **Text** (corpo do post) | Copia o texto entre `>>> COPY BODY START` e `>>> COPY BODY END` |
| **Imagem** | **IMG-1** | botão de imagem / mídia | Baixa o PNG e anexa (não é texto) |

**Não cole** as linhas que começam com `>>>` — são só marcadores.

---

## Passos (ordem)

| Passo | O que fazer |
| --- | --- |
| **S1** | Abre https://www.reddit.com/r/rust → **Create Post** → tipo **Text** |
| **S2** | Comunidade = **r/rust** |
| **S3** | No campo **Title**, cola **T-EN-1** (bloco abaixo) |
| **S4** | No campo **Text**, cola **B-EN-1** (bloco grande abaixo) |
| **S5** | Anexa a imagem **IMG-1** (link no final) |
| **S6** | Flair **Show & Tell** se aparecer |
| **S7** | Clica **Post** |
| **S8** | **Para.** Não posta em outro lugar hoje |

---

## T-EN-1 — cola no TITLE

>>> COPY TITLE START

[Show & Tell] RamShared — idle GPU memory as a backup cushion on Linux/WSL2 (when RAM is tight, borrow the GPU — give it back if the GPU needs it)

>>> COPY TITLE END

---

## B-EN-1 — cola no TEXT / BODY

Tudo entre START e END abaixo (incluindo “Numbers”, “Try it”, “Honest limits”, o link do repo, etc.) é **um único texto** para o corpo do post.  
Sim: os números, o `quickstart.sh` e o “Looking for feedback” **também são para colar**.

>>> COPY BODY START

**When your PC runs out of RAM, use idle GPU memory as a safety cushion — and give it back if the GPU gets busy.**

I built **RamShared** (Rust, Linux/WSL2, NVIDIA): a practical way to borrow **idle graphics memory** when system RAM is tight, without pretending GPU memory is as safe/fast as main RAM.

## Problem (human)

You’re compiling / running containers / drowning in tabs. RAM is gone. The machine starts thrashing the **SSD**. Meanwhile the **GPU memory** is often almost empty. You already paid for that silicon.

## Why not “just put all swap on the GPU”?

When Windows reclaims graphics memory under pressure, that memory can get **very slow**. We measured about **1.2 seconds** for a tiny read in the bad case. If that were your *first* emergency store, the whole machine freezes. So GPU memory is only a **second** cushion — and we can **give it back**.

## Design (still short)

```
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

```
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

>>> COPY BODY END

---

## IMG-1 — NÃO é texto (anexar arquivo)

1. Abre: https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png  
2. Baixa o PNG (Download / raw).  
3. No post do Reddit, anexa essa imagem.

---

## Checklist mental

- [ ] Title = só a linha do **T-EN-1**  
- [ ] Body = **tudo** do **B-EN-1** (do “When your PC…” até o link do GitHub)  
- [ ] Imagem anexada  
- [ ] Postou e parou  

Próximo canal (não hoje): [`02-twitter-en.md`](02-twitter-en.md)
