# Post 03 — Twitter / X (thread em português)

**Cada tweet abaixo é para COLAR** (um tweet por bloco).  
**Quando:** depois do Post 01 (+1 dia de preferência).  
**Onde:** https://x.com  

**Não cole** as linhas que começam com `>>>`.

---

## Passos

| Passo | O que fazer |
| --- | --- |
| **S1** | X → Nova postagem → **thread** |
| **S2** | Tweet 1 = **X-PT-1** + anexar **IMG-1** |
| **S3** | + Tweet 2 = **X-PT-2** |
| **S4** | + Tweet 3 = **X-PT-3** |
| **S5** | + Tweet 4 = **X-PT-4** |
| **S6** | + Tweet 5 = **X-PT-5** |
| **S7** | Publicar |
| **S8** | Parar |

**IMG-1:** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## X-PT-1 — cola no tweet 1

>>> COPY TWEET START

Sua GPU fica ~90% ociosa enquanto o notebook engasga no swap de SSD.

Open source: RamShared — quando a RAM aperta, empresta memória ociosa da placa — e devolve se a GPU precisar.

https://github.com/emersonbusson/ramshared

>>> COPY TWEET END

---

## X-PT-2 — cola no tweet 2

>>> COPY TWEET START

Por que não “só swapon na GPU”?

Sob pressão medimos ~1,2s de stall em leitura pequena. Memória de emergência lenta como *primeiro* recurso trava a máquina. GPU só como segunda opção.

>>> COPY TWEET END

---

## X-PT-3 — cola no tweet 3

>>> COPY TWEET START

Ordem de uso:

1) RAM comprimida  — primeiro, rápido
2) GPU ociosa      — segundo
3) disco           — último

Se o PC precisar da GPU → devolvemos essa memória → dados vão pro disco → apps seguem.

>>> COPY TWEET END

---

## X-PT-4 — cola no tweet 4

>>> COPY TWEET START

Medido:
• ~241µs vs ~326µs (medianas, várias rodadas)
• ~500 MB na GPU · ~480 MB de volta · 0 corrupção

Rust. Linux/WSL2. NVIDIA no dia 1.

>>> COPY TWEET END

---

## X-PT-5 — cola no tweet 5

>>> COPY TWEET START

Limites: não é RAM grátis para jogo no talo. Sem thrash no WSL2 do dia a dia.

O que você atacaria primeiro: a ideia da segunda opção ou o “devolver”?

>>> COPY TWEET END

---

Próximo: [`04-reddit-brdev-pt.md`](04-reddit-brdev-pt.md) ou [`05-linkedin-en.md`](05-linkedin-en.md)
