//! `broker_srv` — núcleo **puro** do broker (decisão/estado), testável sem threads/sockets/GPU.
//!
//! Mesma disciplina do árbitro: o [`BrokerCore`] recebe [`CoreEvent`]s e devolve [`Outbound`]s
//! que a camada de IO ([`spawn_broker`]) executa — enviar `Msg` à sessão, fechar sessão, pedir
//! ao worker para zerar uma slice (DT-17), ou logar (RF-B4).
//!
//! SPEC: docs/memory-broker/SPECv2.md ITEM-8. Cobre: sessões + Register/Psi/Ack/Status/Disconnect
//! (DT-18/20/22), reconciliação (DT-9/21), rebalanço com higiene (DT-17), DemoteAll e **lease
//! revogável (RF-B3/DT-19)**. A camada de IO ([`spawn_broker`]) roda o core sobre TCP (DT-2/24);
//! falta apenas a fiação no `run_nbd` do daemon (`--slices`/`--arbiter-listen`).

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ramshared_broker::arbiter::{Action, Arbiter, ArbiterConfig, TenantView};
use ramshared_broker::model::{PsiSample, Slice, SliceId, SliceState, TenantId, TransportKind};
use ramshared_broker::protocol::{
    Msg, NbdEndpoint, PROTO_VERSION, SliceIo, SwapEntry, TenantMem, TenantStatus, read_msg,
    write_msg,
};
use ramshared_broker::slices::SliceMap;

use crate::canary_probe::CANARY_BYTES;
use crate::conn::WMsg;
use crate::residency::DemoteReason;
use crate::telemetry::{
    ReconcileFlag, ReconcileInput, SliceIoCounters, TelemetryCore, VramGauge, reconcile,
    vram_outros,
};

/// Endpoints NBD que os agentes recebem no `SwapOn`, escolhidos pelo transporte (DT-25).
#[derive(Clone, Debug, Default)]
pub struct EndpointCfg {
    pub nbd_unix: Option<String>,
    pub nbd_tcp: Option<(String, u16)>,
}

/// Evento de entrada do core (produzido pela camada de IO). `sid` = id da conexão (sessão).
#[derive(Clone, Debug, PartialEq)]
pub enum CoreEvent {
    Msg(usize, Msg),
    Disconnected(usize),
    ZeroDone(SliceId, bool),
    Demote(String),
    Tick,
}

/// Ação de saída que a camada de IO executa.
#[derive(Clone, Debug, PartialEq)]
pub enum Outbound {
    ToSession(usize, Msg),
    CloseSession(usize),
    ZeroSlice {
        slice: SliceId,
        base: u64,
        len: u64,
    },
    Log(String),
    /// Amostra de telemetria reconciliada (RF-5); a camada de IO carimba `t`/`branch`/`commit`.
    Telemetry(TelemetryCore),
}

#[derive(Clone, Debug)]
struct TenantState {
    name: String,
    transport: TransportKind,
    present: bool,
    sid: Option<usize>,
    psi: PsiSample,
    reconciled: bool,
    /// Última telemetria de memória do tenant (RF-2); `None` se o agente não reporta cgroup/diskstats.
    mem: Option<TenantMem>,
    /// Swap ocupado (bytes) nas slices `Active` deste tenant, derivado no `Psi` (DT-10/F-v2-2).
    occupied_bytes: u64,
}

/// Núcleo do broker: dono único de `SliceMap` + `Arbiter` + tabela de sessões (sem locks; a
/// camada de IO roda isto em uma thread só). `BTreeMap` por `TenantId` dá iteração estável
/// (round-robin determinístico).
pub struct BrokerCore {
    slice_map: SliceMap,
    arbiter: Arbiter,
    endpoints: EndpointCfg,
    swap_prio: Option<i32>,
    tenants: BTreeMap<TenantId, TenantState>,
    sessions: HashMap<usize, TenantId>,    // conexão viva → tenant
    name_to_id: HashMap<String, TenantId>, // id estável por nome (DT-22)
    next_tenant: TenantId,
    pending_dest: HashMap<SliceId, TenantId>, // destino de um MoveSlice em voo (pós-zero)
    pending_lease: Option<(TenantId, u64)>,   // lease pedido, ainda não concedido (RF-B3)
    lease: Option<(u32, TenantId)>,           // lease ativo (id, holder); o id vem do árbitro
    last_rebalance: Option<Instant>,
    // Telemetria/reconciliação (SPECv2 broker-telemetry-reconciliation).
    slice_io: Arc<Vec<SliceIoCounters>>, // contadores por slice (data-plane escreve, RF-1/DT-1)
    vram: Arc<VramGauge>,                // gauge de VRAM publicado pelo worker (RF-3/DT-5)
    demotes_total: u64,                  // DEMOTEs do canário acumulados (RF-4)
    last_demote_reason: Option<String>,
    demotes_at_last_sample: u64, // base p/ o `demotes_delta` por tick
    recon_flag: ReconcileFlag,   // flag candidato (histerese DT-12)
    recon_count: u32,            // ticks consecutivos com o mesmo flag candidato
    tol_frac: f64,               // tolerância da reconciliação (DT-7)
    recon_streak: u32,           // ticks p/ confirmar o flag (DT-12)
    // R4: slices pós-`SwapOffDone` aguardando confirmação de zero (`ZeroDone`), com nº de ticks
    // sem confirmar. Se o `try_send(ZeroExport)` falhou (canal cheio), não vem `ZeroDone` → o tick
    // re-emite o zero (retry) até confirmar; escala a ERROR após N. Só slices AQUI já foram
    // swapped-off pelo tenant (seguro re-zerar); slices em Draining aguardando SwapOffDone NÃO entram.
    pending_zero: HashMap<SliceId, u32>,
}

/// Carência (em ticks) antes do 1º retry de zero — dá tempo ao `ZeroDone` em voo. Acima disso o
/// tick re-emite o `ZeroExport`; em [`ZERO_RETRY_ERROR`] ticks sem confirmar, loga ERROR (R4).
const ZERO_RETRY_GRACE: u32 = 1;
const ZERO_RETRY_ERROR: u32 = 5;

/// Extrai o inteiro final de um device (`/dev/nbd5` → 5), agnóstico ao prefixo (DT-21).
fn dev_to_slice(dev: &str) -> Option<SliceId> {
    let tail: String = dev.chars().rev().take_while(char::is_ascii_digit).collect();
    if tail.is_empty() {
        return None;
    }
    tail.chars().rev().collect::<String>().parse().ok()
}

impl BrokerCore {
    #[allow(clippy::too_many_arguments)] // construtor do core: config + Arcs de telemetria
    pub fn new(
        slice_map: SliceMap,
        arbiter_cfg: ArbiterConfig,
        endpoints: EndpointCfg,
        swap_prio: Option<i32>,
        slice_io: Arc<Vec<SliceIoCounters>>,
        vram: Arc<VramGauge>,
        tol_frac: f64,
        recon_streak: u32,
    ) -> Self {
        Self {
            slice_map,
            arbiter: Arbiter::new(arbiter_cfg),
            endpoints,
            swap_prio,
            tenants: BTreeMap::new(),
            sessions: HashMap::new(),
            name_to_id: HashMap::new(),
            next_tenant: 1,
            pending_dest: HashMap::new(),
            pending_lease: None,
            lease: None,
            last_rebalance: None,
            slice_io,
            vram,
            demotes_total: 0,
            last_demote_reason: None,
            demotes_at_last_sample: 0,
            recon_flag: ReconcileFlag::None,
            recon_count: 0,
            tol_frac,
            recon_streak,
            pending_zero: HashMap::new(),
        }
    }

