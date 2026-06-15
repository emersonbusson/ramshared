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
use std::io::{self, BufReader};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ramshared_broker::arbiter::{Action, Arbiter, ArbiterConfig, TenantView};
use ramshared_broker::model::{PsiSample, Slice, SliceId, SliceState, TenantId, TransportKind};
use ramshared_broker::protocol::{
    Msg, NbdEndpoint, PROTO_VERSION, SwapEntry, TenantStatus, read_msg, write_msg,
};
use ramshared_broker::slices::SliceMap;

use crate::conn::WMsg;
use crate::residency::DemoteReason;

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
    ZeroSlice { slice: SliceId, base: u64, len: u64 },
    Log(String),
}

#[derive(Clone, Debug)]
struct TenantState {
    name: String,
    transport: TransportKind,
    present: bool,
    sid: Option<usize>,
    psi: PsiSample,
    reconciled: bool,
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
}

/// Extrai o inteiro final de um device (`/dev/nbd5` → 5), agnóstico ao prefixo (DT-21).
fn dev_to_slice(dev: &str) -> Option<SliceId> {
    let tail: String = dev.chars().rev().take_while(char::is_ascii_digit).collect();
    if tail.is_empty() {
        return None;
    }
    tail.chars().rev().collect::<String>().parse().ok()
}

impl BrokerCore {
    pub fn new(
        slice_map: SliceMap,
        arbiter_cfg: ArbiterConfig,
        endpoints: EndpointCfg,
        swap_prio: Option<i32>,
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
            Msg::Psi { sample, swaps } => self.on_psi(sid, sample, swaps, out),
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
                    // higiene (DT-17): zera antes de liberar. ZeroDone → release.
                    if let Some(s) = self.slice_map.get(slice) {
                        out.push(Outbound::ZeroSlice {
                            slice,
                            base: s.offset,
                            len: s.len,
                        });
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
            })
            .collect();
        Msg::StatusReply {
            tenants,
            slices: self.slice_map.slices().to_vec(),
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
    let core = BrokerCore::new(slice_map, cfg.arbiter, cfg.endpoints, cfg.swap_prio);
    let tick = cfg.tick;
    let handle = thread::spawn(move || core_loop(core, &io_rx, &io_tx, &jobs, tick, &shutdown));
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

fn core_loop(
    mut core: BrokerCore,
    io_rx: &Receiver<IoEvent>,
    io_tx: &Sender<IoEvent>,
    jobs: &SyncSender<WMsg>,
    tick: Duration,
    shutdown: &AtomicBool,
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
            dispatch(outs, &mut sessions, jobs, io_tx);
            break;
        }
        let wait = next_tick.saturating_duration_since(Instant::now());
        match io_rx.recv_timeout(wait) {
            Ok(IoEvent::NewSession(sid, wtx)) => {
                sessions.insert(sid, wtx);
            }
            Ok(IoEvent::Core(ev)) => {
                let outs = core.handle(ev, Instant::now());
                dispatch(outs, &mut sessions, jobs, io_tx);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_tick {
            let outs = core.handle(CoreEvent::Tick, Instant::now());
            dispatch(outs, &mut sessions, jobs, io_tx);
            next_tick = Instant::now() + tick;
        }
    }
}

fn dispatch(
    outs: Vec<Outbound>,
    sessions: &mut HashMap<usize, SyncSender<Msg>>,
    jobs: &SyncSender<WMsg>,
    io_tx: &Sender<IoEvent>,
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
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use ramshared_broker::model::SliceState;

    fn core(k: u16) -> BrokerCore {
        let mut cfg = ArbiterConfig::default();
        cfg.streak = 1; // move já no 1º tick acima do delta (testes)
        BrokerCore::new(
            SliceMap::new(k, 64 * 1024 * 1024),
            cfg,
            EndpointCfg {
                nbd_unix: Some("/run/x.sock".into()),
                nbd_tcp: None,
            },
            None,
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
