use crate::model::{PsiSample, Slice, SliceId, TenantId, TransportKind};

/// Protocol version; `Register` with `proto != PROTO_VERSION` is rejected by the broker (ITEM-8).
pub const PROTO_VERSION: u32 = 1;

/// Protocol version negotiation header.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]

pub struct VersionHeader {
    pub proto: u32,
    #[serde(default)]
    pub features: Vec<String>,
}

/// Protocol message (internally tagged by `type`, in snake_case).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]

pub enum Msg {
    // agent/client → broker
    Register {
        #[serde(flatten)]
        header: VersionHeader,
        tenant: String,
        transport: TransportKind,
    },
    Psi {
        sample: PsiSample,
        swaps: Vec<SwapEntry>,
        #[serde(default)]
        mem: Option<TenantMem>,
    },
    SwapOnDone {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
    SwapOffDone {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
    LeaseRequest {
        bytes: u64,
    },
    LeaseRelease {
        lease: u32,
    },
    Status,
    // broker → agent/client
    Registered {
        tenant_id: TenantId,
        #[serde(default)]
        features: Vec<String>,
    },
    Ack,
    SwapOn {
        slice: SliceId,
        export: String,
        endpoint: NbdEndpoint,
        swap_prio: Option<i32>,
    },
    SwapOff {
        slice: SliceId,
    },
    DemoteAll,
    LeaseGranted {
        lease: u32,
        bytes: u64,
    },
    LeaseDenied {
        reason: String,
    },
    StatusReply {
        tenants: Vec<TenantStatus>,
        slices: Vec<Slice>,
        #[serde(default)]
        slice_io: Vec<SliceIo>,
        last_rebalance_secs: Option<u64>,
    },
    Error {
        reason: String,
    },
    #[serde(other)]
    Unknown,
}

/// NBD endpoint that the agent receives in `SwapOn` (DT-25: Unix for local tenant, TCP for remote tenant).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]

pub enum NbdEndpoint {
    Unix { path: String },
    Tcp { host: String, port: u16 },
    #[serde(other)]
    Unknown,
}

/// Entry of `/proc/swaps` reported by the agent (reconciliation DT-9/DT-21; "most idle" DT-19).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]

pub struct SwapEntry {
    pub dev: String,
    pub prio: i32,
    pub size_kb: u64,
    pub used_kb: u64,
}

/// State of a tenant in `StatusReply` (RF-B4). `present=false` = session dropped (DT-20).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]

pub struct TenantStatus {
    pub id: TenantId,
    pub name: String,
    pub psi: PsiSample,
    pub slices: Vec<SliceId>,
    pub present: bool,
    #[serde(default)]
    pub bytes_served: u64,
}

/// Telemetry added in DT-9 (additive field to `Psi`): tracks exact VRAM limit/usage.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]

pub struct TenantMem {
    pub swap_current: Option<u64>,
    pub diskstats_io: u64,
}

/// IO counters per slice in `StatusReply` (RF-1 telemetry; parallel to [`Slice`] to avoid touching
/// arbitration layout).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]

pub struct SliceIo {
    pub id: SliceId,
    pub bytes_served: u64,
    pub io_count: u64,
}
