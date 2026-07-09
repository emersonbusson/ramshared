# IMPL — RamShared P4 / Trilha 2: swap-para-VRAM no Windows nativo (StorPort virtual miniport)

> SSDV3 PASSO 3. Implementa `SPEC.md` em `docs/specs/no-milestone/windows-swap-driver/`.
> Branch: `main`. PR: ainda não.

## Status

**parcial** · gates:

| Gate | Resultado |
| --- | --- |
| `cargo test --workspace` | ✓ verde (Linux host) |
| `cargo fmt --all -- --check` | ✓ |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✓ (pós-fmt) |
| RNF-8 drills `qemu-ublk-*` | env-bound (não rodados nesta sessão) |
| WDK MSBuild / SDV / InfVerif | env-bound (sem EWDK neste host) |
| ITEM-8 kernel-page drill (DT-21) | ✗ script pronto; prova empírica pendente em VM |
| ITEM-9 K numérico | ✗ harness pronto; K fixado na 1ª medição real |
| ITEM-10 soak 72 h | ✗ script pronto; 3×24 h pendente |
| ITEM-11 attestation load | ✗ script + R9 org pendente |
| Host-real driver load | **proibido** até ITEM-8 PASS |

## Arquivos (RF/ITEM → mudança)

| Arquivo | ITEM / RF | O que foi feito |
| --- | --- | --- |
| `crates/ramshared-cuda/src/loader_*.rs` + `driver.rs` | ITEM-1 (RF-4) | Já existia (preflight); loader unix/win + `loader::close` no Drop — verificado |
| `crates/ramshared-block/src/vram_backend.rs` | ITEM-2 (RF-3, DT-6) | `VramBackend<M>` promovido; testes FakeVram write/read/OOB/zero |
| `crates/ramshared-block/Cargo.toml` | ITEM-2 | dep `ramshared-vram` |
| `crates/ramshared-wsl2d/src/backend.rs` | ITEM-2 | re-export `ramshared_block::VramBackend`; SliceView/RamBackend locais |
| `crates/ramshared-broker/src/model.rs` | ITEM-3 (RF-5, DT-7) | `TransportKind::WinDrive` |
| `crates/ramshared-wsl2d/src/broker_srv.rs` | ITEM-3 | `endpoint_for(WinDrive)=>None`; `on_tick` exclui WinDrive; testes `windrive_*` |
| `crates/ramshared-winsvc/` | ITEM-3/4/6/7 | lib: config, broker_tenant, driver_link+FakeDriver, ntpagefile, smoke, service, proto |
| `drivers/windows/ramshared/protocol.h` | ITEM-4 (RF-2, DT-17) | ABI congelada (preflight + mantida) |
| `drivers/windows/ramshared/{driver,virtdisk,queue,control}.{c,h}` | ITEM-5 (RF-1/RF-2) | StorPort virtual miniport + control device + rings/DT-10 |
| `drivers/windows/ramshared/ramshared.{inf,vcxproj,sln}` | ITEM-5 (H4) | superfície de build WDK |
| `drivers/windows/tools/poolstress/*` | ITEM-8 (DT-11) | test driver VM-only |
| `scripts/windows/*.ps1` | ITEM-8..11 | harness KernelPage/Measure/Soak/Revoke/Build-Sign |

## Decisões pequenas (sem nova ADR)

- Golden SQE test serializa campo-a-campo (sem `unsafe`) para manter `#![forbid(unsafe_code)]` no winsvc.
- `NtCreatePagingFile` / `RtlGetVersion` no Windows: API bound falha-fechada com `PagefileError::Api` até FFI real no host Windows (allow-list + fallback já testáveis no Linux).
- `StorPortNotification(RequestComplete, …)` no esqueleto C usa placeholder de DeviceExtension onde o WDK exige o adapter extension real — a ser amarrado no 1º build EWDK (sem mudança de contrato).
- `service.rs` isola provision/teardown com traits injetáveis (`FreeVram`, `DiskControl`, `WipeVram`) para DT-20/DT-9 no Linux.

