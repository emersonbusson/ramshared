# IMPL — RamShared P4 / Trilha 2: swap-para-VRAM no Windows nativo (StorPort virtual miniport)

> SSDV3 PASSO 3. Implementa `SPEC.md` em `docs/specs/no-milestone/windows-swap-driver/`.
> Branch: `main`. PR: ainda não.
>
> **Disciplina Kahneman (campanha 2026-07-09):** #1 WYSIATI, #2 checkpoint, #3 números, #5/#13 fail paths,
> #15 sem retry em gap determinístico, #16 sem thrash no host, RNF-6 VM-only.

## Status

**lab-complete / host-real blocked** · gates:

| Gate | Resultado |
| --- | --- |
| `cargo test --workspace` (Linux) | ✓ **233** lib/bin tests pass, **22** ignored (GPU/Vulkan/etc.), **0** fail |
| `cargo fmt --check` / `clippy -D warnings` | ✓ limpo |
| RNF-8 `qemu-ublk-daemon.sh` | ✓ **PASS** (serve + teardown limpo) |
| RNF-8 `qemu-ublk-crash-e1b.sh` | ✓ **PASS** `KTEST-E1B-VERDICT=CONTAINED-SIGBUS` (exit 42 SIGBUS contido; bystander vivo) |
| Hyper-V elevated path (`sudo.exe` from WSL) | ✓ `IsAdmin=True`, `Get-VM` + PSD |
| VM `win11-drill` campaign | ✓ **PASS_WITH_SKIPS** (fail paths honestos) |
| WDK + VS Build Tools (host) | ✓ instalados; `Build-Drivers.ps1` produz `.sys` |
| `ramshared.sys` / `poolstress.sys` build | ✓ 26624 / 7680 bytes (x64 Release) |
| Load on `win11-drill` (test-sign) | ✓ **RUNNING**; `\\.\RamSharedCtl` OK; INF+devcon Root\RamShared |
| Get-Disk LUN product | ✓ **N=1 RAMSHARE VRAMDISK 64 MiB** (2026-07-09 clean PnP) |
| poolstress IOCTL ALLOC/FREE 1 GiB | ✓ ok=True (rodada anterior) |
| Format NTFS + smoke file | ✓ **PASS** (2026-07-09): LUN 64 MiB, `format /fs:NTFS` OK, smoke file; backend `maxIo=1MiB` |
| ITEM-8 residency DT-21 (pagefile-VRAM) | ✓ **PASS** (Usage 25%, KPD 3/3). B1 safe **PASS**; pagefile-hot → 0x7A (DT-9). Lab SCM **PASS**. Host-real **FORBIDDEN**. |
| DT-9 ordered teardown | ✓ pure tests; lab **PASS_DT9_REFUSE_KILL** + **PASS_DT9_REBOOT_KILL** |
| Lab SCM autostart | ✓ **PASS_LAB_SCM** `RamSharedWinSvc` delayed-auto |
| B1 safe surprise-remove | ✓ **PASS_B1_SAFE_ARM** (no secondary PF; kill backend; no new dump) |
| ITEM-9 K (p99 VRAM vs disk) | ✗ harness OK; **K não inventado** (DT-13) |
| ITEM-10 soak 72 h | ✗ script only |
| ITEM-11 attestation | ✗ R9 org + no `.sys` |
| Host-real driver load | **proibido** — lab ITEM-8 evidence green; host-real needs CUDA product path + B1 |

## WYSIATI (#1) — o que **não** foi visto

- Build EWDK de `ramshared.sys` / `poolstress.sys` (sem WDK no guest; sem `link.exe` MSVC).
- `Cuda::load` / `nvcuda.dll` no Windows (guest sem GPU).
- Pagefile secundário em volume StorPort RamShared (disco produto ainda não carrega).
- Soak Driver Verifier 3×24 h; attestation Partner Center.
- SDV report; InfVerif limpo em build real.

Sem isso, **não** se afirma “driver Windows pronto” nem ITEM-8 PASS.

## Arquivos (RF/ITEM → mudança)