    pub fn handle(&mut self, ev: CoreEvent, now: Instant) -> Vec<Outbound> {
        let mut out = Vec::new();
        match ev {
            CoreEvent::Msg(sid, msg) => self.on_msg(sid, msg, &mut out),
            CoreEvent::Disconnected(sid) => self.on_disconnect(sid, &mut out),
            CoreEvent::ZeroDone(slice, ok) => self.on_zero_done(slice, ok, &mut out),
            CoreEvent::Demote(reason) => self.on_demote(&reason, &mut out),
            CoreEvent::Tick => self.on_tick(now, &mut out),
        }
        out
    }

    fn endpoint_for(&self, transport: TransportKind) -> Option<NbdEndpoint> {
        match transport {
            TransportKind::NbdUnix => self
                .endpoints
                .nbd_unix
                .clone()
                .map(|path| NbdEndpoint::Unix { path }),
            TransportKind::NbdTcp => self
                .endpoints
                .nbd_tcp
                .clone()
                .map(|(host, port)| NbdEndpoint::Tcp { host, port }),
        }
    }

    /// Envia `msg` para a sessão viva do tenant (ou loga se ausente — DT-20).
    fn to_tenant(&self, tenant: TenantId, msg: Msg, out: &mut Vec<Outbound>) {
        match self.tenants.get(&tenant).and_then(|t| t.sid) {
            Some(sid) => out.push(Outbound::ToSession(sid, msg)),
            None => out.push(Outbound::Log(format!(
                "[ramsharedd] WARN tenant {tenant} ausente; msg descartada"
            ))),
        }
    }

    fn slices_of(&self, tenant: TenantId) -> Vec<SliceId> {
        self.slice_map
            .slices()
            .iter()
            .filter(|s| s.tenant == Some(tenant))
            .map(|s| s.id)
            .collect()
    }

    fn on_msg(&mut self, sid: usize, msg: Msg, out: &mut Vec<Outbound>) {
        match msg {
            Msg::Register {
                proto,
                tenant,
                transport,
            } => self.on_register(sid, proto, tenant, transport, out),
            Msg::Psi { sample, swaps, mem } => self.on_psi(sid, sample, swaps, mem, out),
            Msg::SwapOnDone { slice, ok, detail } => {
                if ok {
                    out.push(Outbound::Log(format!(
                        "[ramsharedd] swapon ok slice=s{slice}"
                    )));
                } else {
                    // mount falhou: nada foi escrito → devolve a slice direto (sem zero).
                    let _ = self
                        .slice_map
                        .drain(slice)
                        .and_then(|()| self.slice_map.release(slice));
                    self.pending_dest.remove(&slice);
                    out.push(Outbound::Log(format!(
                        "[ramsharedd] swapon FALHOU slice=s{slice} ({detail}); slice liberada"
                    )));
                }
            }
            Msg::SwapOffDone { slice, ok, detail } => {
                if ok {
                    // higiene (DT-17): zera antes de liberar. ZeroDone → release. O tenant já fez
                    // swapoff → seguro zerar; entra em pending_zero p/ o retry do tick (R4).
                    if let Some(s) = self.slice_map.get(slice) {
                        let (base, len) = (s.offset, s.len);
                        self.pending_zero.entry(slice).or_insert(0);
                        out.push(Outbound::ZeroSlice { slice, base, len });
                    }
                } else {
                    out.push(Outbound::Log(format!(
                        "[ramsharedd] swapoff FALHOU slice=s{slice} ({detail}); fica Draining"
                    )));
                }
            }
            Msg::Status => out.push(Outbound::ToSession(sid, self.status_reply())),
            Msg::LeaseRequest { bytes } => self.on_lease_request(sid, bytes, out),
            Msg::LeaseRelease { lease } => self.on_lease_release(lease, out),
            other => {
                // Mensagens broker→agente nunca chegam aqui; shape inesperado → fecha.
                out.push(Outbound::ToSession(
                    sid,
                    Msg::Error {
                        reason: format!("mensagem inesperada do agente: {other:?}"),
                    },
                ));
                out.push(Outbound::CloseSession(sid));
            }
        }
    }

    fn on_register(
        &mut self,
        sid: usize,
        proto: u32,
        name: String,
        transport: TransportKind,
        out: &mut Vec<Outbound>,
    ) {
        if proto != PROTO_VERSION {
            out.push(Outbound::ToSession(
                sid,
                Msg::Error {
                    reason: format!("proto {proto} != {PROTO_VERSION}"),
                },
            ));
            out.push(Outbound::CloseSession(sid));
            return;
        }
        let id = *self
            .name_to_id
            .entry(name.clone())
            .or_insert(self.next_tenant);
        if id == self.next_tenant {
            self.next_tenant += 1;
        }
        // DT-22: tenant já com sessão viva diferente → duplicado.
        if let Some(t) = self.tenants.get(&id)
            && t.present
            && t.sid.is_some()
            && t.sid != Some(sid)
        {
            out.push(Outbound::ToSession(
                sid,
                Msg::Error {
                    reason: "tenant_duplicado".into(),
                },
            ));
            out.push(Outbound::CloseSession(sid));
            return;
        }
        self.tenants.insert(
            id,
            TenantState {
                name: name.clone(),
                transport,
                present: true,
                sid: Some(sid),
                psi: self
                    .tenants
                    .get(&id)
                    .map_or(PsiSample::default(), |t| t.psi),
                reconciled: false,
                mem: None,
                occupied_bytes: 0,
            },
        );
        self.sessions.insert(sid, id);
        out.push(Outbound::Log(format!(
            "[ramsharedd] tenant registrado name={name} id={id} transport={transport:?}"
        )));
        out.push(Outbound::ToSession(sid, Msg::Registered { tenant_id: id }));
    }

    fn on_psi(
        &mut self,
        sid: usize,
        sample: PsiSample,
        swaps: Vec<SwapEntry>,
        mem: Option<TenantMem>,
        out: &mut Vec<Outbound>,
    ) {
        let Some(&id) = self.sessions.get(&sid) else {
            out.push(Outbound::ToSession(
                sid,
                Msg::Error {
                    reason: "psi antes de register".into(),
                },
            ));
            out.push(Outbound::CloseSession(sid));
            return;
        };
        if let Some(t) = self.tenants.get_mut(&id) {
            t.psi = sample;
            t.mem = mem;
            // Reconciliação (DT-9/21): no 1º Psi, re-adota slices que o agente já tem montadas.
            if !t.reconciled {
                t.reconciled = true;
                for e in &swaps {
                    if let Some(slice) = dev_to_slice(&e.dev)
                        && self
                            .slice_map
                            .get(slice)
                            .is_some_and(|s| s.state == SliceState::Free)
                        && self.slice_map.assign(slice, id).is_ok()
                    {
                        out.push(Outbound::Log(format!(
                            "[ramsharedd] reconciliado slice=s{slice} -> tenant {id} (já montada)"
                        )));
                    }
                }
            }
        }
        // occupied (DT-10/F-v2-2): Σ used_kb*1024 das swaps que casam slices Active deste tenant.
        let occupied: u64 = swaps
            .iter()
            .filter_map(|e| dev_to_slice(&e.dev).map(|sl| (sl, e.used_kb)))
            .filter(|(sl, _)| {
                self.slice_map
                    .get(*sl)
                    .is_some_and(|s| s.state == SliceState::Active && s.tenant == Some(id))
            })
            .map(|(_, kb)| kb.saturating_mul(1024))
            .sum();
        if let Some(t) = self.tenants.get_mut(&id) {
            t.occupied_bytes = occupied;
        }
        out.push(Outbound::ToSession(sid, Msg::Ack)); // DT-18: heartbeat
    }

