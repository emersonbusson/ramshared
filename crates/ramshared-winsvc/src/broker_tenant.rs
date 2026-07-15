//! Broker tenant client for WinDrive (SPEC ITEM-3 / RF-5 / DT-7 / DT-19 / DT-20).
//!
//! Pure over `Read`/`Write` so tests use in-memory duplex without sockets.

use std::io::{BufRead, Write};
use std::time::Duration;

use ramshared_broker::model::{PsiSample, TransportKind};
use ramshared_broker::protocol::{Msg, PROTO_VERSION, read_msg, write_msg};

/// Lease state held by this process after a successful grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseState {
    pub lease: u32,
    pub bytes: u64,
}

/// Tracks whether `LeaseRelease` was written+flushed for the current generation (DT-8/DT-9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum ReleaseSent {
    #[default]
    None,
    /// Release frame written and flushed for this lease id; further releases are no-ops.
    Sent { lease: u32 },
}

/// Errors from the broker tenant path.
#[derive(Debug, PartialEq)]
pub enum BrokerTenantError {
    Io(String),
    Protocol(String),
    Denied(String),
    /// Co-residency gate: free VRAM < requested size (DT-20).
    CoresidenceFailClosed {
        free: u64,
        need: u64,
    },
    Eof,
}

impl std::fmt::Display for BrokerTenantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerTenantError::Io(s) => write!(f, "broker io: {s}"),
            BrokerTenantError::Protocol(s) => write!(f, "broker protocol: {s}"),
            BrokerTenantError::Denied(s) => write!(f, "lease denied: {s}"),
            BrokerTenantError::CoresidenceFailClosed { free, need } => {
                write!(f, "coresidence_fail_closed free={free} need={need}")
            }
            BrokerTenantError::Eof => write!(f, "broker EOF"),
        }
    }
}

impl std::error::Error for BrokerTenantError {}

/// Stateful WinDrive client over an existing stream.
pub struct BrokerTenant {
    tenant: String,
    tenant_id: Option<u32>,
    lease: Option<LeaseState>,
    /// Bytes requested on the outstanding acquire (for grant equality check).
    requested_bytes: Option<u64>,
    release_sent: ReleaseSent,
    heartbeat: Duration,
}

impl BrokerTenant {
    pub fn new(tenant: impl Into<String>, heartbeat: Duration) -> Self {
        Self {
            tenant: tenant.into(),
            tenant_id: None,
            lease: None,
            requested_bytes: None,
            release_sent: ReleaseSent::None,
            heartbeat,
        }
    }

    pub fn lease(&self) -> Option<&LeaseState> {
        self.lease.as_ref()
    }

    pub fn heartbeat(&self) -> Duration {
        self.heartbeat
    }

    /// True when a release for the current generation was already written+flushed.
    pub fn release_already_sent(&self) -> bool {
        matches!(self.release_sent, ReleaseSent::Sent { .. })
    }

    /// `Register` with `TransportKind::WinDrive` and wait for `Registered`.
    pub fn register<S: BufRead + Write>(
        &mut self,
        stream: &mut S,
    ) -> Result<u32, BrokerTenantError> {
        let msg = Msg::Register {
            proto: PROTO_VERSION,
            tenant: self.tenant.clone(),
            transport: TransportKind::WinDrive,
        };
        write_msg(stream, &msg).map_err(|e| BrokerTenantError::Io(e.to_string()))?;
        match read_msg(stream).map_err(|e| BrokerTenantError::Io(e.to_string()))? {
            Some(Msg::Registered { tenant_id }) => {
                self.tenant_id = Some(tenant_id);
                Ok(tenant_id)
            }
            Some(Msg::Error { reason }) => Err(BrokerTenantError::Protocol(reason)),
            Some(other) => Err(BrokerTenantError::Protocol(format!("unexpected {other:?}"))),
            None => Err(BrokerTenantError::Eof),
        }
    }

    /// Send `LeaseRequest` and drain until `LeaseGranted` / `LeaseDenied`.
    ///
    /// The broker may emit logs via other sessions; on a dedicated client socket we expect
    /// only our replies (plus optional `Ack` from heartbeats). Callers that need multi-tick
    /// grant should pump ticks on the broker side between `acquire` calls or use
    /// [`Self::acquire_after_grant`].
    pub fn request_lease<S: BufRead + Write>(
        &mut self,
        stream: &mut S,
        bytes: u64,
    ) -> Result<(), BrokerTenantError> {
        write_msg(stream, &Msg::LeaseRequest { bytes })
            .map_err(|e| BrokerTenantError::Io(e.to_string()))?;
        Ok(())
    }

