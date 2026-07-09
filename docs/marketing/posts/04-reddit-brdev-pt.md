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

**Quando a RAM aperta, o RamShared usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.**

Montei o **RamShared** (Rust, Linux/WSL2, NVIDIA): emprestar **memória ociosa da GPU** quando a RAM do sistema aperta, sem fingir que a memória da placa é tão segura/rápida quanto a RAM principal.

## Problema (humano)

Compile, containers, mil abas. A RAM acaba. O PC engasga no **SSD**. Enquanto isso a **memória da placa de vídeo** está quase vazia — e você já pagou por ela.

## Por que não “jogar todo o swap na GPU”?

Quando o Windows recupera memória da GPU sob pressão, essa memória pode ficar **muito lenta** (medimos cerca de **1,2 s** numa leitura pequena no pior caso). Se isso for o *primeiro* recurso de emergência, a máquina trava. Por isso a GPU entra só como **segunda opção** — e dá para **devolver**.

## Design (curto)

```
Precisa de memória?  →  1) RAM comprimida     — primeiro, rápido
                     →  2) GPU ociosa         — segundo
                     →  3) disco (SSD/VHDX)   — último
```

Se a latência disparar: **paramos de usar a memória da GPU**, os dados vão pro disco, **os apps continuam**.

## Números (medidos)

- Pior caso sob pressão da GPU no host: até **~1,2 s** numa leitura pequena.
- Caminho mais rápido ~**241 µs** vs caminho antigo ~**326 µs**.
- Stress: ~**500 MB** na GPU, ~**480 MB** de volta, **0 corrupção**.

## Experimentar

```
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared up --vram 1024 --zram 1024
swapon --show   # sucesso ≈ três linhas
```

## Limites honestos

- Dia 1: **Linux/WSL2 + NVIDIA**.
- Não é RAM grátis para jogo no talo.
- Não thrashamos WSL2 do dia a dia de propósito.

## Feedback

1. GPU como segunda opção + devolver vs outras abordagens.
2. O que falta para “só funciona”.
3. Onde a segurança ainda parece frágil.

Repo + FAQ: https://github.com/emersonbusson/ramshared

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