| Arquivo | ITEM / RF | O que foi feito |
| --- | --- | --- |
| `crates/ramshared-cuda` loaders | ITEM-1 (RF-4) | Cross-platform loaders; residual Windows `mem_info` |
| `crates/ramshared-block/src/vram_backend.rs` | ITEM-2 (RF-3) | `VramBackend` promovido |
| `crates/ramshared-broker` + `wsl2d` | ITEM-3 (RF-5) | `TransportKind::WinDrive` + `on_tick` filter |
| `crates/ramshared-winsvc/` | ITEM-3/4/6/7 | lib pure + stub bin |
| `drivers/windows/ramshared/*` | ITEM-5 | StorPort fontes + INF/vcxproj |
| `drivers/windows/tools/poolstress/*` | ITEM-8 | test driver VM-only |
| `scripts/windows/*` | ITEM-8..11 | harnesses + `wsl-elevated-ps.sh` + `Invoke-DisciplinedCampaign.ps1` |

## Decisões pequenas (sem nova ADR)

- Golden SQE sem `unsafe` no winsvc.
- `NtCreatePagingFile` FFI fail-closed até host Windows com allow-list 26200.
- `StorPortNotification` DeviceExtension real amarra no 1º build EWDK.
- Campanha classifica `link.exe`/WDK ausente como **SKIP** (#15 determinístico), não FAIL de produto.
- Senha PSD só via env `RAMSHARED_DRILL_PASSWORD` (não no git).

## Validação (números)

### Linux (host WSL2)

| Métrica | Valor |
| --- | --- |
| Unit/lib tests pass | **233** (soma `test result: ok. N passed` do workspace) |
| Ignored | **22** |
| Failed | **0** |
| fmt / clippy -D warnings | limpo |
| `qemu-ublk-daemon` | **PASS** |
| `qemu-ublk-crash-e1b` | **PASS** CONTAINED-SIGBUS; victim exit **42**; bystander HB 73→89; elapsed kill→device-gone **2500 ms** |

### Hyper-V `win11-drill` (elevated `sudo.exe`)

| Métrica | Valor |
| --- | --- |
| Guest build | **26200.8037** |
| test-signing | **Yes** |
| Free C: | **~14 GB** |
| Checkpoint | `disciplined-20260709-112351` (#2 rollback surface) |
| Preflight | exit **0** |
| ps1-parse | **6/6** files |
| ITEM-8 DT-21 fail-path | exit **3** INCONCLUSIVO (#13 honesto) |
| Measure n=3 idle | PASS; median_ms ≈ **125** (campanha) / **92** (rodada anterior) |
| Revoke missing service | exit **2** fail-closed |
| rustup | cargo/rustc **1.97.0** instalados no guest |
| `cargo test` no guest | **SKIP** — `link.exe` ausente (precisa VS Build Tools); lógica pure já verde no Linux |
| WDK / nvcuda | **SKIP** |

Artefatos: `C:\Users\emedev\ramshared-drill\agent-disciplined-results.json`, `artifacts-disciplined\`.

### Como reproduzir (disciplina operacional)

```bash
# 1) Linux gates
cargo test --workspace && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/kernel/qemu-ublk-daemon.sh
./scripts/kernel/qemu-ublk-crash-e1b.sh

# 2) Espelhar + Hyper-V elevado
rsync -a --delete --exclude target --exclude .git \
  ./ /mnt/c/Users/emedev/ramshared-src/
export RAMSHARED_DRILL_PASSWORD='…'  # lab only
./scripts/windows/wsl-elevated-ps.sh -File \
  'C:\Users\emedev\ramshared-src\scripts\windows\Invoke-DisciplinedCampaign.ps1'
```

## Gaps

| Classe | Itens |
| --- | --- |
| **Fechados (lab VM)** | ITEM-1..7 pure/Linux; ABI; `.sys` load; LUN/format; pagefile DT-21; KPD 3/3; DT-9; B1 safe; lab SCM; DEGRADATION-MATRIX; RNF-8 ublk drills |
| **Env-bound / open** | Product CUDA `nvcuda` no Windows; MSVC+cargo para `ramshared-winsvc` nativo; ITEM-9 K medido; soak 72h; attestation R9; **host-real load** |
| **By design fail** | B2 pagefile-hot kill → **0x7A** — mitigação = DT-9, não “PASS inventado” |

## Doc surface (maturidade 2026-07-09)

Root docs alinhados ao status acima: `README.md`, `ROADMAP.md`, `ARCHITECTURE.md`, `drivers/windows/README.md`, `PREFLIGHT.md`, `docs/FAQ.md`, `validation.md`.
| **Abertos de código** | DeviceExtension real no StorPort complete; FFI `NtCreatePagingFile` Windows; SCM `windows-service` main |

## Rollback trigger

- BugCheck em host não-VM → parar qualquer load no host; só VM.
- Regressão `cargo test` Linux ou `qemu-ublk-*` FAIL → `git revert` do ITEM tocado (RNF-8).
- ITEM-8 “PASS” sem `% Usage` pagefile-VRAM > 0 → **inválido** (teatro #13); reclassificar INCONCLUSIVO.
- Pagefile ativo + destroy disco → incidente B1 (DT-9).
- Checkpoint Hyper-V `disciplined-*` disponível para Apply se campanha corromper guest.

## Traceability

| PRD | SPEC ITEM | Commit(s) |
| --- | --- | --- |
| RF-3 / RNF-8 | ITEM-2 | `28a7960` |
| RF-5 | ITEM-3 broker | `ae9cc44` |
| RF-3/5/6 | ITEM-3/4/6/7 winsvc | `2145401` |
| RF-1/2 | ITEM-5 | `f149541` |
| RF-7 / RNF-* | ITEM-8..11 scripts | `d2f87f5`, `58c6986` |
| ops | elevated Hyper-V | `cc2ec0d` |
| ops | disciplined campaign | (este commit) |


## Load evidence (2026-07-09, win11-drill)

| Step | Result |
| --- | --- |
| Host toolchain | VS 2022 BuildTools 17.14 + WDK 10.0.26100 + `storport.h`/`storport.lib` |
| Build | `scripts/windows/Build-Drivers.ps1` → `ramshared.sys` (26624 B), `poolstress.sys` (7680 B) |
| Sign | self-signed CodeSigning SHA256 (`signtool`); guest testsigning Yes |
| First start unsigned | **EC 577** (ERROR_INVALID_IMAGE_HASH) — expected |
| After sign + start | **poolstress RUNNING**, **ramshared RUNNING** |
| Device open | `\\.\RamSharedPoolStress` ok; `\\.\RamSharedCtl` ok |
| poolstress IOCTL | ALLOC 1 GiB ok; FREE ok; no BugCheck event |
| Guest | ALIVE freeGB≈7.2; checkpoint `pre-driver-load-*` available |

**Still blocked for ITEM-8 PASS:** pagefile-VRAM on product disk + DT-21 `% Usage` > 0 + kill service B1/B2 with residency. StorPort virtual disk still needs INF/PnP path for full SCSI volume (service load proves DriverEntry + control device).


## Campanha DT-25 (2026-07-09) — WYSIATI

- **Visto:** LUN N=1 `RAMSHARE VRAMDISK` 67108864 bytes apos `devcon install` + CREATE_DISK; adapter `ROOT\SCSIADAPTER\0000` OK.
- **Visto:** BSOD **0xD1** no format (IRQL 2) — root cause: `Srb->DataBuffer` com `STOR_MAP_NON_READ_WRITE_BUFFERS`.
- **Codigo:** dispatch forward (control vs StorPort); Inf2Cat; MDL cap 4 MiB; cancel COMMIT; StorPortGetSystemAddress + LBA CDB.
- **Nao visto ainda:** pagefile-V com `% Usage`>0; KPD PASS; format verde pos-fix 0xD1 (re-test limpo 1 adapter).
- **Proibido:** host-real load; dual-path ImDisk.
