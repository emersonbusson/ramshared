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
