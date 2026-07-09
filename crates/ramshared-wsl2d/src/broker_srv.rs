//! `broker_srv` — **pure** core of the broker (decision/state), testable without threads/sockets/GPU.
//!
//! Same discipline as the arbiter: [`BrokerCore`] receives [`CoreEvent`]s and returns [`Outbound`]s
//! that the IO layer ([`spawn_broker`]) executes — send `Msg` to the session, close session, request
//! the worker to zero a slice (DT-17), or log (RF-B4).
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md ITEM-8. Covers: sessions + Register/Psi/Ack/Status/Disconnect
//! (DT-18/20/22), reconciliation (DT-9/21), rebalancing with hygiene (DT-17), DemoteAll and **revocable
//! lease (RF-B3/DT-19)**. The IO layer ([`spawn_broker`]) runs the core over TCP (DT-2/24);
//! only the wiring in the daemon's `run_nbd` is missing (`--slices`/`--arbiter-listen`).

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

/// NBD endpoints that agents receive in `SwapOn`, chosen by the transport (DT-25).
#[derive(Clone, Debug, Default)]
pub struct EndpointCfg {
    pub nbd_unix: Option<String>,
    pub nbd_tcp: Option<(String, u16)>,
}

/// Input event for the core (produced by the IO layer). `sid` = connection ID (session).
#[derive(Clone, Debug, PartialEq)]
pub enum CoreEvent {
    Msg(usize, Msg),
    Disconnected(usize),
    ZeroDone(SliceId, bool),
    Demote(String),
    Tick,
}

/// Output action executed by the IO layer.
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
    /// Reconciled telemetry sample (RF-5); the IO layer timestamps `t`/`branch`/`commit`.
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
    /// Last memory telemetry of the tenant (RF-2); `None` if the agent does not report cgroup/diskstats.
    mem: Option<TenantMem>,
    /// Occupied swap (bytes) in the `Active` slices of this tenant, derived in `Psi` (DT-10/F-v2-2).
    occupied_bytes: u64,
}

/// Broker core: sole owner of `SliceMap` + `Arbiter` + session table (without locks; the
/// IO layer runs this in a single thread). `BTreeMap` by `TenantId` gives stable iteration
/// (deterministic round-robin).
pub struct BrokerCore {
    slice_map: SliceMap,
    arbiter: Arbiter,
    endpoints: EndpointCfg,
    swap_prio: Option<i32>,
    tenants: BTreeMap<TenantId, TenantState>,
    sessions: HashMap<usize, TenantId>,    // live connection → tenant
    name_to_id: HashMap<String, TenantId>, // stable ID by name (DT-22)
    next_tenant: TenantId,
    pending_dest: HashMap<SliceId, TenantId>, // destination of a MoveSlice in flight (post-zero)
    pending_lease: Option<(TenantId, u64)>,   // requested lease, not yet granted (RF-B3)
    lease: Option<(u32, TenantId)>,           // active lease (id, holder); the id comes from the arbiter
    last_rebalance: Option<Instant>,
    // Telemetria/reconciliação (SPEC broker-telemetry-reconciliation).
    slice_io: Arc<Vec<SliceIoCounters>>, // counters per slice (data-plane writes, RF-1/DT-1)
    vram: Arc<VramGauge>,                // VRAM gauge published by the worker (RF-3/DT-5)
    demotes_total: u64,                  // accumulated canary DEMOTEs (RF-4)
    last_demote_reason: Option<String>,
    demotes_at_last_sample: u64, // baseline for the `demotes_delta` per tick
    recon_flag: ReconcileFlag,   // candidate flag (hysteresis DT-12)
    recon_count: u32,            // consecutive ticks with the same candidate flag
    tol_frac: f64,               // reconciliation tolerance (DT-7)
    recon_streak: u32,           // ticks to confirm the flag (DT-12)
    // R4: slices post-`SwapOffDone` waiting for zero confirmation (`ZeroDone`), with number of ticks
    // without confirmation. If `try_send(ZeroExport)` failed (channel full), `ZeroDone` does not arrive → the tick
    // re-emits zero (retry) until confirmed; escalates to ERROR after N. Only slices HERE have already been
    // swapped-off by the tenant (safe to re-zero); slices in Draining awaiting SwapOffDone are NOT included.
    pending_zero: HashMap<SliceId, u32>,
}

/// Grace period (in ticks) before the 1st zero retry — gives time for `ZeroDone` in flight. Above this, the
/// tick re-emits `ZeroExport`; in [`ZERO_RETRY_ERROR`] ticks without confirmation, logs ERROR (R4).
const ZERO_RETRY_GRACE: u32 = 1;
const ZERO_RETRY_ERROR: u32 = 5;

