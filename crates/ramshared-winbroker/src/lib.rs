use ramshared_broker::lease::{LeaseBook, LeaseDecision, LeaseDeny, LogicalLease};
use ramshared_broker::model::TransportKind;
use ramshared_broker::protocol::{Msg, PROTO_VERSION};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[cfg(windows)]
pub mod pipe;
#[cfg(windows)]
pub mod service;

const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokerPhase {
    Stopped,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfigV1 {
    pub schema: u32,
    pub capacity_bytes: u64,
    pub allowed_tenant: String,
    pub evidence_path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerConfigDocument {
    local_broker: BrokerConfigV1,
}

impl BrokerConfigV1 {
    pub fn from_toml(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err("broker config exceeds 64 KiB".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "broker config is not UTF-8")?;
        let document: BrokerConfigDocument = toml::from_str(text).map_err(|e| e.to_string())?;
        document.local_broker.validate()?;
        Ok(document.local_broker)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("local_broker.schema must be 1".into());
        }
        if self.capacity_bytes == 0 || !self.capacity_bytes.is_multiple_of(4096) {
            return Err("capacity_bytes must be non-zero and 4096-byte aligned".into());
        }
        if self.allowed_tenant.is_empty() {
            return Err("allowed_tenant must be non-empty".into());
        }
        if !is_absolute_windows_or_native(&self.evidence_path) {
            return Err("evidence_path must be absolute".into());
        }
        Ok(())
    }
}

fn is_absolute_windows_or_native(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    value.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

#[derive(Clone, Debug, PartialEq)]
pub enum BrokerEffect {
    Reply(Msg),
    Close,
    Audit(String),
    LeaseReleased(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BrokerStatusV1 {
    pub broker_instance_id: String,
    pub registered: bool,
    pub active_lease: Option<LogicalLease>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrokerStatusRequestV1 {
    Status,
}

pub struct IpcRingBuffer {
    buffers: Vec<Vec<u8>>,
    index: usize,
}

impl IpcRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let actual_capacity = std::cmp::max(1, capacity);
        Self {
            buffers: (0..actual_capacity)
                .map(|_| Vec::with_capacity(4096))
                .collect(),
            index: 0,
        }
    }

    pub fn serialize_msg(&mut self, msg: &Msg) -> Result<&[u8], std::io::Error> {
        let index = self.index;
        let buffer = &mut self.buffers[index];
        buffer.clear();
        serde_json::to_writer(&mut *buffer, msg)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        buffer.push(b'\n');

        let len = self.buffers.len();
        self.index = (index + 1) % len;

        Ok(&self.buffers[index])
    }
}

pub struct BrokerSessionCore {
    allowed_tenant: String,
    broker_instance_id: String,
    live_session: Option<usize>,
    lease_book: LeaseBook,
}

impl BrokerSessionCore {
    pub fn new(
        capacity_bytes: u64,
        allowed_tenant: impl Into<String>,
        broker_instance_id: impl Into<String>,
    ) -> Self {
        Self {
            allowed_tenant: allowed_tenant.into(),
            broker_instance_id: broker_instance_id.into(),
            live_session: None,
            lease_book: LeaseBook::new(capacity_bytes),
        }
    }

    pub fn on_authenticated_msg(&mut self, session_id: usize, message: Msg) -> Vec<BrokerEffect> {
        if self.live_session != Some(session_id) {
            return self.on_unregistered_msg(session_id, message);
        }
        match message {
            Msg::LeaseRequest { bytes } => match self.lease_book.begin_request(1, bytes) {
                LeaseDecision::Pending(_) => match self.lease_book.grant_pending(bytes) {
                    Ok(lease) => vec![
                        BrokerEffect::Audit("lease_granted".into()),
                        BrokerEffect::Reply(Msg::LeaseGranted {
                            lease: lease.id,
                            bytes: lease.bytes,
                        }),
                    ],
                    Err(reason) => vec![Self::denied(reason)],
                },
                LeaseDecision::Denied(reason) => vec![Self::denied(reason)],
            },
            Msg::LeaseRelease { lease } => match self.lease_book.release(1, lease) {
                Ok(true) => vec![
                    BrokerEffect::Audit("lease_released_explicit".into()),
                    BrokerEffect::LeaseReleased(lease),
                ],
                Ok(false) | Err(LeaseDeny::WrongLease) => {
                    vec![BrokerEffect::Audit("lease_release_idempotent".into())]
                }
                Err(reason) => vec![Self::error_and_close(reason), BrokerEffect::Close],
            },
            // WinDrive PSI is a liveness heartbeat only. It is deliberately
            // excluded from local arbitration, but it must keep the
            // authoritative lease session open.
            Msg::Psi { .. } => Vec::new(),
            _ => vec![
                BrokerEffect::Reply(Msg::Error {
                    reason: "unexpected_message".into(),
                }),
                BrokerEffect::Close,
            ],
        }
    }

    fn on_unregistered_msg(&mut self, session_id: usize, message: Msg) -> Vec<BrokerEffect> {
        let Msg::Register {
            proto,
            tenant,
            transport,
        } = message
        else {
            return vec![
                BrokerEffect::Reply(Msg::Error {
                    reason: "register_required".into(),
                }),
                BrokerEffect::Close,
            ];
        };
        if self.live_session.is_some()
            || proto != PROTO_VERSION
            || tenant != self.allowed_tenant
            || transport != TransportKind::WinDrive
        {
            return vec![
                BrokerEffect::Reply(Msg::Error {
                    reason: "registration_refused".into(),
                }),
                BrokerEffect::Close,
            ];
        }
        self.live_session = Some(session_id);
        vec![
            BrokerEffect::Audit("registered_ready".into()),
            BrokerEffect::Reply(Msg::Registered { tenant_id: 1 }),
        ]
    }

    pub fn on_disconnect(&mut self, session_id: usize) -> Vec<BrokerEffect> {
        if self.live_session != Some(session_id) {
            return Vec::new();
        }
        self.live_session = None;
        let disconnected = self.lease_book.disconnect(1);
        let mut effects = vec![BrokerEffect::Audit("session_disconnected".into())];
        if let Some(lease) = disconnected.released {
            effects.push(BrokerEffect::Audit(
                "lease_released_on_disconnect_ambiguous".into(),
            ));
            effects.push(BrokerEffect::LeaseReleased(lease.id));
        }
        effects
    }

    pub fn status(&self) -> BrokerStatusV1 {
        BrokerStatusV1 {
            broker_instance_id: self.broker_instance_id.clone(),
            registered: self.live_session.is_some(),
            active_lease: self.lease_book.active().cloned(),
        }
    }

    fn denied(reason: LeaseDeny) -> BrokerEffect {
        BrokerEffect::Reply(Msg::LeaseDenied {
            reason: format!("{reason:?}").to_ascii_lowercase(),
        })
    }

    fn error_and_close(reason: LeaseDeny) -> BrokerEffect {
        BrokerEffect::Reply(Msg::Error {
            reason: format!("{reason:?}").to_ascii_lowercase(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::BrokerConfigV1;
    use super::{BrokerEffect, BrokerSessionCore, MAX_CONFIG_BYTES};
    use ramshared_broker::model::TransportKind;
    use ramshared_broker::protocol::{Msg, PROTO_VERSION};

    fn register(tenant: &str) -> Msg {
        Msg::Register {
            proto: PROTO_VERSION,
            tenant: tenant.into(),
            transport: TransportKind::WinDrive,
        }
    }

    #[test]
    fn register_is_readiness_without_lease() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        let effects = core.on_authenticated_msg(1, register("winsvc"));
        assert!(effects.contains(&BrokerEffect::Reply(Msg::Registered { tenant_id: 1 })));
        assert!(core.status().active_lease.is_none());
    }

    #[test]
    fn message_before_register_is_refused() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        let effects = core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        assert!(effects.contains(&BrokerEffect::Close));
    }

    #[test]
    fn tenant_mismatch_is_refused() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        assert!(
            core.on_authenticated_msg(1, register("other"))
                .contains(&BrokerEffect::Close)
        );
    }

    #[test]
    fn one_live_session_only() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        assert!(
            core.on_authenticated_msg(2, register("winsvc"))
                .contains(&BrokerEffect::Close)
        );
    }

    #[test]
    fn exact_lease_grant_and_release() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        let granted = core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        assert!(granted.contains(&BrokerEffect::Reply(Msg::LeaseGranted {
            lease: 1,
            bytes: 1024
        })));
        let released = core.on_authenticated_msg(1, Msg::LeaseRelease { lease: 1 });
        assert!(released.contains(&BrokerEffect::LeaseReleased(1)));
    }

    #[test]
    fn registered_psi_heartbeat_keeps_lease_session_open() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });

        let effects = core.on_authenticated_msg(
            1,
            Msg::Psi {
                sample: Default::default(),
                swaps: Vec::new(),
                mem: None,
            },
        );

        assert!(effects.is_empty());
        assert_eq!(
            core.status().active_lease.map(|lease| lease.bytes),
            Some(1024)
        );
        assert!(
            !core
                .on_authenticated_msg(1, Msg::LeaseRelease { lease: 1 })
                .contains(&BrokerEffect::Close)
        );
    }

    #[test]
    fn registered_session_refuses_non_lease_control_messages() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));

        let effects = core.on_authenticated_msg(1, Msg::Status);

        assert!(effects.contains(&BrokerEffect::Reply(Msg::Error {
            reason: "unexpected_message".into()
        })));
        assert!(effects.contains(&BrokerEffect::Close));
    }

    #[test]
    fn lease_capacity_and_duplicate_requests_are_refused() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        assert!(
            core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 2048 })
                .iter()
                .any(|effect| matches!(effect, BrokerEffect::Reply(Msg::LeaseDenied { .. })))
        );
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        assert!(
            core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 })
                .iter()
                .any(|effect| matches!(effect, BrokerEffect::Reply(Msg::LeaseDenied { .. })))
        );
    }

    #[test]
    fn lease_release_is_idempotent() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        core.on_authenticated_msg(1, Msg::LeaseRelease { lease: 1 });

        assert_eq!(
            core.on_authenticated_msg(1, Msg::LeaseRelease { lease: 1 }),
            vec![BrokerEffect::Audit("lease_release_idempotent".into())]
        );
    }

    #[test]
    fn unrelated_disconnect_does_not_change_live_session() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });

        assert!(core.on_disconnect(2).is_empty());
        assert!(core.status().registered);
        assert!(core.status().active_lease.is_some());
    }

    #[test]
    fn disconnect_releases_server_state_and_audits_ambiguous() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "01");
        core.on_authenticated_msg(1, register("winsvc"));
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        let effects = core.on_disconnect(1);
        assert!(effects.contains(&BrokerEffect::LeaseReleased(1)));
        assert!(effects.iter().any(
            |e| matches!(e, BrokerEffect::Audit(s) if s == "lease_released_on_disconnect_ambiguous")
        ));
    }

    #[test]
    fn status_has_instance_and_lease_state() {
        let mut core = BrokerSessionCore::new(1024, "winsvc", "0123456789abcdef0123456789abcdef");
        core.on_authenticated_msg(1, register("winsvc"));
        core.on_authenticated_msg(1, Msg::LeaseRequest { bytes: 1024 });
        let status = core.status();
        assert_eq!(
            status.broker_instance_id,
            "0123456789abcdef0123456789abcdef"
        );
        assert_eq!(status.active_lease.map(|l| l.bytes), Some(1024));
    }

    #[test]
    fn example_config_parses() {
        let config = BrokerConfigV1::from_toml(include_bytes!("../broker.example.toml")).unwrap();
        assert_eq!(config.capacity_bytes, 536_870_912);
    }

    #[test]
    fn capacity_must_be_nonzero_and_aligned() {
        let example = include_str!("../broker.example.toml");
        assert!(BrokerConfigV1::from_toml(example.replace("536870912", "0").as_bytes()).is_err());
        assert!(
            BrokerConfigV1::from_toml(example.replace("536870912", "536870913").as_bytes())
                .is_err()
        );
    }

    #[test]
    fn config_rejects_oversize_schema_tenant_and_relative_path() {
        assert!(BrokerConfigV1::from_toml(&vec![b'x'; MAX_CONFIG_BYTES + 1]).is_err());
        let example = include_str!("../broker.example.toml");
        assert!(
            BrokerConfigV1::from_toml(example.replace("schema = 1", "schema = 2").as_bytes())
                .is_err()
        );
        assert!(
            BrokerConfigV1::from_toml(
                example
                    .replace(
                        "allowed_tenant = \"windrive-host\"",
                        "allowed_tenant = \"\"",
                    )
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            BrokerConfigV1::from_toml(
                example
                    .replace(
                        r#"evidence_path = "C:\\ProgramData\\RamShared\\evidence""#,
                        r#"evidence_path = "relative\\evidence""#
                    )
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_config_field_is_refused() {
        let text = format!(
            "{}\nforeign = true\n",
            include_str!("../broker.example.toml")
        );
        assert!(BrokerConfigV1::from_toml(text.as_bytes()).is_err());
    }

    #[test]
    fn ipc_ring_buffer_recycles_and_serializes() {
        let mut pool = super::IpcRingBuffer::new(3);
        let msg1 = Msg::LeaseRequest { bytes: 1024 };
        let msg2 = Msg::LeaseRelease { lease: 1 };

        let buf1 = pool.serialize_msg(&msg1).unwrap();
        assert!(buf1.ends_with(b"\n"));
        let p1 = buf1.as_ptr();

        let buf2 = pool.serialize_msg(&msg2).unwrap();
        assert!(buf2.ends_with(b"\n"));
        let p2 = buf2.as_ptr();

        assert_ne!(p1, p2, "different buffers should be used");

        let _ = pool.serialize_msg(&msg1).unwrap(); // 3rd buffer

        let buf4 = pool.serialize_msg(&msg1).unwrap(); // Back to 1st buffer
        let p4 = buf4.as_ptr();
        assert_eq!(p1, p4, "buffer should be recycled");
    }
}