    fn on_lease_request(&mut self, sid: usize, bytes: u64, out: &mut Vec<Outbound>) {
        let Some(&holder) = self.sessions.get(&sid) else {
            out.push(Outbound::ToSession(
                sid,
                Msg::Error {
                    reason: "lease antes de register".into(),
                },
            ));
            out.push(Outbound::CloseSession(sid));
            return;
        };
        // P1: no máximo 1 lease pendente/ativo (DT-19).
        if self.pending_lease.is_some() || self.lease.is_some() {
            out.push(Outbound::ToSession(
                sid,
                Msg::LeaseDenied {
                    reason: "lease_em_andamento".into(),
                },
            ));
            return;
        }
        if bytes > self.slice_map.total_bytes() {
            out.push(Outbound::ToSession(
                sid,
                Msg::LeaseDenied {
                    reason: "acima_da_capacidade".into(),
                },
            ));
            return;
        }
        self.pending_lease = Some((holder, bytes));
        out.push(Outbound::Log(format!(
            "[ramsharedd] lease pedido holder={holder} bytes={bytes} (grant no próximo tick)"
        )));
    }

    fn on_lease_release(&mut self, lease_id: u32, out: &mut Vec<Outbound>) {
        if self.lease.map(|(id, _)| id) != Some(lease_id) {
            return; // lease desconhecido; ignora
        }
        self.lease = None;
        let leased: Vec<SliceId> = self
            .slice_map
            .slices()
            .iter()
            .filter(|s| s.state == SliceState::Leased)
            .map(|s| s.id)
            .collect();
        for slice in leased {
            let _ = self.slice_map.unlease(slice); // Leased → Free (round-robin re-arrenda)
        }
        out.push(Outbound::Log(format!(
            "[ramsharedd] lease {lease_id} liberado; slices devolvidas ao tier de swap"
        )));
    }

    fn on_disconnect(&mut self, sid: usize, out: &mut Vec<Outbound>) {
        if let Some(id) = self.sessions.remove(&sid) {
            if let Some(t) = self.tenants.get_mut(&id) {
                t.present = false;
                t.sid = None;
            }
            out.push(Outbound::Log(format!(
                "[ramsharedd] tenant {id} desconectou; slices congeladas (DT-20)"
            )));
            // Holder/requester do lease caiu → release/cancela automático (DT-19).
            if let Some((lid, h)) = self.lease
                && h == id
            {
                self.on_lease_release(lid, out);
            }
            if self.pending_lease.map(|(h, _)| h) == Some(id) {
                self.pending_lease = None;
            }
        }
    }

    fn on_zero_done(&mut self, slice: SliceId, ok: bool, out: &mut Vec<Outbound>) {
        if !ok {
            // retry do zero no próximo evento de movimento; por ora loga e fica Draining (R4).
            out.push(Outbound::Log(format!(
                "[ramsharedd] zero FALHOU slice=s{slice}; fica Draining (retry)"
            )));
            if let Some(s) = self.slice_map.get(slice) {
                out.push(Outbound::ZeroSlice {
                    slice,
                    base: s.offset,
                    len: s.len,
                });
            }
            return;
        }
        self.pending_zero.remove(&slice); // zero confirmado → encerra o retry (R4)
        if self.slice_map.release(slice).is_err() {
            return; // não estava Draining; ignora
        }
        // Movimento em voo: a slice limpa vai para o destino (SwapOn).
        if let Some(dest) = self.pending_dest.remove(&slice)
            && self.slice_map.assign(slice, dest).is_ok()
        {
            self.emit_swapon(slice, dest, out);
        }
    }

    fn on_demote(&mut self, reason: &str, out: &mut Vec<Outbound>) {
        self.demotes_total += 1;
        self.last_demote_reason = Some(reason.to_string());
        out.push(Outbound::Log(format!(
            "[ramsharedd] DemoteAll reason={reason}"
        )));
        let present: Vec<TenantId> = self
            .tenants
            .iter()
            .filter(|(_, t)| t.present)
            .map(|(id, _)| *id)
            .collect();
        for id in present {
            self.to_tenant(id, Msg::DemoteAll, out);
        }
    }

    fn emit_swapon(&self, slice: SliceId, tenant: TenantId, out: &mut Vec<Outbound>) {
        let transport = match self.tenants.get(&tenant) {
            Some(t) => t.transport,
            None => return,
        };
        let Some(endpoint) = self.endpoint_for(transport) else {
            out.push(Outbound::Log(format!(
                "[ramsharedd] ERRO transporte indisponível p/ tenant {tenant}"
            )));
            return;
        };
        self.to_tenant(
            tenant,
            Msg::SwapOn {
                slice,
                export: format!("s{slice}"),
                endpoint,
                swap_prio: self.swap_prio,
            },
            out,
        );
    }

    fn status_reply(&self) -> Msg {
        let tenants = self
            .tenants
            .iter()
            .map(|(id, t)| TenantStatus {
                id: *id,
                name: t.name.clone(),
                psi: t.psi,
                slices: self.slices_of(*id),
                present: t.present,
                bytes_served: self
                    .slices_of(*id)
                    .iter()
                    .filter_map(|s| self.slice_io.get(*s as usize))
                    .map(|c| c.bytes_served.load(Ordering::Relaxed))
                    .sum(),
            })
            .collect();
        let slice_io = self
            .slice_map
            .slices()
            .iter()
            .filter_map(|s| {
                self.slice_io.get(s.id as usize).map(|c| SliceIo {
                    id: s.id,
                    bytes_served: c.bytes_served.load(Ordering::Relaxed),
                    io_count: c.io_count.load(Ordering::Relaxed),
                })
            })
            .collect();
        Msg::StatusReply {
            tenants,
            slices: self.slice_map.slices().to_vec(),
            slice_io,
            last_rebalance_secs: None,
        }
    }

