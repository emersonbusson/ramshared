# Runbook — Kernel WSL2 custom para a Fase B (zram-writeback + ublk)

Destrava o **Passo 3 (IMPL) dos itens 4-5** (`docs/zram-writeback-vram/`, `docs/ublk-backend/`).
O kernel prebuilt da Microsoft **não** tem os configs (verificado: `# CONFIG_ZRAM_WRITEBACK is
not set`, `# CONFIG_BLK_DEV_UBLK is not set`). `CONFIG_IO_URING=y` **já existe**.

> **Atenção:** o passo de **boot** exige `wsl --shutdown` (no Windows) — encerra TODAS as sessões
> WSL, inclusive a do agente. Por isso o build/install é runbook (o dono controla o restart).

## 0. Pré-requisitos (no WSL2)

```sh
# Deps de build de kernel (faltam: flex, bison, libelf-dev).
sudo apt-get update
sudo apt-get install -y build-essential flex bison libelf-dev libssl-dev bc dwarves \
  python3 pahole cpio
```

## 1. Fonte do kernel (tag = versão em uso)

```sh
uname -r   # ex.: 6.6.114.1-microsoft-standard-WSL2  → use a tag linux-msft-wsl-6.6.y
cd ~
git clone --depth 1 --branch linux-msft-wsl-6.6.y \
  https://github.com/microsoft/WSL2-Linux-Kernel.git
cd WSL2-Linux-Kernel
```

## 2. Config: base Microsoft + os 2 CONFIGs da Fase B

```sh
# Base = config oficial do WSL2 (já vem no repo em Microsoft/config-wsl).
cp Microsoft/config-wsl .config
# Habilita os gatekeepers da Fase B:
./scripts/config --file .config --module  CONFIG_ZRAM_WRITEBACK   # writeback do zram (item 4)
./scripts/config --file .config --module  CONFIG_BLK_DEV_UBLK      # ublk_drv (item 5)
./scripts/config --file .config --enable  CONFIG_IO_URING          # já =y; garante
make olddefconfig
# Confirme:
grep -E "CONFIG_ZRAM_WRITEBACK|CONFIG_BLK_DEV_UBLK|CONFIG_IO_URING" .config
```

## 3. Build (pesado — cuidado no WSL2)

```sh
# -j limitado evita travar o WSL2 (regra MEMORY: builds pesados podem congelar).
make -j"$(($(nproc)/2))" 2>&1 | tee /tmp/kbuild.log
# Saída: ./arch/x86/boot/bzImage
ls -la arch/x86/boot/bzImage
```

## 4. Install (Windows-side)

```sh
# Copie o bzImage para um caminho Windows (ex.: C:\wsl\kernel-ramshared).
mkdir -p /mnt/c/wsl
cp arch/x86/boot/bzImage /mnt/c/wsl/kernel-ramshared
```

No **Windows**, `%UserProfile%\.wslconfig`:

```ini
[wsl2]
kernel=C:\\wsl\\kernel-ramshared
```

## 5. Boot (encerra a sessão do agente)

```powershell
# No PowerShell/CMD do Windows:
wsl --shutdown
# Reabra o WSL.
```

## 6. Verificação pós-boot (nova sessão)

```sh
uname -r                                   # deve refletir o kernel novo
zcat /proc/config.gz | grep -E "ZRAM_WRITEBACK|BLK_DEV_UBLK"   # ambos m/y
sudo modprobe ublk_drv && ls /dev/ublk-control   # item 5 disponível
# zram writeback: backing_dev passa a existir após zram alocado.
```

## 7. Então: Passo 3 da Fase B (SSDV3)

- **Item 4** (`docs/zram-writeback-vram/SPECv2.md`): a recomendação ativa é **NÃO** implementar o
  backing userspace (reentrância sob reclaim + DEMOTE sem drenagem) — preferir block device de
  VRAM **kernel-side** OU manter a cascata de 2 tiers. Reabrir SPEC se o caminho kernel-side for
  perseguido. **Não** basta o kernel ter o config; o desenho seguro exige o driver kernel-side.
- **Item 5** (`docs/ublk-backend/SPECv2.md` + [`ADR-0004`](../decisions/ADR-0004-ublk-io-uring-crate.md)):
  IMPL do servidor ublk reusando o worker H1; `io-uring` crate (ADR-0004); `--swap-dev` genérico;
  **bench latência ublk vs NBD** (gate de adoção — sem ganho, manter NBD).

## Rollback

- Remover a linha `kernel=` do `.wslconfig` + `wsl --shutdown` → volta ao kernel prebuilt da MS.
  App-only; nenhum dado é tocado (a cascata Day-0 de 2 tiers segue funcionando no kernel padrão).
