# Post 06 — LinkedIn (português)

**Post ID:** `POST-06`  
**When:** Depois de **POST-01** (+2 dias), ou no mesmo dia do **POST-05** se a rede for BR  
**Where:** LinkedIn  
**Language:** Português  

---

## Passos

| Passo | ID | Ação |
| --- | --- | --- |
| 1 | **S1** | LinkedIn → **Começar publicação** |
| 2 | **S2** | Colar → **LI-PT-1** |
| 3 | **S3** | Opcional: anexar **IMG-1** |
| 4 | **S4** | Publicar |
| 5 | **S5** | Parar |

**IMG-1:** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## LI-PT-1 — Texto completo (sem campo “título”)

```text
Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.

Open source: RamShared (Rust, Linux/WSL2, NVIDIA).

Problema: o PC engasga no SSD quando a RAM acaba, enquanto a memória da GPU muitas vezes está quase vazia.

Por que não jogar tudo na GPU? Sob pressão medimos ~1,2s de stall em leitura pequena — péssimo como *primeiro* colchão. Por isso a GPU é o segundo; dá para devolver sem matar os apps.

Ordem: RAM comprimida → GPU ociosa → disco.

Medido: ~241µs vs ~326µs; ~500MB / ~480MB no stress com 0 corrupção logada.

https://github.com/emersonbusson/ramshared

Curioso como outras equipes lidam com pressão de memória + GPU em WSL2 / hosts híbridos.
```

---

## Próximo (opcional)

[`07-hackernews-en.md`](07-hackernews-en.md)