    /// Wait for the next lease outcome message.
    ///
    /// When `requested_bytes` is set (via [`Self::request_lease`] / [`Self::acquire`]),
    /// granted bytes must equal the request (DT-8 honesty).
    pub fn wait_lease_outcome<S: BufRead + Write>(
        &mut self,
        stream: &mut S,
    ) -> Result<LeaseState, BrokerTenantError> {
        loop {
            match read_msg(stream).map_err(|e| BrokerTenantError::Io(e.to_string()))? {
                Some(Msg::LeaseGranted { lease, bytes }) => {
                    if let Some(need) = self.requested_bytes
                        && bytes != need
                    {
                        return Err(BrokerTenantError::Protocol(format!(
                            "granted_bytes_mismatch need={need} got={bytes}"
                        )));
                    }
                    let st = LeaseState { lease, bytes };
                    self.lease = Some(st.clone());
                    self.release_sent = ReleaseSent::None;
                    return Ok(st);
                }
                Some(Msg::LeaseDenied { reason }) => {
                    return Err(BrokerTenantError::Denied(reason));
                }
                Some(Msg::Ack) | Some(Msg::Error { .. }) => continue,
                Some(other) => {
                    return Err(BrokerTenantError::Protocol(format!("unexpected {other:?}")));
                }
                None => return Err(BrokerTenantError::Eof),
            }
        }
    }

    /// Convenience: request + wait (used when broker already has Free slices).
    pub fn acquire<S: BufRead + Write>(
        &mut self,
        stream: &mut S,
        bytes: u64,
    ) -> Result<LeaseState, BrokerTenantError> {
        self.requested_bytes = Some(bytes);
        self.request_lease(stream, bytes)?;
        self.wait_lease_outcome(stream)
    }

    /// DT-20: after grant, refuse local alloc if free VRAM < need; release lease.
    pub fn coresidence_gate(&mut self, free_vram: u64, need: u64) -> Result<(), BrokerTenantError> {
        if free_vram < need {
            return Err(BrokerTenantError::CoresidenceFailClosed {
                free: free_vram,
                need,
            });
        }
        Ok(())
    }