## Validação (números)

- testes Linux: **workspace PASS** — winsvc 23 pass; block +3 VramBackend; wsl2d +2 WinDrive; zero fail (ignored GPU/Vulkan inalterados)
- checkpatch: N/A (Windows C; DT-14 checklist WDK)
- clippy: limpo com `-D warnings` (após `cargo fmt`)
- **VM Hyper-V `win11-drill` (2026-07-09):** build **26200.8037**, test-signing **Yes**, PSD via `drilladmin`
  - `Get-WinDrivePreflight` exit **0**
  - `ps1-parse` **PARSE_OK** (todos `scripts/windows/*.ps1`)
  - `Invoke-KernelPageDrill` exit **3 INCONCLUSIVO** (esperado: sem pagefile-VRAM / sem `.sys` — DT-21 gate honesto)
  - `Measure-PagefileVram` n=3 idle: median_ms≈**91.7** (run-id `pfvram-20260709-141707`; pagefile counter −1 no guest — só `C:\pagefile.sys` 1408 MB alloc / ~340 MB used)
  - `Invoke-RevokeDrill` exit **2** sem serviço (script OK)
  - `Build-Sign-Install -SkipSign` **SKIP** (sem msbuild/WDK no guest)
  - Overall harness: **PASS_WITH_SKIPS fails=0** (`C:\Users\emedev\ramshared-drill\agent-vm-e2e-results.json`)
- benchmark: K (RNF-2) **ainda não** — falta pagefile-VRAM real vs disco

## Gaps

- **fechados nesta sessão:** ITEM-1 (verificado), ITEM-2, ITEM-3 (broker+winsvc pure), ITEM-4 (ABI+tests), ITEM-5 (código fonte driver), ITEM-6 (driver_link pure), ITEM-7 (ntpagefile/smoke pure + allow-list), ITEM-8..11 (scripts executáveis + smoke na VM)
- **env-bound (precisa hardware/civm/GPU/WDK):** MSBuild+SDV+InfVerif no guest ou host build; `Cuda::load` em `nvcuda.dll` (VM sem GPU/nvcuda); drills `qemu-ublk-*`; kernel-page drill com residência DT-21 **após** `.sys`+pagefile-VRAM; soak 72 h; attestation Partner Center (R9)
- **abertos:** amarrar DeviceExtension real no `StorPortNotification`; FFI `NtCreatePagingFile` no Windows; SCM `windows-service` main path; e2e NTFS format em VM; instalar EWDK/WDK na pipeline de build

## Rollback trigger

- Qualquer BugCheck em host não-VM → parar loads no host; só reexecutar ITEM-8 em VM.
- Regressão de teste/drill Linux após ITEM-1/2 → `git revert` do commit do ITEM (RNF-8).
- Double-complete de SRB / SDV defect sem waiver → não promover driver.
- Pagefile ativo + destroy de disco → proibido (DT-9); tratar como incidente B1.

## Traceability

| PRD | SPEC ITEM | Commit(s) |
| --- | --- | --- |
| RF-4 / RNF-8 | ITEM-1 | pré-existente (`loader_unix`/`loader_win`); verificado nesta IMPL |
| RF-3 / RNF-8 | ITEM-2 | `28a7960` |
| RF-5 / DT-7 | ITEM-3 (broker) | `ae9cc44` |
| RF-3/RF-5/RF-6 | ITEM-3/4/6/7 (winsvc) | `2145401` |
| RF-1 / RF-2 / RNF-4 | ITEM-5 | `f149541` |
| RF-7 / RNF-1/2/5/7 | ITEM-8..11 | `d2f87f5` |
| — | IMPL.md | `6d3fb4a` (+ este amend de SHAs se aplicável) |
