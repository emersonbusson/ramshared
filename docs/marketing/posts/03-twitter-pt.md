# Post 03 — Twitter / X (thread em português)

**Post ID:** `POST-03`  
**When:** Depois de **POST-01** (e de preferência **POST-02**) — +1 dia  
**Where:** https://twitter.com / https://x.com  
**Language:** Português (BR)  

---

## Passos

| Passo | ID | Ação |
| --- | --- | --- |
| 1 | **S1** | Abrir X → Nova postagem → **thread** |
| 2 | **S2** | Tweet 1 → colar **X-PT-1** + imagem **IMG-1** |
| 3 | **S3** | + tweet → **X-PT-2** |
| 4 | **S4** | + tweet → **X-PT-3** |
| 5 | **S5** | + tweet → **X-PT-4** |
| 6 | **S6** | + tweet → **X-PT-5** |
| 7 | **S7** | Publicar |
| 8 | **S8** | Parar |

**IMG-1:** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## X-PT-1 — Primeiro tweet (título da thread)

```text
Sua GPU fica ~90% ociosa enquanto o notebook engasga no swap de SSD.

Open source: RamShared — quando a RAM aperta, empresta memória ociosa da placa como colchão — e devolve se a GPU precisar.

https://github.com/emersonbusson/ramshared
```

---

## X-PT-2

```text
Por que não “só swapon na GPU”?

Sob pressão medimos ~1,2s de stall em leitura pequena. Memória de emergência lenta como *primeiro* recurso trava a máquina. GPU = segundo colchão só.
```

---

## X-PT-3

```text
Ordem dos colchões:

1) RAM comprimida  — primeiro, rápido
2) GPU ociosa      — segundo
3) disco           — último

Se o PC precisar da GPU → devolvemos o colchão → dados vão pro disco → apps seguem.
```

---

## X-PT-4

```text
Medido:
• ~241µs vs ~326µs (medianas, várias rodadas)
• ~500 MB no colchão da GPU · ~480 MB de volta · 0 corrupção

Rust. Linux/WSL2. NVIDIA no dia 1.
```

---

## X-PT-5

```text
Limites: não é RAM grátis para jogo no talo. Sem thrash no WSL2 do dia a dia.

O que você atacaria primeiro: a ideia do segundo colchão ou o “devolver”?
```

---

## Próximo

[`05-linkedin-en.md`](05-linkedin-en.md) ou [`04-reddit-brdev-pt.md`](04-reddit-brdev-pt.md)