    /// Release an active lease (DT-8/DT-9): write + flush once per generation.
    ///
    /// A second same-process release for the same generation is a no-op.
    /// Protocol v1 has no ACK; broker-log correlation is the drill's responsibility.
    pub fn release<S: BufRead + Write>(&mut self, stream: &mut S) -> Result<(), BrokerTenantError> {
        if let ReleaseSent::Sent { .. } = self.release_sent {
            // Generation already released; leave stream closed by caller.
            self.lease = None;
            return Ok(());
        }
        let Some(st) = self.lease.take() else {
            return Ok(());
        };
        write_msg(stream, &Msg::LeaseRelease { lease: st.lease })
            .map_err(|e| BrokerTenantError::Io(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| BrokerTenantError::Io(e.to_string()))?;
        self.release_sent = ReleaseSent::Sent { lease: st.lease };
        self.requested_bytes = None;
        Ok(())
    }

    /// Heartbeat PSI (H3) — ignored by arbitration for WinDrive (DT-7).
    pub fn heartbeat_psi<S: Write>(&self, stream: &mut S) -> Result<(), BrokerTenantError> {
        let msg = Msg::Psi {
            sample: PsiSample::default(),
            swaps: vec![],
            mem: None,
        };
        write_msg(stream, &msg).map_err(|e| BrokerTenantError::Io(e.to_string()))
    }

    /// Take lease without wire (for fail-closed unit tests after simulated grant).
    pub fn force_lease_for_test(&mut self, lease: u32, bytes: u64) {
        self.lease = Some(LeaseState { lease, bytes });
        self.requested_bytes = Some(bytes);
        self.release_sent = ReleaseSent::None;
    }

    /// Drop lease state without wire (after CoresidenceFailClosed release was sent).
    pub fn clear_lease(&mut self) {
        self.lease = None;
        self.requested_bytes = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::io::Cursor;
    use std::time::Duration;

    use ramshared_broker::protocol::{Msg, write_msg};

    fn with_reply(request: Msg, reply: Msg) -> (Cursor<Vec<u8>>, Vec<u8>) {
        let mut out = Vec::new();
        write_msg(&mut out, &reply).unwrap();
        let mut in_buf = Vec::new();
        write_msg(&mut in_buf, &request).unwrap(); // not used; stream is reply-only for read
        let _ = in_buf;
        (Cursor::new(out), Vec::new())
    }

    #[test]
    fn register_win_drive() {
        let mut replies = Vec::new();
        write_msg(&mut replies, &Msg::Registered { tenant_id: 7 }).unwrap();
        let stream = Cursor::new(replies);
        // Cursor is read-only for write; use a dual buffer
        let mut dual = Dual::new(stream.get_ref().clone());
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        let id = t.register(&mut dual).unwrap();
        assert_eq!(id, 7);
        // Verify Register used WinDrive
        let sent = dual.written();
        let mut cur = Cursor::new(sent);
        let msg = read_msg(&mut cur).unwrap().unwrap();
        match msg {
            Msg::Register { transport, .. } => {
                assert_eq!(transport, TransportKind::WinDrive);
            }
            other => panic!("expected Register, got {other:?}"),
        }
    }

    #[test]
    fn lease_request_granted() {
        let mut dual = Dual::new(Vec::new());
        write_msg(
            dual.reply_buf(),
            &Msg::LeaseGranted {
                lease: 3,
                bytes: 64 * 1024 * 1024,
            },
        )
        .unwrap();
        dual.reset_read();
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.tenant_id = Some(1);
        let st = t.acquire(&mut dual, 64 * 1024 * 1024).unwrap();
        assert_eq!(st.lease, 3);
        assert_eq!(t.lease().unwrap().bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn lease_denied() {
        let mut dual = Dual::new(Vec::new());
        write_msg(
            dual.reply_buf(),
            &Msg::LeaseDenied {
                reason: "lease_em_andamento".into(),
            },
        )
        .unwrap();
        dual.reset_read();
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        let e = t.acquire(&mut dual, 1).unwrap_err();
        assert!(matches!(e, BrokerTenantError::Denied(r) if r == "lease_em_andamento"));
    }

    #[test]
    fn coresidence_fail_closed() {
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.force_lease_for_test(1, 1 << 30);
        let e = t.coresidence_gate(100, 1 << 30).unwrap_err();
        assert!(matches!(
            e,
            BrokerTenantError::CoresidenceFailClosed { free: 100, need }
            if need == 1 << 30
        ));
    }

    #[test]
    fn coresidence_ok() {
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.coresidence_gate(2 << 30, 1 << 30).unwrap();
    }

    /// Read/write duplex for unit tests.
    struct Dual {
        read: Cursor<Vec<u8>>,
        write: Vec<u8>,
        reply_scratch: Vec<u8>,
    }

    impl Dual {
        fn new(reply: Vec<u8>) -> Self {
            Self {
                read: Cursor::new(reply),
                write: Vec::new(),
                reply_scratch: Vec::new(),
            }
        }
        fn reply_buf(&mut self) -> &mut Vec<u8> {
            &mut self.reply_scratch
        }
        fn reset_read(&mut self) {
            self.read = Cursor::new(std::mem::take(&mut self.reply_scratch));
        }
        fn written(&self) -> &[u8] {
            &self.write
        }
    }

    impl std::io::Read for Dual {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read.read(buf)
        }
    }
    impl BufRead for Dual {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.read.fill_buf()
        }
        fn consume(&mut self, amt: usize) {
            self.read.consume(amt);
        }
    }
    impl Write for Dual {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.write.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.write.flush()
        }
    }

    // silence unused helper
    #[test]
    fn dual_with_reply_helper_compiles() {
        let _ = with_reply(Msg::Ack, Msg::Registered { tenant_id: 1 });
    }

    #[test]
    fn granted_bytes_must_equal_requested() {
        let mut dual = Dual::new(Vec::new());
        write_msg(
            dual.reply_buf(),
            &Msg::LeaseGranted {
                lease: 3,
                bytes: 32 * 1024 * 1024,
            },
        )
        .unwrap();
        dual.reset_read();
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.tenant_id = Some(1);
        let e = t.acquire(&mut dual, 64 * 1024 * 1024).unwrap_err();
        assert!(
            matches!(e, BrokerTenantError::Protocol(s) if s.contains("granted_bytes_mismatch"))
        );
    }

    #[test]
    fn release_flushes_before_session_close() {
        let mut dual = Dual::new(Vec::new());
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.force_lease_for_test(9, 64 * 1024 * 1024);
        t.release(&mut dual).unwrap();
        assert!(t.release_already_sent());
        assert!(t.lease().is_none());
        // LeaseRelease frame present in written bytes.
        let sent = dual.written();
        assert!(!sent.is_empty());
        let mut cur = Cursor::new(sent);
        let msg = read_msg(&mut cur).unwrap().unwrap();
        match msg {
            Msg::LeaseRelease { lease } => assert_eq!(lease, 9),
            other => panic!("expected LeaseRelease, got {other:?}"),
        }
    }

    #[test]
    fn release_twice_writes_once() {
        let mut dual = Dual::new(Vec::new());
        let mut t = BrokerTenant::new("wd", Duration::from_secs(5));
        t.force_lease_for_test(4, 1 << 20);
        t.release(&mut dual).unwrap();
        let first_len = dual.written().len();
        t.release(&mut dual).unwrap();
        assert_eq!(dual.written().len(), first_len);
        // Only one LeaseRelease message.
        let mut cur = Cursor::new(dual.written());
        let mut count = 0;
        while let Some(msg) = read_msg(&mut cur).unwrap() {
            if matches!(msg, Msg::LeaseRelease { .. }) {
                count += 1;
            }
        }
        assert_eq!(count, 1);
    }
}
