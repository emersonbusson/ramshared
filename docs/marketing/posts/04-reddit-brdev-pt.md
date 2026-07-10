# Post 04 — Reddit r/brdev (português)

**Sim: o título e o corpo abaixo são para COLAR no Reddit.**  
Há **3 coisas** — não um bloco só.

| O quê | ID | Onde no Reddit | Como usar |
| --- | --- | --- | --- |
| **Título** | **T-PT-1** | campo **Title** / **Título** | Copia entre `>>> COPY TITLE START` e `>>> COPY TITLE END` |
| **Corpo** | **B-PT-1** | campo **Text** / **Texto** | Copia entre `>>> COPY BODY START` e `>>> COPY BODY END` |
| **Imagem** | **IMG-PT-1** | botão de imagem | Baixa o PNG **em português** e anexa (não é texto) |

**Não cole** as linhas que começam com `>>>` — são só marcadores.

**Quando:** só **depois** do post no r/rust (Post 01).  
**Onde:** https://www.reddit.com/r/brdev  

---

## Passos (ordem)

| Passo | O que fazer |
| --- | --- |
| **S1** | Abre https://www.reddit.com/r/brdev → **Criar post** → tipo **Texto** |
| **S2** | Comunidade = **r/brdev** |
| **S3** | No campo **Título**, cola **T-PT-1** (bloco abaixo) |
| **S4** | No campo **Texto**, cola **B-PT-1** (bloco grande abaixo) |
| **S5** | Anexa a imagem **IMG-PT-1** (diagrama em PT-BR) |
| **S6** | Flair se existir (projeto / show) |
| **S7** | Clica **Postar** |
| **S8** | **Para.** |

---

## T-PT-1 — cola no TÍTULO

>>> COPY TITLE START

[Show] RamShared — quando a RAM aperta, usa memória ociosa da GPU (Linux/WSL2) e devolve se a placa precisar

>>> COPY TITLE END

---

## B-PT-1 — cola no TEXTO / CORPO

Tudo entre START e END (números, comandos, limites, feedback, link) é **um único texto** para o corpo.  
**Sim: também é para colar.**

>>> COPY BODY START

Cansei do PC engasgando no SSD enquanto a placa de vídeo fica com memória sobrando.

Escrevi o **RamShared** (Rust, Linux/WSL2, NVIDIA). Quando a RAM aperta, ele **empresta** VRAM ociosa. Se você abre um jogo ou render no Windows e a placa precisa da memória, ele **devolve**. Os programas no WSL continuam vivos.

Não é mágica: sob pressão o Windows pode deixar a VRAM **lenta** (medimos ~**1,2 s** numa leitura pequena). Por isso a ordem é:

```
RAM comprimida (zram)  →  GPU ociosa  →  disco
```

Números reais do drill: ~500 MB na GPU, ~480 MB de volta, **0** corrupção.

```
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show
```

Boot no WSL (opcional, se recusa se o estado estiver sujo):  
`sudo bash scripts/safety/install-cascade-boot.sh --enable`

Não é RAM grátis para jogo no talo. Não é driver Windows pro notebook do dia a dia. Quero quem já sofreu com swap/CUDA/WSL e diga onde ainda parece frágil.

https://github.com/emersonbusson/ramshared

>>> COPY BODY END

---

## IMG-PT-1 — NÃO é texto (anexar arquivo em português)

1. Abre: https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram-pt.png  
2. Baixa o PNG.  
3. No post do Reddit, anexa essa imagem.

(Versão em inglês: `cascade-diagram.png` — use **só** a PT neste post.)

---

## Checklist

- [ ] Título = só o que está entre **COPY TITLE START/END**  
- [ ] Corpo = **tudo** entre **COPY BODY START/END** (até o link do GitHub)  
- [ ] Imagem anexada  
- [ ] Postou e parou  

Próximo: [`05-linkedin-en.md`](05-linkedin-en.md)