    fn on_tick(&mut self, now: Instant, out: &mut Vec<Outbound>) {
        // DT-20: só tenants presentes; só slices Free ou de dono presente.
        let present: Vec<TenantView> = self
            .tenants
            .iter()
            .filter(|(_, t)| t.present)
            .map(|(id, t)| TenantView {
                id: *id,
                psi: t.psi,
                slices: self.slices_of(*id).len() as u16,
            })
            .collect();
        let visible: Vec<Slice> = self
            .slice_map
            .slices()
            .iter()
            .filter(|s| {
                s.tenant.is_none()
                    || s.tenant
                        .is_some_and(|t| self.tenants.get(&t).is_some_and(|ts| ts.present))
            })
            .cloned()
            .collect();

        let actions = self
            .arbiter
            .tick(now, &present, &visible, self.pending_lease);
        for action in actions {
            match action {
                Action::AssignFree { slice, to } => {
                    if self.slice_map.assign(slice, to).is_ok() {
                        out.push(Outbound::Log(format!(
                            "[ramsharedd] arbiter assign slice=s{slice} to={to}"
                        )));
                        self.emit_swapon(slice, to, out);
                    }
                }
                Action::MoveSlice { slice, from, to } | Action::RevertMove { slice, from, to } => {
                    if self.slice_map.drain(slice).is_ok() {
                        self.pending_dest.insert(slice, to);
                        self.last_rebalance = Some(now);
                        let pf = present
                            .iter()
                            .find(|t| t.id == from)
                            .map_or(0.0, |t| t.psi.avg10);
                        let pt = present
                            .iter()
                            .find(|t| t.id == to)
                            .map_or(0.0, |t| t.psi.avg10);
                        out.push(Outbound::Log(format!(
                            "[ramsharedd] arbiter move slice=s{slice} from={from}(psi10={pf:.1}) to={to}(psi10={pt:.1})"
                        )));
                        self.to_tenant(from, Msg::SwapOff { slice }, out);
                    }
                }
                Action::RevokeForLease { slice, from, .. } => {
                    // Revoga p/ o lease: drena+SwapOff; no ZeroDone vira Free (sem pending_dest),
                    // e o árbitro o conta p/ o lease no próximo tick (DT-8/R2).
                    if self.slice_map.drain(slice).is_ok() {
                        out.push(Outbound::Log(format!(
                            "[ramsharedd] revoke-for-lease slice=s{slice} from={from}"
                        )));
                        self.to_tenant(from, Msg::SwapOff { slice }, out);
                    }
                }
                Action::GrantLease {
                    lease,
                    holder,
                    slices,
                } => {
                    let mut granted = 0u64;
                    for s in &slices {
                        if let Some(len) = self.slice_map.get(*s).map(|sl| sl.len)
                            && self.slice_map.lease(*s).is_ok()
                        {
                            granted += len; // Free → Leased
                        }
                    }
                    self.lease = Some((lease, holder));
                    self.pending_lease = None;
                    out.push(Outbound::Log(format!(
                        "[ramsharedd] lease {lease} concedido holder={holder} slices={slices:?}"
                    )));
                    self.to_tenant(
                        holder,
                        Msg::LeaseGranted {
                            lease,
                            bytes: granted,
                        },
                        out,
                    );
                }
            }
        }

        // R4: re-tenta o zero de slices presas (se o `try_send(ZeroExport)` falhou, não vem
        // `ZeroDone`). Carência de 1 tick p/ o zero em voo; acima disso re-emite; ERROR após N
        // ticks sem confirmar. Só toca slices em `pending_zero` (já swapped-off; seguro re-zerar).
        let stuck: Vec<SliceId> = self.pending_zero.keys().copied().collect();
        for slice in stuck {
            let count = match self.pending_zero.get_mut(&slice) {
                Some(n) => {
                    *n += 1;
                    *n
                }
                None => continue,
            };
            if count == ZERO_RETRY_ERROR {
                out.push(Outbound::Log(format!(
                    "[ramsharedd] ERRO: zero de s{slice} sem confirmar em {ZERO_RETRY_ERROR} ticks (slice presa em Draining, R4); re-tentando"
                )));
            }
            if count > ZERO_RETRY_GRACE
                && let Some(s) = self.slice_map.get(slice)
            {
                let (base, len) = (s.offset, s.len);
                out.push(Outbound::ZeroSlice { slice, base, len });
            }
        }

        self.emit_telemetry(out);
    }

    /// Reconciliação por tick (RF-4/RF-5): invariante de **ocupação** + histerese (DT-12) → emite
    /// `Outbound::Telemetry`. Observador: não toca o árbitro/SliceMap.
    fn emit_telemetry(&mut self, out: &mut Vec<Outbound>) {
        let alloc_active: u64 = self
            .slice_map
            .slices()
            .iter()
            .filter(|s| matches!(s.state, SliceState::Active | SliceState::Draining))
            .map(|s| s.len)
            .sum();
        let occupied: u64 = self.tenants.values().map(|t| t.occupied_bytes).sum();
        // Σ diskstats (cumulativo em bytes) dos tenants que reportam `mem` (RF-2); `None` se ninguém
        // reporta. O consumidor deriva a taxa pela diferença entre amostras (campo `t` da linha).
        let page_io: Option<u64> = self.tenants.values().any(|t| t.mem.is_some()).then(|| {
            self.tenants
                .values()
                .filter_map(|t| t.mem.as_ref())
                .map(|m| m.diskstats_io)
                .sum()
        });
        let free = self.vram.free.load(Ordering::Relaxed);
        let total = self.vram.total.load(Ordering::Relaxed);
        let has_vram = total > 0; // F-v2-6: sentinela RAM (sem GPU) → vram_* = None
        let vram_alloc_daemon = alloc_active + CANARY_BYTES as u64;
        let vram_total_used = has_vram.then(|| total.saturating_sub(free));
        let vram_others = vram_total_used.map(|u| vram_outros(u, vram_alloc_daemon));
        let demotes_delta = self.demotes_total - self.demotes_at_last_sample;
        self.demotes_at_last_sample = self.demotes_total;
        let stuck_draining = self.pending_zero.values().any(|n| *n >= ZERO_RETRY_ERROR);
        let any_present = self.tenants.values().any(|t| t.present);
        let inp = ReconcileInput {
            alloc_active_bytes: alloc_active,
            occupied_swap_bytes: occupied,
            stuck_draining,
            demotes_delta,
            any_source_missing: !has_vram || !any_present,
        };
        let (delta, candidate) = reconcile(&inp, self.tol_frac);
        // Histerese (DT-12) para flags SUSTENTADOS (Unaccounted/StuckSlice/Partial): confirma após
        // `recon_streak` ticks consecutivos iguais. `Eviction` é EVENTO do canário (DT-6;
        // `demotes_delta` é per-tick, dura 1 tick) → confirma IMEDIATO; senão a histerese engoliria
        // uma evicção transitória (1-2 DEMOTEs) e o sinal de eviction nunca apareceria (bug C1).
        if candidate == self.recon_flag {
            self.recon_count = self.recon_count.saturating_add(1);
        } else {
            self.recon_flag = candidate;
            self.recon_count = 1;
        }
        let confirmed = match candidate {
            ReconcileFlag::Eviction => ReconcileFlag::Eviction, // evento: sem histerese
            ReconcileFlag::None => ReconcileFlag::None,
            sustained if self.recon_count >= self.recon_streak => sustained,
            _ => ReconcileFlag::None,
        };
        if confirmed != ReconcileFlag::None {
            out.push(Outbound::Log(format!(
                "[ramsharedd] reconcile flag={confirmed:?} delta={delta:.3} reason={:?}",
                self.last_demote_reason
            )));
        }
        out.push(Outbound::Telemetry(TelemetryCore {
            tenant: None,
            slice: None,
            swap_used: occupied,
            alloc_active,
            page_io_s: page_io,
            vram_alloc_daemon,
            vram_total_used,
            vram_outros: vram_others,
            canario_demotes: self.demotes_total,
            demote_reason: self.last_demote_reason.clone(),
            reconcile_delta: delta,
            flag: confirmed,
        }));
    }
}

// ===================== Camada de IO (DT-2/DT-24) =====================
//
// O `BrokerCore` é puro; `spawn_broker` é a fina casca de threads que o roda: acceptor TCP,
// reader/writer por sessão (writer = canal bounded 64, DT-24), o loop do core (`recv_timeout`
// = tick) e forwarders de DEMOTE e de zero-done. O core nunca faz IO de socket.