/// Extracts the trailing integer of a device (`/dev/nbd5` → 5), agnostic to the prefix (DT-21).
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

    /// Sends `msg` to the tenant's live session (or logs if absent — DT-20).
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
                    // mount failed: nothing was written → returns the slice directly (without zero).
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
                    // hygiene (DT-17): zeroes before release. ZeroDone → release. The tenant already did
                    // swapoff → safe to zero; enters pending_zero for the tick retry (R4).
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
                // Broker→agent messages never arrive here; unexpected shape → close.
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
        // DT-22: tenant already has a different live session → duplicate.
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
            // Reconciliation (DT-9/21): on the 1st Psi, re-adopts slices that the agent already has mounted.
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
        // occupied (DT-10/F-v2-2): Σ used_kb*1024 of swaps that match Active slices of this tenant.
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
        // P1: at most 1 lease pending/active (DT-19).
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
            return; // unknown lease; ignore
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
            let _ = self.slice_map.unlease(slice); // Leased → Free (round-robin re-leases)
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
            // Lease holder/requester dropped → automatic release/cancel (DT-19).
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
            // retry zeroing on the next movement event; for now log and remain Draining (R4).
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
        // Movement in flight: the cleaned slice goes to the destination (SwapOn).
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
        // DT-20: only present tenants; only Free slices or slices with a present owner.
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
                    // Revokes for the lease: drain+SwapOff; on ZeroDone becomes Free (no pending_dest),
                    // and the arbiter counts it for the lease on the next tick (DT-8/R2).
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

        // R4: retries zeroing of stuck slices (if `try_send(ZeroExport)` failed, `ZeroDone` doesn't arrive).
        // Grace period of 1 tick for zero in flight; above that, re-emits; ERROR after N
        // ticks without confirmation. Only touches slices in `pending_zero` (already swapped-off; safe to re-zero).
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

    /// Reconciliation per tick (RF-4/RF-5): occupancy invariant + hysteresis (DT-12) → emits
    /// `Outbound::Telemetry`. Observer: does not touch the arbiter/SliceMap.
    fn emit_telemetry(&mut self, out: &mut Vec<Outbound>) {
        let alloc_active: u64 = self
            .slice_map
            .slices()
            .iter()
            .filter(|s| matches!(s.state, SliceState::Active | SliceState::Draining))
            .map(|s| s.len)
            .sum();
        let occupied: u64 = self.tenants.values().map(|t| t.occupied_bytes).sum();
        // Σ diskstats (cumulative in bytes) of tenants reporting `mem` (RF-2); `None` if nobody
        // reports. The consumer derives the rate by the difference between samples (`t` field of the line).
        let page_io: Option<u64> = self.tenants.values().any(|t| t.mem.is_some()).then(|| {
            self.tenants
                .values()
                .filter_map(|t| t.mem.as_ref())
                .map(|m| m.diskstats_io)
                .sum()
        });
        let free = self.vram.free.load(Ordering::Relaxed);
        let total = self.vram.total.load(Ordering::Relaxed);
        let has_vram = total > 0; // F-v2-6: RAM sentinel (no GPU) → vram_* = None
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
        // Hysteresis (DT-12) for SUSTAINED flags (Unaccounted/StuckSlice/Partial): confirms after
        // `recon_streak` consecutive identical ticks. `Eviction` is a canary EVENT (DT-6;
        // `demotes_delta` is per-tick, lasts 1 tick) → immediate confirmation; otherwise hysteresis would swallow
        // a transient eviction (1-2 DEMOTEs) and the eviction signal would never appear (bug C1).
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

// ===================== IO Layer (DT-2/DT-24) =====================
//
// `BrokerCore` is pure; `spawn_broker` is the thin shell of threads running it: TCP acceptor,
// reader/writer per session (writer = bounded channel 64, DT-24), the core loop (`recv_timeout`
// = tick) and DEMOTE and zero-done forwarders. The core never does socket IO.

/// Broker config (DT-2: in-process in the daemon; RNF-2: `listen` already validated as non-unspecified).
pub struct BrokerConfig {
    pub listen: SocketAddr,
    pub endpoints: EndpointCfg,
    pub swap_prio: Option<i32>,
    pub arbiter: ArbiterConfig,
    pub tick: Duration,
    /// Telemetry (SPEC): counters per slice + VRAM gauge (shared with the worker).
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

/// Starts the broker: acceptor + sessions + single-thread core. Returns the core handle and the
/// bound `SocketAddr` (useful with port 0 in tests). `shutdown` triggers `DemoteAll` + exit.
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
    // DEMOTE forwarder (canary/residency) → CoreEvent::Demote.
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
    // Core (single thread owner of BrokerCore). Keeps an `io_tx` for the zero-done forwarders.
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
    // closed channel (CloseSession/backpressure) or error → closes the socket (the reader sees EOF).
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
    // Wall-clock deadline for the next Tick. CRITICAL: the Arbiter's Tick MUST NOT be starved
    // by messages. Pure `recv_timeout(tick)` never expires under normal `Psi` flow
    // (~1/s per tenant) → the arbiter would never run `AssignFree`/rebalance. Here the wait shrinks
    // as messages arrive, and the Tick fires when the deadline passes, regardless of
    // message rate. (Bug caught in e2e cross-host civm; the QEMU drill passed by luck of timing.)
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
                // DT-24: try_send; channel full/dead → drops the session (without blocking the core).
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
                    eprintln!("[ramsharedd] WARN jobs channel full; zeroing of s{slice} postponed (R4)");
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

/// JSONL telemetry sink (RF-5/DT-8): timestamps `t`/`branch`/`commit` on the core's `TelemetryCore` and
/// appends 1 JSON object per line. `branch`/`commit` come from env
/// (`RAMSHARED_BUILD_BRANCH`/`RAMSHARED_BUILD_COMMIT`; `None` if absent — F-v2-4).
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

    /// Like `core`, but with configurable `recon_streak` (tests the actual production hysteresis).
    fn core_streak(k: u16, recon_streak: u32) -> BrokerCore {
        let cfg = ArbiterConfig {
            streak: 1, // move already on the 1st tick above delta (tests)
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
        let o = reg(&mut c, 11, "wsl2"); // same name, another live connection
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
        // 2 Free slices → SwapOn for each tenant (round-robin: s0→a(1), s1→b(2))
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
        c.slice_map.drain(0).unwrap(); // simulates start of move (Active→Draining)
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
        // R4: if zeroing is not confirmed (channel full → no ZeroDone), the tick re-emits ZeroExport
        // after grace and escalates to ERROR; once confirmed, stops retrying.
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
        // WITHOUT ZeroDone (simulates full channel). Tick 1 = grace → does not re-emit.
        let t1 = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            !t1.iter()
                .any(|x| matches!(x, Outbound::ZeroSlice { slice: 0, .. })),
            "carência: sem retry no 1º tick"
        );
        // Tick 2 → re-emits the zero.
        let t2 = c.handle(CoreEvent::Tick, Instant::now());
        assert!(
            t2.iter()
                .any(|x| matches!(x, Outbound::ZeroSlice { slice: 0, .. })),
            "retry do zero no 2º tick"
        );
        // More ticks → escalates to ERROR (R4).
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
        // Zero confirms → Free + stops retrying.
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
        // imbalance: recv (2) highly pressured, donor (1) idle, no Free slices → move
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
        // SwapOffDone → zero → ZeroDone → SwapOn for recv (session 20)
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
        // slice remains Active (frozen), owner absent
        assert_eq!(c.slice_map.get(0).unwrap().state, SliceState::Active);
        // tick does not touch the absent one's slice (does not become Free nor reassigned)
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
        // RF-1: StatusReply exposes counters per slice (read from the shared Arc).
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
        // RF-5: every tick emits a telemetry sample.
        let mut c = core(1);
        let o = c.handle(CoreEvent::Tick, Instant::now());
        assert!(o.iter().any(|x| matches!(x, Outbound::Telemetry(_))));
    }

    #[test]
    fn eviction_flag_after_demote() {
        // RF-4/DT-6: a canary DEMOTE → Eviction flag on the next tick (streak=1 in helper).
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
        // RF-4: occupied > borrowed + tol → Unaccounted (no demote, with VRAM).
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
        // C1: Eviction is an EVENT (canary) → confirms in 1 tick even with recon_streak=3 (production).
        // Without the fix, the hysteresis would swallow transient eviction.
        let mut c = core_streak(1, 3);
        reg(&mut c, 10, "a");
        c.vram.total.store(1 << 30, Ordering::Relaxed); // has_vram (senão Partial)
        c.vram.free.store(1 << 29, Ordering::Relaxed);
        c.handle(CoreEvent::Demote("latency".into()), Instant::now());
        assert_eq!(tick_flag(&mut c), ReconcileFlag::Eviction);
    }

    #[test]
    fn unaccounted_respects_streak() {
        // RF-4/DT-12: SUSTAINED flag (Unaccounted) only confirms after `recon_streak` ticks.
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
        // RF-5/DT-8: the sink writes 1 JSON object per line (write-in-file, in-process).
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
        assert_eq!(n_leased(&c), 0); // lease released (DT-19)
    }
}
