# Post 06 — LinkedIn (português)

**O bloco abaixo é para COLAR** no LinkedIn (um post; não tem campo “título” separado).  
**Quando:** depois do Post 01 (+2 dias).  
**Onde:** LinkedIn  

**Não cole** as linhas que começam com `>>>`.

---

## Passos

| Passo | O que fazer |
| --- | --- |
| **S1** | LinkedIn → **Começar publicação** |
| **S2** | Colar **LI-PT-1** (corpo abaixo) |
| **S3** | Opcional: anexar **IMG-PT-1** (diagrama PT-BR) |
| **S4** | Publicar |
| **S5** | Parar |

**IMG-PT-1:** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram-pt.png

---

## LI-PT-1 — cola na caixa do LinkedIn

>>> COPY BODY START

Quando a RAM aperta, usa memória ociosa da placa de vídeo — e devolve se a GPU precisar.

Open source: RamShared (Rust, Linux/WSL2, NVIDIA).

Problema: o PC engasga no SSD quando a RAM acaba, enquanto a memória da GPU muitas vezes está quase vazia.

Por que não jogar tudo na GPU? Sob pressão medimos ~1,2s de stall em leitura pequena — péssimo como *primeira* opção. Por isso a GPU fica como segunda opção; dá para devolver sem matar os apps.

Ordem: RAM comprimida → GPU ociosa → disco.

Medido: ~241µs vs ~326µs; ~500MB / ~480MB no stress com 0 corrupção logada.

https://github.com/emersonbusson/ramshared

Curioso como outras equipes lidam com pressão de memória + GPU em WSL2 / hosts híbridos.

>>> COPY BODY END

---

Próximo (opcional): [`07-hackernews-en.md`](07-hackernews-en.md)
