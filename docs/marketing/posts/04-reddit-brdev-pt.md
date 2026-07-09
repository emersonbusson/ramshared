# Post 04 — Reddit r/brdev (português)

**Post ID:** `POST-04`  
**When:** Depois de **POST-01** (+1 dia)  
**Where:** https://www.reddit.com/r/brdev (ou comunidade BR similar)  
**Language:** Português  

---

## Passos

| Passo | ID | Ação |
| --- | --- | --- |
| 1 | **S1** | Abrir r/brdev → **Criar post** → tipo **Texto** |
| 2 | **S2** | Comunidade = **r/brdev** (ou a que você escolheu) |
| 3 | **S3** | Colar título → **T-PT-1** |
| 4 | **S4** | Colar corpo → **B-PT-1** |
| 5 | **S5** | Anexar **IMG-1** |
| 6 | **S6** | Flair se existir (ex.: projeto / show) |
| 7 | **S7** | Publicar |
| 8 | **S8** | Parar |

**IMG-1:** https://github.com/emersonbusson/ramshared/blob/main/docs/marketing/cascade-diagram.png

---

## T-PT-1 — Título

```text
[Show] RamShared — quando a RAM acaba, usa memória ociosa da GPU como colchão (Linux/WSL2) e devolve se a placa precisar
```

---

## B-PT-1 — Corpo

```markdown
**Quando a RAM acaba, usa a memória ociosa da placa de vídeo como colchão — e devolve se a GPU precisar.**

Montei o **RamShared** (Rust, Linux/WSL2, NVIDIA): emprestar **memória ociosa da GPU** quando a RAM do sistema aperta, sem fingir que a memória da placa é tão segura/rápida quanto a RAM principal.

## Problema (humano)
Compile, containers, mil abas. A RAM acaba. O PC engasga no **SSD**. Enquanto isso a **memória da placa de vídeo** está quase vazia — e você já pagou por ela.

## Por que não “jogar todo o swap na GPU”?
Quando o Windows recupera memória da GPU sob pressão, essa memória pode ficar **muito lenta** (medimos cerca de **1,2 s** numa leitura pequena no pior caso). Se isso for o *primeiro* recurso de emergência, a máquina trava. Por isso a GPU é só o **segundo** colchão — e dá para **devolver**.

## Design (curto)
```text
Precisa de memória?  →  1) RAM comprimida     — primeiro, rápido
                     →  2) GPU ociosa         — segundo
                     →  3) disco (SSD/VHDX)   — último
```

Se a latência disparar: **paramos de usar o colchão da GPU**, os dados vão pro disco, **os apps continuam**.

## Números (medidos)
- Pior caso sob pressão da GPU no host: até **~1,2 s** numa leitura pequena.
- Caminho mais rápido ~**241 µs** vs caminho antigo ~**326 µs**.
- Stress: ~**500 MB** no colchão da GPU, ~**480 MB** de volta, **0 corrupção**.

## Experimentar
```bash
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
1. Colchão secundário + devolver vs outras abordagens.
2. O que falta para “só funciona”.
3. Onde a segurança ainda parece frágil.

Repo + FAQ: https://github.com/emersonbusson/ramshared
```

---

## Próximo

[`05-linkedin-en.md`](05-linkedin-en.md)