/// Config do broker (DT-2: in-process no daemon; RNF-2: `listen` já validado não-unspecified).
pub struct BrokerConfig {
    pub listen: SocketAddr,
    pub endpoints: EndpointCfg,
    pub swap_prio: Option<i32>,
    pub arbiter: ArbiterConfig,
    pub tick: Duration,
    /// Telemetria (SPECv2): contadores por slice + gauge de VRAM (compartilhados com o worker).
    pub slice_io: Arc<Vec<SliceIoCounters>>,
    pub vram: Arc<VramGauge>,
    pub tol_frac: f64,
    pub recon_streak: u32,
    /// Destino do JSONL de telemetria (RF-5); `None` = telemetria silenciosa.
    pub telemetry_jsonl: Option<PathBuf>,
}

/// Evento interno do IO (multiplexa registro de sessão e eventos do core).
enum IoEvent {
    NewSession(usize, SyncSender<Msg>),
    Core(CoreEvent),
}

/// Sobe o broker: acceptor + sessões + core single-thread. Devolve o handle do core e o
/// `SocketAddr` ligado (útil com porta 0 em testes). `shutdown` dispara `DemoteAll` + saída.
pub fn spawn_broker(
    cfg: BrokerConfig,
    slice_map: SliceMap,
    demote_rx: Receiver<DemoteReason>,
    jobs: SyncSender<WMsg>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<(JoinHandle<()>, SocketAddr)> {
    let listener = TcpListener::bind(cfg.listen)?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let (io_tx, io_rx) = mpsc::channel::<IoEvent>();

    // Acceptor.
    {
        let io_tx = io_tx.clone();
        let shutdown = Arc::clone(&shutdown);
        thread::spawn(move || acceptor_loop(&listener, &io_tx, &shutdown));
    }
    // Forwarder de DEMOTE (canário/residência) → CoreEvent::Demote.
    {
        let io_tx = io_tx.clone();
        thread::spawn(move || {
            for reason in demote_rx.iter() {
                if io_tx
                    .send(IoEvent::Core(CoreEvent::Demote(format!("{reason:?}"))))
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    // Core (thread única dona do BrokerCore). Mantém um `io_tx` p/ os forwarders de zero-done.
    let core = BrokerCore::new(
        slice_map,
        cfg.arbiter,
        cfg.endpoints,
        cfg.swap_prio,
        cfg.slice_io,
        cfg.vram,
        cfg.tol_frac,
        cfg.recon_streak,
    );
    let tick = cfg.tick;
    let sink = cfg.telemetry_jsonl.and_then(TelemetrySink::open);
    let handle =
        thread::spawn(move || core_loop(core, &io_rx, &io_tx, &jobs, tick, &shutdown, sink));
    Ok((handle, addr))
}

fn acceptor_loop(listener: &TcpListener, io_tx: &Sender<IoEvent>, shutdown: &AtomicBool) {
    let mut next_sid = 0usize;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nodelay(true);
                let Ok(wsock) = stream.try_clone() else {
                    continue;
                };
                let sid = next_sid;
                next_sid += 1;
                let (wtx, wrx) = mpsc::sync_channel::<Msg>(64); // DT-24: bounded
                thread::spawn(move || session_writer(wsock, &wrx));
                if io_tx.send(IoEvent::NewSession(sid, wtx)).is_err() {
                    break; // core encerrou
                }
                let io2 = io_tx.clone();
                thread::spawn(move || session_reader(stream, sid, &io2));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn session_writer(mut sock: TcpStream, rx: &Receiver<Msg>) {
    for msg in rx.iter() {
        if write_msg(&mut sock, &msg).is_err() {
            break;
        }
    }
    // canal fechado (CloseSession/backpressure) ou erro → fecha o socket (o reader vê EOF).
    let _ = sock.shutdown(Shutdown::Both);
}

fn session_reader(sock: TcpStream, sid: usize, io_tx: &Sender<IoEvent>) {
    let mut r = BufReader::new(sock);
    while let Ok(Some(m)) = read_msg(&mut r) {
        if io_tx.send(IoEvent::Core(CoreEvent::Msg(sid, m))).is_err() {
            return;
        }
    }
    let _ = io_tx.send(IoEvent::Core(CoreEvent::Disconnected(sid)));
}

#[allow(clippy::too_many_arguments)] // casca de IO do core: canais + tick + shutdown + sink
fn core_loop(
    mut core: BrokerCore,
    io_rx: &Receiver<IoEvent>,
    io_tx: &Sender<IoEvent>,
    jobs: &SyncSender<WMsg>,
    tick: Duration,
    shutdown: &AtomicBool,
    mut sink: Option<TelemetrySink>,
) {
    let mut sessions: HashMap<usize, SyncSender<Msg>> = HashMap::new();
    // Deadline de wall-clock do próximo Tick. CRÍTICO: o Tick do árbitro NÃO pode ser starvado
    // pelas mensagens. `recv_timeout(tick)` puro nunca expira sob o fluxo normal de `Psi`
    // (~1/s por tenant) → o árbitro nunca rodaria `AssignFree`/rebalanço. Aqui o wait encolhe
    // conforme as mensagens chegam, e o Tick dispara quando o deadline passa, qualquer que seja
    // a taxa de mensagens. (Bug pego no e2e cross-host civm; o drill qemu passava por sorte de
    // timing.)
    let mut next_tick = Instant::now() + tick;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            let outs = core.handle(CoreEvent::Demote("shutdown".into()), Instant::now());
            dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            break;
        }
        let wait = next_tick.saturating_duration_since(Instant::now());
        match io_rx.recv_timeout(wait) {
            Ok(IoEvent::NewSession(sid, wtx)) => {
                sessions.insert(sid, wtx);
            }
            Ok(IoEvent::Core(ev)) => {
                let outs = core.handle(ev, Instant::now());
                dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_tick {
            let outs = core.handle(CoreEvent::Tick, Instant::now());
            dispatch(outs, &mut sessions, jobs, io_tx, &mut sink);
            next_tick = Instant::now() + tick;
        }
    }
}

fn dispatch(
    outs: Vec<Outbound>,
    sessions: &mut HashMap<usize, SyncSender<Msg>>,
    jobs: &SyncSender<WMsg>,
    io_tx: &Sender<IoEvent>,
    sink: &mut Option<TelemetrySink>,
) {
    for o in outs {
        match o {
            Outbound::ToSession(sid, msg) => {
                // DT-24: try_send; canal cheio/morto → derruba a sessão (sem bloquear o core).
                let dead = match sessions.get(&sid) {
                    Some(wtx) => wtx.try_send(msg).is_err(),
                    None => false,
                };
                if dead {
                    sessions.remove(&sid);
                }
            }
            Outbound::CloseSession(sid) => {
                sessions.remove(&sid);
            }
            Outbound::ZeroSlice { slice, base, len } => {
                let (dtx, drx) = mpsc::channel::<bool>();
                if jobs
                    .try_send(WMsg::ZeroExport {
                        base,
                        len,
                        done: dtx,
                    })
                    .is_ok()
                {
                    let io2 = io_tx.clone();
                    thread::spawn(move || {
                        let ok = drx.recv().unwrap_or(false);
                        let _ = io2.send(IoEvent::Core(CoreEvent::ZeroDone(slice, ok)));
                    });
                } else {
                    eprintln!("[ramsharedd] WARN canal jobs cheio; zero de s{slice} adiado (R4)");
                }
            }
            Outbound::Log(s) => eprintln!("{s}"),
            Outbound::Telemetry(core) => {
                if let Some(s) = sink.as_mut() {
                    s.emit(&core);
                }
            }
        }
    }
}

/// Sink de telemetria JSONL (RF-5/DT-8): carimba `t`/`branch`/`commit` no `TelemetryCore` do core e
/// faz append de 1 objeto JSON por linha. `branch`/`commit` vêm de env
/// (`RAMSHARED_BUILD_BRANCH`/`RAMSHARED_BUILD_COMMIT`; `None` se ausentes — F-v2-4).
struct TelemetrySink {
    file: File,
    branch: Option<String>,
    commit: Option<String>,
}

impl TelemetrySink {
    fn open(path: PathBuf) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| eprintln!("[ramsharedd] WARN telemetria off: {path:?}: {e}"))
            .ok()?;
        Some(Self {
            file,
            branch: std::env::var("RAMSHARED_BUILD_BRANCH").ok(),
            commit: std::env::var("RAMSHARED_BUILD_COMMIT").ok(),
        })
    }

    fn emit(&mut self, core: &TelemetryCore) {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sample = crate::telemetry::TelemetrySample {
            t,
            branch: self.branch.clone(),
            commit: self.commit.clone(),
            core: core.clone(),
        };
        if let Ok(mut line) = serde_json::to_string(&sample) {
            line.push('\n');
            let _ = self.file.write_all(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use ramshared_broker::model::SliceState;

    fn core(k: u16) -> BrokerCore {
        core_streak(k, 1)
    }

    /// Como `core`, mas com `recon_streak` configurável (testa a histerese real de produção).
    fn core_streak(k: u16, recon_streak: u32) -> BrokerCore {
        let cfg = ArbiterConfig {
            streak: 1, // move já no 1º tick acima do delta (testes)
            ..ArbiterConfig::default()
        };
        BrokerCore::new(
            SliceMap::new(k, 64 * 1024 * 1024),
            cfg,
            EndpointCfg {
                nbd_unix: Some("/run/x.sock".into()),
                nbd_tcp: None,
            },
            None,
            Arc::new((0..k).map(|_| SliceIoCounters::default()).collect()),
            Arc::new(VramGauge::default()),
            0.10,
            recon_streak,
        )
    }

    fn reg(c: &mut BrokerCore, sid: usize, name: &str) -> Vec<Outbound> {
        c.handle(
            CoreEvent::Msg(
                sid,
                Msg::Register {
                    proto: PROTO_VERSION,
                    tenant: name.into(),
                    transport: TransportKind::NbdUnix,
                },
            ),
            Instant::now(),
        )
    }

    fn psi(c: &mut BrokerCore, sid: usize, avg10: f32) -> Vec<Outbound> {
        c.handle(
            CoreEvent::Msg(
                sid,
                Msg::Psi {
                    sample: PsiSample {
                        avg10,
                        avg60: avg10,
                        stall_us: 0,
                    },
                    swaps: vec![],
                    mem: None,
                },
            ),
            Instant::now(),
        )
    }

    #[test]
    fn register_assigns_stable_id_and_acks_psi() {
        let mut c = core(2);
        let o = reg(&mut c, 10, "wsl2");
        assert!(o.contains(&Outbound::ToSession(10, Msg::Registered { tenant_id: 1 })));
        let o = psi(&mut c, 10, 0.0);
        assert!(o.contains(&Outbound::ToSession(10, Msg::Ack)));
    }

    #[test]
    fn duplicate_register_is_rejected() {
        let mut c = core(2);
        reg(&mut c, 10, "wsl2");
        let o = reg(&mut c, 11, "wsl2"); // mesmo nome, outra conexão viva
        assert!(o.iter().any(|x| matches!(x, Outbound::CloseSession(11))));
        assert!(o.iter().any(
            |x| matches!(x, Outbound::ToSession(11, Msg::Error { reason }) if reason == "tenant_duplicado")
        ));
    }

    #[test]
    fn proto_mismatch_rejected() {
        let mut c = core(1);
        let o = c.handle(
            CoreEvent::Msg(
                10,
                Msg::Register {
                    proto: 999,
                    tenant: "x".into(),
                    transport: TransportKind::NbdUnix,
                },
            ),
            Instant::now(),
        );
        assert!(o.iter().any(|x| matches!(x, Outbound::CloseSession(10))));
    }

    #[test]
    fn psi_before_register_closes() {
        let mut c = core(1);
        let o = psi(&mut c, 7, 0.0);
        assert!(o.iter().any(|x| matches!(x, Outbound::CloseSession(7))));
    }

    #[test]
    fn tick_round_robin_assigns_free_slices() {
        let mut c = core(2);
        reg(&mut c, 10, "a");
        reg(&mut c, 20, "b");
        psi(&mut c, 10, 0.0);
        psi(&mut c, 20, 0.0);
        let o = c.handle(CoreEvent::Tick, Instant::now());
        // 2 slices Free → SwapOn p/ cada tenant (round-robin: s0→a(1), s1→b(2))
        let swapons: Vec<_> = o
            .iter()
            .filter(|x| matches!(x, Outbound::ToSession(_, Msg::SwapOn { .. })))
            .collect();
        assert_eq!(swapons.len(), 2);
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Active);
        assert_eq!(c.slice_map.get(1).unwrap().state, SliceState::Active);
    }

    #[test]
    fn swapoff_done_triggers_zero_then_release() {
        let mut c = core(1);
        reg(&mut c, 10, "a");
        psi(&mut c, 10, 0.0);
        c.handle(CoreEvent::Tick, Instant::now()); // assign s0 → a (Active)
        c.slice_map.drain(0).unwrap(); // simula início de move (Active→Draining)
        let o = c.handle(
            CoreEvent::Msg(
                10,
                Msg::SwapOffDone {
                    slice: 0,
                    ok: true,
                    detail: String::new(),
                },
            ),
            Instant::now(),
        );
        assert!(o.iter().any(|x| matches!(
            x,
            Outbound::ZeroSlice { slice: 0, base: 0, len } if *len == 64 * 1024 * 1024
        )));
        // ZeroDone → release → Free
        c.handle(CoreEvent::ZeroDone(0, true), Instant::now());
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Free);
    }

    #[test]
    fn stuck_draining_zero_is_retried_on_tick() {
        // R4: se o zero não confirma (canal cheio → sem ZeroDone), o tick re-emite o ZeroExport
        // após a carência e escala a ERROR; ao confirmar, para de re-tentar.
        let mut c = core(1);
        reg(&mut c, 10, "a");
        psi(&mut c, 10, 0.0);
        c.handle(CoreEvent::Tick, Instant::now()); // assign s0 (Active)
        c.slice_map.drain(0).unwrap(); // Draining
        c.handle(
            CoreEvent::Msg(
                10,
                Msg::SwapOffDone {
                    slice: 0,
                    ok: true,
                    detail: String::new(),
                },
            ),
            Instant::now(),
        );
        // SEM ZeroDone (simula canal cheio). Tick 1 = carência → não re-emite.
        let t1 = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            !t1.iter()
                .any(|x| matches!(x, Outbound::ZeroSlice { slice: 0, .. })),
            "carência: sem retry no 1º tick"
        );
        // Tick 2 → re-emite o zero.
        let t2 = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            t2.iter()
                .any(|x| matches!(x, Outbound::ZeroSlice { slice: 0, .. })),
            "retry do zero no 2º tick"
        );
        // Mais ticks → escala a ERROR (R4).
        let mut saw_error = false;
        for _ in 0..ZERO_RETRY_ERROR {
            let o = c.handle(CoreEvent::Tick, Instant::now());
            if o.iter()
                .any(|x| matches!(x, Outbound::Log(s) if s.contains("R4")))
            {
                saw_error = true;
            }
        }
        assert!(
            saw_error,
            "deve escalar a ERROR (R4) após N ticks sem confirmar"
        );
        // Zero confirma → Free + para de re-tentar.
        c.handle(CoreEvent::ZeroDone(0, true), Instant::now());
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Free);
        let after = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            !after
                .iter()
                .any(|x| matches!(x, Outbound::ZeroSlice { slice: 0, .. })),
            "pós-confirmação: sem mais retry"
        );
    }

    #[test]
    fn move_drains_donor_then_swapon_dest_after_zero() {
        let mut c = core(2);
        reg(&mut c, 10, "donor");
        reg(&mut c, 20, "recv");
        psi(&mut c, 10, 0.0);
        psi(&mut c, 20, 0.0);
        c.handle(CoreEvent::Tick, Instant::now()); // assign s0→donor(1), s1→recv(2)
        // desequilíbrio: recv (2) muito pressionado, donor (1) idle, sem slices Free → move
        psi(&mut c, 10, 0.0);
        psi(&mut c, 20, 50.0);
        let o = c.handle(CoreEvent::Tick, Instant::now());
        // move de uma slice do donor(1) p/ recv(2): SwapOff p/ a sessão do donor (10)
        assert!(
            o.iter()
                .any(|x| matches!(x, Outbound::ToSession(10, Msg::SwapOff { .. })))
        );
        let moved = c
            .slice_map
            .slices()
            .iter()
            .find(|s| s.state == SliceState::Draining)
            .map(|s| s.id)
            .expect("uma slice drenando");
        // SwapOffDone → zero → ZeroDone → SwapOn p/ recv (sessão 20)
        c.handle(
            CoreEvent::Msg(
                10,
                Msg::SwapOffDone {
                    slice: moved,
                    ok: true,
                    detail: String::new(),
                },
            ),
            Instant::now(),
        );
        let o = c.handle(CoreEvent::ZeroDone(moved, true), Instant::now());
        assert!(o.iter().any(|x| matches!(
            x,
            Outbound::ToSession(20, Msg::SwapOn { slice, .. }) if *slice == moved
        )));
        assert_eq!(c.slice_map.get(moved).unwrap().tenant, Some(2));
    }

    #[test]
    fn disconnect_marks_absent_and_freezes() {
        let mut c = core(1);
        reg(&mut c, 10, "a");
        psi(&mut c, 10, 0.0);
        c.handle(CoreEvent::Tick, Instant::now()); // s0 → a (Active)
        c.handle(CoreEvent::Disconnected(10), Instant::now());
        // slice continua Active (congelada), dono ausente
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Active);
        // tick não mexe na slice do ausente (não vira Free nem é reatribuída)
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            !o.iter()
                .any(|x| matches!(x, Outbound::ToSession(_, Msg::SwapOn { .. })))
        );
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Active);
    }

    #[test]
    fn demote_broadcasts_to_present() {
        let mut c = core(1);
        reg(&mut c, 10, "a");
        let o = c.handle(CoreEvent::Demote("latency".into()), Instant::now());
        assert!(o.contains(&Outbound::ToSession(10, Msg::DemoteAll)));
    }

    #[test]
    fn status_reply_lists_tenants_and_slices() {
        let mut c = core(2);
        reg(&mut c, 10, "a");
        let o = c.handle(CoreEvent::Msg(10, Msg::Status), Instant::now());
        assert!(o.iter().any(|x| matches!(
            x,
            Outbound::ToSession(10, Msg::StatusReply { tenants, slices, .. })
                if tenants.len() == 1 && slices.len() == 2
        )));
    }

    #[test]
    fn status_reply_includes_slice_io() {
        // RF-1: o StatusReply expõe os contadores por slice (lidos do Arc compartilhado).
        let c = core(2);
        c.slice_io[0].bytes_served.store(4096, Ordering::Relaxed);
        c.slice_io[0].io_count.store(1, Ordering::Relaxed);
        match c.status_reply() {
            Msg::StatusReply { slice_io, .. } => {
                assert_eq!(slice_io.len(), 2);
                let s0 = slice_io.iter().find(|s| s.id == 0).expect("slice 0");
                assert_eq!(s0.bytes_served, 4096);
                assert_eq!(s0.io_count, 1);
            }
            other => panic!("esperava StatusReply, veio {other:?}"),
        }
    }

    #[test]
    fn on_tick_emits_telemetry() {
        // RF-5: todo tick emite uma amostra de telemetria.
        let mut c = core(1);
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(o.iter().any(|x| matches!(x, Outbound::Telemetry(_))));
    }

    #[test]
    fn eviction_flag_after_demote() {
        // RF-4/DT-6: um DEMOTE do canário → flag Eviction no tick seguinte (streak=1 no helper).
        let mut c = core(1);
        reg(&mut c, 10, "a");
        c.vram.total.store(1 << 30, Ordering::Relaxed); // has_vram (senão Partial)
        c.vram.free.store(1 << 29, Ordering::Relaxed);
        c.handle(CoreEvent::Demote("latency".into()), Instant::now());
        let o = c.handle(CoreEvent::Tick, Instant::now());
        let flag = o
            .iter()
            .find_map(|x| match x {
                Outbound::Telemetry(t) => Some(t.flag),
                _ => None,
            })
            .expect("telemetria no tick");
        assert_eq!(flag, ReconcileFlag::Eviction);
    }

    #[test]
    fn unaccounted_when_occupied_exceeds_alloc() {
        // RF-4: ocupado > emprestado + tol → Unaccounted (sem demote, com VRAM).
        let mut c = core(1); // 1 slice de 64 MiB
        reg(&mut c, 10, "a");
        let id = *c.tenants.keys().next().expect("tenant");
        c.slice_map.assign(0, id).expect("assign slice 0"); // Free→Active (64 MiB emprestado)
        c.tenants.get_mut(&id).expect("tenant").occupied_bytes = 200 * 1024 * 1024; // 200 MiB > 64
        c.vram.total.store(1 << 30, Ordering::Relaxed); // has_vram
        let o = c.handle(CoreEvent::Tick, Instant::now());
        let flag = o
            .iter()
            .find_map(|x| match x {
                Outbound::Telemetry(t) => Some(t.flag),
                _ => None,
            })
            .expect("telemetria no tick");
        assert_eq!(flag, ReconcileFlag::Unaccounted);
    }

    fn tick_flag(c: &mut BrokerCore) -> ReconcileFlag {
        c.handle(CoreEvent::Tick, Instant::now())
            .iter()
            .find_map(|x| match x {
                Outbound::Telemetry(t) => Some(t.flag),
                _ => None,
            })
            .expect("telemetria no tick")
    }

    #[test]
    fn eviction_confirmed_immediately_despite_streak() {
        // C1: Eviction é EVENTO (canário) → confirma em 1 tick mesmo com recon_streak=3 (produção).
        // Sem o fix, a histerese engoliria a evicção transitória.
        let mut c = core_streak(1, 3);
        reg(&mut c, 10, "a");
        c.vram.total.store(1 << 30, Ordering::Relaxed); // has_vram (senão Partial)
        c.vram.free.store(1 << 29, Ordering::Relaxed);
        c.handle(CoreEvent::Demote("latency".into()), Instant::now());
        assert_eq!(tick_flag(&mut c), ReconcileFlag::Eviction);
    }

    #[test]
    fn unaccounted_respects_streak() {
        // RF-4/DT-12: flag SUSTENTADO (Unaccounted) só confirma após `recon_streak` ticks.
        let mut c = core_streak(1, 2);
        reg(&mut c, 10, "a");
        let id = *c.tenants.keys().next().expect("tenant");
        c.slice_map.assign(0, id).expect("assign");
        c.tenants.get_mut(&id).expect("tenant").occupied_bytes = 200 * 1024 * 1024;
        c.vram.total.store(1 << 30, Ordering::Relaxed);
        assert_eq!(
            tick_flag(&mut c),
            ReconcileFlag::None,
            "1º tick: streak ainda não atingido"
        );
        assert_eq!(
            tick_flag(&mut c),
            ReconcileFlag::Unaccounted,
            "2º tick: confirma"
        );
    }

    #[test]
    fn telemetry_sink_writes_jsonl_line() {
        // RF-5/DT-8: o sink escreve 1 objeto JSON por linha (write-no-arquivo, in-process).
        let path =
            std::env::temp_dir().join(format!("ramshared-telem-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut sink = TelemetrySink::open(path.clone()).expect("abre sink");
            let tc = TelemetryCore {
                tenant: None,
                slice: None,
                swap_used: 7,
                alloc_active: 8,
                page_io_s: None,
                vram_alloc_daemon: 9,
                vram_total_used: None,
                vram_outros: None,
                canario_demotes: 0,
                demote_reason: None,
                reconcile_delta: 0.0,
                flag: ReconcileFlag::None,
            };
            sink.emit(&tc);
            sink.emit(&tc);
        }
        let content = std::fs::read_to_string(&path).expect("lê o jsonl");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "uma linha por emit");
        let v: serde_json::Value = serde_json::from_str(lines[0]).expect("json válido");
        assert_eq!(v["swap_used"], 7);
        assert!(v.get("t").is_some(), "carimbo de tempo presente");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dev_to_slice_parses_trailing_int() {
        assert_eq!(dev_to_slice("/dev/nbd5"), Some(5));
        assert_eq!(dev_to_slice("/dev/nbd0"), Some(0));
        assert_eq!(dev_to_slice("/dev/sda"), None);
    }

    const SLICE: u64 = 64 * 1024 * 1024;

    fn lease_req(c: &mut BrokerCore, sid: usize, bytes: u64) -> Vec<Outbound> {
        c.handle(
            CoreEvent::Msg(sid, Msg::LeaseRequest { bytes }),
            Instant::now(),
        )
    }

    fn n_leased(c: &BrokerCore) -> usize {
        c.slice_map
            .slices()
            .iter()
            .filter(|s| s.state == SliceState::Leased)
            .count()
    }

    #[test]
    fn lease_granted_from_free_slices() {
        let mut c = core(2);
        reg(&mut c, 10, "dcc");
        psi(&mut c, 10, 0.0);
        let o = lease_req(&mut c, 10, SLICE); // 1 slice
        assert!(
            !o.iter()
                .any(|x| matches!(x, Outbound::ToSession(_, Msg::LeaseDenied { .. })))
        );
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            o.iter()
                .any(|x| matches!(x, Outbound::ToSession(10, Msg::LeaseGranted { .. })))
        );
        assert_eq!(n_leased(&c), 1);
    }

    #[test]
    fn lease_denied_when_in_progress() {
        let mut c = core(2);
        reg(&mut c, 10, "dcc");
        psi(&mut c, 10, 0.0);
        lease_req(&mut c, 10, SLICE);
        let o = lease_req(&mut c, 10, SLICE);
        assert!(o.iter().any(
            |x| matches!(x, Outbound::ToSession(10, Msg::LeaseDenied { reason }) if reason == "lease_em_andamento")
        ));
    }

    #[test]
    fn lease_denied_over_capacity() {
        let mut c = core(2); // total = 128 MiB
        reg(&mut c, 10, "dcc");
        psi(&mut c, 10, 0.0);
        let o = lease_req(&mut c, 10, 200 * 1024 * 1024);
        assert!(o.iter().any(
            |x| matches!(x, Outbound::ToSession(10, Msg::LeaseDenied { reason }) if reason == "acima_da_capacidade")
        ));
    }

    #[test]
    fn lease_release_returns_slices() {
        let mut c = core(2);
        reg(&mut c, 10, "dcc");
        psi(&mut c, 10, 0.0);
        lease_req(&mut c, 10, SLICE);
        c.handle(CoreEvent::Tick, Instant::now()); // GrantLease (lease id 1)
        assert_eq!(n_leased(&c), 1);
        c.handle(
            CoreEvent::Msg(10, Msg::LeaseRelease { lease: 1 }),
            Instant::now(),
        );
        assert_eq!(n_leased(&c), 0); // devolvida ao tier de swap
    }

    #[test]
    fn lease_revokes_active_then_grants_after_zero() {
        // DT-8/R2: lease drena uma slice Active (de outro tenant), zera, e concede.
        let mut c = core(1);
        reg(&mut c, 10, "swap"); // id 1
        reg(&mut c, 20, "dcc"); // id 2
        psi(&mut c, 10, 0.0);
        psi(&mut c, 20, 0.0);
        c.handle(CoreEvent::Tick, Instant::now()); // s0 → swap(1) Active (round-robin)
        assert_eq!(c.slice_map.get(0).unwrap().tenant, Some(1));
        // dcc(20) pede lease que precisa da slice → revoga de swap(1)
        lease_req(&mut c, 20, SLICE);
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            o.iter()
                .any(|x| matches!(x, Outbound::ToSession(10, Msg::SwapOff { slice: 0 })))
        );
        assert!(
            !o.iter()
                .any(|x| matches!(x, Outbound::ToSession(_, Msg::LeaseGranted { .. })))
        ); // ainda não
        // swapoff confirma → zero → release → Free
        c.handle(
            CoreEvent::Msg(
                10,
                Msg::SwapOffDone {
                    slice: 0,
                    ok: true,
                    detail: String::new(),
                },
            ),
            Instant::now(),
        );
        c.handle(CoreEvent::ZeroDone(0, true), Instant::now());
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Free);
        // próximo tick: agora há Free suficiente → GrantLease p/ dcc(20)
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            o.iter()
                .any(|x| matches!(x, Outbound::ToSession(20, Msg::LeaseGranted { .. })))
        );
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Leased);
    }

    #[test]
    fn lease_released_when_holder_disconnects() {
        let mut c = core(2);
        reg(&mut c, 10, "dcc");
        psi(&mut c, 10, 0.0);
        lease_req(&mut c, 10, SLICE);
        c.handle(CoreEvent::Tick, Instant::now()); // grant
        assert_eq!(n_leased(&c), 1);
        c.handle(CoreEvent::Disconnected(10), Instant::now()); // holder cai
        assert_eq!(n_leased(&c), 0); // lease liberado (DT-19)
    }
}
