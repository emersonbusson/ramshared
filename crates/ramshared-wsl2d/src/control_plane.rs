//! Product control-plane composition (native-vsock ITEM-3, RF-3, RF-6).
//!
//! Scope note — read this before wiring anything to the I/O path:
//!
//! This module implements the **transport and lease-maintenance** half of
//! ITEM-3 only: the [`ControlPlane`] trait, [`VsockControlPlane`], and
//! [`HeartbeatLoop`]. It deliberately does **not** modify `run_nbd_with_startup`
//! and does **not** touch `ramshared_block::isolated_origin`.
//! `AuthoritativeOriginBackend` gates its VRAM cache on `CacheState` alone and
//! has no lease awareness; whether a host with no vsock peer may populate that
//! cache is an open SPEC question recorded as EVD-0174 Finding A. Neither
//! reading of that question is implemented here.
//!
//! For the same reason this trait has exactly **one** production
//! implementation, [`VsockControlPlane`]. A file-backed control plane is not
//! defined: `host_gate::lease_after_connect` states "there is no file
//! fallback", while the SPEC §MODIFY symbol list mentions "(vsock or file
//! fallback)". Inventing either one would pick a side of that contradiction.
//!
//! Fail-closed rules encoded here (Kahneman #15/#16, `.claude/rules/security.md`):
//!
//! - Every transport failure during handshake or heartbeat drives
//!   [`ControlPlaneAuthority::on_vsock_disconnect`], which revokes cache
//!   authority and preserves only the verified origin identity (RF-3).
//! - A zero or non-positive heartbeat/lease interval from the peer is refused
//!   before any state is accepted (arg smuggling).
//! - Frame payloads are length-checked against the caller's cap before
//!   allocation (`ramshared_ipc::read_frame`).
//! - The HMAC shared secret is supplied by the caller from an environment or
//!   secret-store source. Nothing in this module hardcodes a key.

use ramshared_ipc::{
    self as ipc, read_frame, write_frame, FrameError, Handshake, HandshakeAck, HandshakeError,
    HandshakeFinish, HandshakeTranscript, Heartbeat, HeartbeatAck, HostChallenge, LeaseDenied,
    LeaseGranted, LeaseRequest, MSG_HANDSHAKE, MSG_HANDSHAKE_ACK, MSG_HANDSHAKE_FINISH,
    MSG_HEARTBEAT, MSG_HEARTBEAT_ACK, MSG_LEASE_DENIED, MSG_LEASE_GRANTED, MSG_LEASE_REQUEST,
};

use crate::host_gate::{
    ControlPlaneAuthority, ControlPlaneState, GateError, LeaseToken, SealedOrigin,
};

/// Errors from control-plane transport and lease maintenance.
#[derive(Debug, PartialEq, Eq)]
pub enum ControlPlaneError {
    Frame(FrameError),
    Handshake(HandshakeError),
    Gate(GateError),
    /// The peer refused to mint a lease, or the lease has already lapsed.
    LeaseDenied(String),
    /// Heartbeat observed a lease that has no remaining lifetime.
    LeaseExpired,
    /// No transport is currently authorised to serve (disconnected / safe mode).
    Disconnected,
    /// The peer sent a protocol value this side refuses to honour.
    Protocol(String),
}

impl std::fmt::Display for ControlPlaneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame(e) => write!(f, "control-plane frame error: {e}"),
            Self::Handshake(e) => write!(f, "control-plane handshake error: {e}"),
            Self::Gate(e) => write!(f, "control-plane gate error: {e}"),
            Self::LeaseDenied(e) => write!(f, "lease denied: {e}"),
            Self::LeaseExpired => write!(f, "lease expired"),
            Self::Disconnected => write!(f, "control plane disconnected"),
            Self::Protocol(e) => write!(f, "control-plane protocol error: {e}"),
        }
    }
}

impl std::error::Error for ControlPlaneError {}

impl From<FrameError> for ControlPlaneError {
    fn from(value: FrameError) -> Self {
        Self::Frame(value)
    }
}

impl From<HandshakeError> for ControlPlaneError {
    fn from(value: HandshakeError) -> Self {
        Self::Handshake(value)
    }
}

impl From<GateError> for ControlPlaneError {
    fn from(value: GateError) -> Self {
        Self::Gate(value)
    }
}

/// The authority the I/O path consults before using cache or origin.
///
/// This is the seam that lets `AuthoritativeOriginBackend` stay unaware of
/// vsock while a future wiring asks a `dyn ControlPlane` whether the current
/// moment is authorised. Wiring that seam is **not** done here (EVD-0174
/// Finding A).
pub trait ControlPlane: Send {
    /// Current gate state, for telemetry and for callers that only read.
    fn state(&self) -> ControlPlaneState;

    /// Authorise a cache admission at `now_ms`.
    fn admit_cache(&self, now_ms: u64) -> Result<(), GateError>;

    /// Authorise authoritative-origin I/O. Lease state must not take this down
    /// (RF-3); only loss of the verified origin identity may.
    fn admit_origin_io(&self) -> Result<(), GateError>;

    /// One lease-maintenance beat. On failure the authority is already moved
    /// to its fail-closed state before this returns.
    fn heartbeat(&mut self, now_ms: u64) -> Result<(), ControlPlaneError>;
}

/// Live parameters the host negotiated in [`HandshakeAck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegotiatedCadence {
    pub heartbeat_secs: u64,
    pub lease_timeout_secs: u64,
}

/// Reject a peer-supplied cadence that would spin the loop or outrun the lease.
fn validate_cadence(c: NegotiatedCadence) -> Result<NegotiatedCadence, ControlPlaneError> {
    if c.heartbeat_secs == 0 {
        return Err(ControlPlaneError::Protocol(
            "peer negotiated a zero heartbeat interval".into(),
        ));
    }
    if c.lease_timeout_secs == 0 {
        return Err(ControlPlaneError::Protocol(
            "peer negotiated a zero lease timeout".into(),
        ));
    }
    if c.lease_timeout_secs <= c.heartbeat_secs {
        return Err(ControlPlaneError::Protocol(format!(
            "lease timeout {} does not outlive heartbeat {}",
            c.lease_timeout_secs, c.heartbeat_secs
        )));
    }
    Ok(c)
}

/// Native-vsock control plane: drives the DT-3 handshake and keeps the lease
/// alive over one connected stream.
pub struct VsockControlPlane<S: ipc::vsock::VsockStreamTrait> {
    stream: S,
    authority: ControlPlaneAuthority,
    /// HMAC shared secret from an env or secret-store source. Never a constant.
    secret: Vec<u8>,
    boot_id: String,
    distro_id: String,
    origin: SealedOrigin,
    cadence: Option<NegotiatedCadence>,
    lease_deadline_ms: u64,
    io_timeout: std::time::Duration,
}

impl<S: ipc::vsock::VsockStreamTrait> VsockControlPlane<S> {
    /// Build a disconnected control plane over an already-connected stream.
    ///
    /// `secret` is supplied by the caller; this constructor never invents one.
    /// `origin` is an already-verified sealed origin — this module does not
    /// establish origin identity, it only carries it. It is installed on the
    /// authority immediately so RF-3 holds from construction: origin I/O is
    /// available before any lease and survives lease loss and disconnect.
    pub fn new(
        stream: S,
        secret: Vec<u8>,
        boot_id: String,
        distro_id: String,
        origin: SealedOrigin,
        io_timeout: std::time::Duration,
    ) -> Self {
        let mut authority = ControlPlaneAuthority::disconnected();
        authority.adopt_verified_origin(origin.clone());
        Self {
            stream,
            authority,
            secret,
            boot_id,
            distro_id,
            origin,
            cadence: None,
            lease_deadline_ms: 0,
            io_timeout,
        }
    }

    pub fn authority(&self) -> &ControlPlaneAuthority {
        &self.authority
    }

    pub fn cadence(&self) -> Option<NegotiatedCadence> {
        self.cadence
    }

    pub fn lease_deadline_ms(&self) -> u64 {
        self.lease_deadline_ms
    }

    /// Drive the DT-3 handshake and accept the resulting lease.
    ///
    /// Order (SPEC ITEM-3 / `HandshakeTranscript`):
    /// `Handshake` → `HandshakeAck` (verified) → `HandshakeFinish` →
    /// `LeaseRequest` → `LeaseGranted`.
    ///
    /// Any failure before `accept_lease` succeeds leaves the authority in safe
    /// mode with no lease. A connection failure never grants a lease
    /// (`host_gate::lease_after_connect`).
    pub fn perform_handshake(&mut self, now_ms: u64) -> Result<(), ControlPlaneError> {
        self.authority.begin_handshake();

        let result = self.handshake_exchange(now_ms);
        if result.is_err() {
            // Fail closed: never leave the authority advertising Handshaking
            // after a failed attempt, and never leave a half-accepted lease.
            self.authority.on_vsock_disconnect();
            self.cadence = None;
            self.lease_deadline_ms = 0;
        }
        result
    }

    fn handshake_exchange(&mut self, now_ms: u64) -> Result<(), ControlPlaneError> {
        self.set_io_timeouts();

        let guest_nonce = ipc::random_nonce()?;
        let transcript = HandshakeTranscript {
            min_version: ipc::IPC_MIN_VERSION,
            max_version: ipc::IPC_VERSION_3,
            boot_id: self.boot_id.clone(),
            distro_id: self.distro_id.clone(),
            guest_nonce,
        };
        let opening_proof = ipc::guest_proof(&self.secret, &transcript)?;
        let opening = Handshake {
            min_version: ipc::IPC_MIN_VERSION,
            max_version: ipc::IPC_VERSION_3,
            boot_id: self.boot_id.clone(),
            distro_id: self.distro_id.clone(),
            guest_nonce,
            guest_proof: opening_proof,
        };
        let payload = ipc::encode_control(&opening)?;
        write_frame(&mut self.stream, MSG_HANDSHAKE, &payload, 1)?;

        let (msg_type, ack_payload, _correlation) =
            read_frame(&mut self.stream, ipc::MAX_CONTROL_PAYLOAD)?;
        if msg_type != MSG_HANDSHAKE_ACK {
            return Err(ControlPlaneError::Protocol(format!(
                "expected handshake ack ({}), received {msg_type}",
                MSG_HANDSHAKE_ACK
            )));
        }
        let ack: HandshakeAck = ipc::decode_control(&ack_payload)?;

        let negotiated = ipc::negotiate_version(
            ipc::IPC_MIN_VERSION,
            ack.accepted_version.max(ipc::IPC_MIN_VERSION),
        )
        .ok_or_else(|| {
            ControlPlaneError::Protocol(format!(
                "no mutually supported version (peer accepted {})",
                ack.accepted_version
            ))
        })?;
        if negotiated != ack.accepted_version {
            return Err(ControlPlaneError::Protocol(format!(
                "peer accepted version {} which is outside the negotiated range",
                ack.accepted_version
            )));
        }

        let cadence = validate_cadence(NegotiatedCadence {
            heartbeat_secs: ack.heartbeat_secs,
            lease_timeout_secs: ack.lease_timeout_secs,
        })?;

        let challenge = HostChallenge {
            accepted_version: ack.accepted_version,
            heartbeat_secs: ack.heartbeat_secs,
            lease_timeout_secs: ack.lease_timeout_secs,
            host_nonce: ack.host_nonce,
        };
        if !ipc::verify_host_proof(&self.secret, &transcript, &challenge, &ack.host_proof) {
            return Err(ControlPlaneError::Handshake(HandshakeError::ProofMismatch));
        }

        let finish_proof = ipc::guest_finish_proof(&self.secret, &transcript, &challenge)?;
        let finish = HandshakeFinish {
            guest_finish_proof: finish_proof,
        };
        let payload = ipc::encode_control(&finish)?;
        write_frame(&mut self.stream, MSG_HANDSHAKE_FINISH, &payload, 2)?;

        let request = LeaseRequest {
            nonce: guest_nonce.to_vec(),
        };
        let payload = ipc::encode_control(&request)?;
        write_frame(&mut self.stream, MSG_LEASE_REQUEST, &payload, 3)?;

        let (msg_type, lease_payload, _correlation) =
            read_frame(&mut self.stream, ipc::MAX_CONTROL_PAYLOAD)?;
        let lease = match msg_type {
            MSG_LEASE_GRANTED => {
                let granted: LeaseGranted = ipc::decode_control(&lease_payload)?;
                LeaseToken {
                    lease_id: granted.lease_id,
                    deadline_ms: granted.deadline_ms,
                }
            }
            MSG_LEASE_DENIED => {
                let denied: LeaseDenied = ipc::decode_control(&lease_payload)?;
                return Err(ControlPlaneError::LeaseDenied(denied.reason));
            }
            other => {
                return Err(ControlPlaneError::Protocol(format!(
                    "expected lease granted ({}) or lease denied ({}), received {other}",
                    MSG_LEASE_GRANTED, MSG_LEASE_DENIED
                )));
            }
        };

        if lease.deadline_ms < now_ms {
            return Err(ControlPlaneError::LeaseDenied(
                "peer granted an already-expired lease".into(),
            ));
        }

        self.authority
            .accept_lease(lease.clone(), self.origin.clone(), now_ms)?;
        self.cadence = Some(cadence);
        self.lease_deadline_ms = lease.deadline_ms;
        Ok(())
    }

    fn set_io_timeouts(&self) {
        // Timeouts are best-effort: a transport that cannot be bounded is still
        // bounded by the frame length checks, and every exchange is a single
        // request/response pair rather than an unbounded stream.
        let _ = self.stream.set_read_timeout(Some(self.io_timeout));
        let _ = self.stream.set_write_timeout(Some(self.io_timeout));
    }
}

impl<S: ipc::vsock::VsockStreamTrait> ControlPlane for VsockControlPlane<S> {
    fn state(&self) -> ControlPlaneState {
        self.authority.state()
    }

    fn admit_cache(&self, now_ms: u64) -> Result<(), GateError> {
        self.authority.admit_cache(now_ms)
    }

    fn admit_origin_io(&self) -> Result<(), GateError> {
        self.authority.admit_origin_io()
    }

    fn heartbeat(&mut self, now_ms: u64) -> Result<(), ControlPlaneError> {
        if self.authority.state() != ControlPlaneState::VsockLeased {
            return Err(ControlPlaneError::Disconnected);
        }
        self.set_io_timeouts();

        let beat = Heartbeat {
            timestamp_ms: now_ms,
        };
        let payload = ipc::encode_control(&beat)?;
        match self.exchange_heartbeat(&payload, now_ms) {
            Ok(()) => Ok(()),
            // A clean lease expiry is not a transport failure. RF-3: lease
            // loss revokes cache authority and leaves the verified origin
            // path up, so this must stay at `OriginOnly` and must not be
            // downgraded to safe mode by the disconnect handler below.
            Err(ControlPlaneError::LeaseExpired) => {
                self.cadence = None;
                self.lease_deadline_ms = 0;
                Err(ControlPlaneError::LeaseExpired)
            }
            Err(error) => {
                // Fail closed on every transport failure. A beat that cannot be
                // completed is not evidence the lease is alive.
                self.authority.on_vsock_disconnect();
                self.cadence = None;
                self.lease_deadline_ms = 0;
                Err(error)
            }
        }
    }
}

impl<S: ipc::vsock::VsockStreamTrait> VsockControlPlane<S> {
    fn exchange_heartbeat(
        &mut self,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<(), ControlPlaneError> {
        write_frame(&mut self.stream, MSG_HEARTBEAT, payload, 4)?;
        let (msg_type, ack_payload, _correlation) =
            read_frame(&mut self.stream, ipc::MAX_CONTROL_PAYLOAD)?;
        if msg_type != MSG_HEARTBEAT_ACK {
            return Err(ControlPlaneError::Protocol(format!(
                "expected heartbeat ack ({}), received {msg_type}",
                MSG_HEARTBEAT_ACK
            )));
        }
        let ack: HeartbeatAck = ipc::decode_control(&ack_payload)?;
        if ack.lease_remaining_ms == 0 {
            self.authority.on_lease_expired();
            self.cadence = None;
            self.lease_deadline_ms = 0;
            return Err(ControlPlaneError::LeaseExpired);
        }
        self.lease_deadline_ms = now_ms.saturating_add(ack.lease_remaining_ms);
        Ok(())
    }
}

/// Bounded lease-maintenance campaign.
///
/// Never spins forever: it stops after `ticks` beats or after
/// `max_consecutive_failures` consecutive failures (Kahneman #16 — the safe
/// default is to stop and leave the authority fail-closed, not to hammer a
/// dead peer). Time and sleep are injected so the loop is deterministic under
/// test and can be driven from a real clock in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatLoop {
    pub interval_ms: u64,
    pub max_consecutive_failures: u32,
}

/// Why a [`HeartbeatLoop`] campaign stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum HeartbeatLoopStop {
    /// The requested number of beats was attempted and every one succeeded.
    TicksExhausted { ticks: u32 },
    /// The consecutive-failure budget ran out. The plane is already fail-closed.
    Failure {
        ticks: u32,
        consecutive_failures: u32,
        error: ControlPlaneError,
    },
}

impl HeartbeatLoop {
    pub fn new(interval_ms: u64, max_consecutive_failures: u32) -> Result<Self, ControlPlaneError> {
        if interval_ms == 0 {
            return Err(ControlPlaneError::Protocol(
                "heartbeat interval must be non-zero".into(),
            ));
        }
        if max_consecutive_failures == 0 {
            return Err(ControlPlaneError::Protocol(
                "heartbeat failure budget must be non-zero".into(),
            ));
        }
        Ok(Self {
            interval_ms,
            max_consecutive_failures,
        })
    }

    /// Run at most `ticks` beats. `now_ms` supplies the observation time for
    /// each beat; `sleep_ms` receives the configured interval between beats
    /// (not after the last one).
    pub fn run(
        &self,
        plane: &mut dyn ControlPlane,
        ticks: u32,
        mut now_ms: impl FnMut() -> u64,
        mut sleep_ms: impl FnMut(u64),
    ) -> HeartbeatLoopStop {
        let mut consecutive_failures: u32 = 0;
        for tick in 0..ticks {
            match plane.heartbeat(now_ms()) {
                Ok(()) => consecutive_failures = 0,
                Err(error) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    if consecutive_failures >= self.max_consecutive_failures {
                        return HeartbeatLoopStop::Failure {
                            ticks: tick + 1,
                            consecutive_failures,
                            error,
                        };
                    }
                }
            }
            if tick + 1 < ticks {
                sleep_ms(self.interval_ms);
            }
        }
        HeartbeatLoopStop::TicksExhausted { ticks }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use ramshared_ipc::vsock::{MockVsockStream, VsockStreamTrait};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    /// Test-only shared secret. Not a product key and never a default.
    const SECRET: &[u8] = b"unit-test-shared-secret-not-a-product-key";
    const IO_TIMEOUT: Duration = Duration::from_secs(5);

    fn sealed_origin() -> SealedOrigin {
        SealedOrigin {
            logical_capacity_mib: 4096,
            partuuid: "11111111-2222-3333-4444-555555555555".into(),
            origin_vhdx: "C:\\ramshared\\origin.vhdx".into(),
        }
    }

    fn plane_over<S: VsockStreamTrait>(stream: S) -> VsockControlPlane<S> {
        VsockControlPlane::new(
            stream,
            SECRET.to_vec(),
            "boot-test".into(),
            "distro-test".into(),
            sealed_origin(),
            IO_TIMEOUT,
        )
    }

    // --- Scripted stream (no peer thread) ---------------------------------

    /// Reads come from a prepared buffer; writes are captured. Used for the
    /// paths that must fail closed without needing a real host proof.
    struct ScriptedStream {
        to_guest: std::io::Cursor<Vec<u8>>,
        from_guest: Vec<u8>,
        fail_write: bool,
        fail_read: bool,
    }

    impl ScriptedStream {
        fn new(to_guest: Vec<u8>) -> Self {
            Self {
                to_guest: std::io::Cursor::new(to_guest),
                from_guest: Vec::new(),
                fail_write: false,
                fail_read: false,
            }
        }
    }

    impl Read for ScriptedStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.fail_read {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "scripted read failure",
                ));
            }
            self.to_guest.read(buf)
        }
    }

    impl Write for ScriptedStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.fail_write {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "scripted write failure",
                ));
            }
            self.from_guest.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl VsockStreamTrait for ScriptedStream {
        fn set_read_timeout(&self, _timeout: Option<Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn set_write_timeout(&self, _timeout: Option<Duration>) -> std::io::Result<()> {
            Ok(())
        }
        fn shutdown_both(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn frame_bytes(msg_type: u8, payload: &[u8], correlation_id: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        write_frame(&mut buf, msg_type, payload, correlation_id).expect("frame");
        buf
    }

    /// A structurally valid `HandshakeAck` whose proof is all-zero. Valid
    /// enough to reach proof verification; never a valid proof (the guest
    /// nonce is 32 fresh random bytes).
    fn bogus_handshake_ack(heartbeat_secs: u64, lease_timeout_secs: u64) -> Vec<u8> {
        let ack = HandshakeAck {
            accepted_version: ipc::IPC_VERSION_3,
            heartbeat_secs,
            lease_timeout_secs,
            host_nonce: [7u8; 32],
            host_proof: [0u8; 32],
        };
        let payload = ipc::encode_control(&ack).expect("encode ack");
        frame_bytes(MSG_HANDSHAKE_ACK, &payload, 1)
    }

    // --- Handshake: fail-closed paths ------------------------------------

    #[test]
    fn a_zero_heartbeat_interval_from_the_peer_is_refused_before_any_authority() {
        let stream = ScriptedStream::new(bogus_handshake_ack(0, 30));
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Protocol(ref m) if m.contains("zero heartbeat")),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
        assert!(matches!(
            plane.admit_cache(1_000),
            Err(GateError::LeaseDenied(_))
        ));
    }

    #[test]
    fn a_zero_lease_timeout_from_the_peer_is_refused() {
        let stream = ScriptedStream::new(bogus_handshake_ack(5, 0));
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Protocol(ref m) if m.contains("zero lease")),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
    }

    #[test]
    fn a_lease_timeout_that_does_not_outlive_the_heartbeat_is_refused() {
        let stream = ScriptedStream::new(bogus_handshake_ack(10, 10));
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Protocol(ref m) if m.contains("does not outlive")),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
    }

    #[test]
    fn an_invalid_host_proof_never_grants_a_lease() {
        // Cadence is acceptable; only the proof is wrong.
        let stream = ScriptedStream::new(bogus_handshake_ack(5, 30));
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Handshake(HandshakeError::ProofMismatch)),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
        assert!(matches!(
            plane.admit_cache(1_000),
            Err(GateError::LeaseDenied(_))
        ));
        // Origin I/O is preserved: losing the lease must not take the
        // authoritative path down (RF-3).
        assert!(plane.admit_origin_io().is_ok());
    }

    #[test]
    fn an_unexpected_message_type_mid_handshake_is_refused() {
        let heartbeat = ipc::encode_control(&Heartbeat { timestamp_ms: 1 }).expect("encode");
        let stream = ScriptedStream::new(frame_bytes(MSG_HEARTBEAT, &heartbeat, 1));
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Protocol(ref m) if m.contains("expected handshake ack")),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
    }

    #[test]
    fn a_truncated_stream_during_handshake_fails_closed() {
        let stream = ScriptedStream::new(vec![0x52, 0x41, 0x4D]);
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Frame(FrameError::TruncatedFrame { .. })),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
    }

    #[test]
    fn a_write_failure_during_handshake_fails_closed() {
        let mut stream = ScriptedStream::new(Vec::new());
        stream.fail_write = true;
        let mut plane = plane_over(stream);
        let err = plane.perform_handshake(1_000).expect_err("must refuse");
        assert!(
            matches!(err, ControlPlaneError::Frame(FrameError::StreamIo(_))),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
    }

    #[test]
    fn heartbeat_before_a_lease_is_refused_without_touching_the_stream() {
        let stream = ScriptedStream::new(Vec::new());
        let mut plane = plane_over(stream);
        assert_eq!(
            plane.heartbeat(1_000),
            Err(ControlPlaneError::Disconnected)
        );
        assert!(plane.stream.from_guest.is_empty());
    }

    // --- HeartbeatLoop with a fake plane ---------------------------------

    struct FakePlane {
        beats: Vec<u64>,
        fail_on: Vec<usize>,
        state: ControlPlaneState,
    }

    impl FakePlane {
        fn new() -> Self {
            Self {
                beats: Vec::new(),
                fail_on: Vec::new(),
                state: ControlPlaneState::VsockLeased,
            }
        }

        fn failing_on(mut self, indices: &[usize]) -> Self {
            self.fail_on = indices.to_vec();
            self
        }
    }

    impl ControlPlane for FakePlane {
        fn state(&self) -> ControlPlaneState {
            self.state
        }
        fn admit_cache(&self, _now_ms: u64) -> Result<(), GateError> {
            Ok(())
        }
        fn admit_origin_io(&self) -> Result<(), GateError> {
            Ok(())
        }
        fn heartbeat(&mut self, now_ms: u64) -> Result<(), ControlPlaneError> {
            let index = self.beats.len();
            self.beats.push(now_ms);
            if self.fail_on.contains(&index) {
                self.state = ControlPlaneState::SafeMode;
                return Err(ControlPlaneError::Disconnected);
            }
            Ok(())
        }
    }

    #[test]
    fn heartbeat_loop_rejects_a_zero_interval_or_zero_failure_budget() {
        assert!(matches!(
            HeartbeatLoop::new(0, 3),
            Err(ControlPlaneError::Protocol(_))
        ));
        assert!(matches!(
            HeartbeatLoop::new(100, 0),
            Err(ControlPlaneError::Protocol(_))
        ));
    }

    #[test]
    fn heartbeat_loop_runs_every_requested_tick_when_all_beats_succeed() {
        let loop_ctl = HeartbeatLoop::new(50, 3).expect("valid loop");
        let mut plane = FakePlane::new();
        let mut slept = Vec::new();
        let stop = loop_ctl.run(
            &mut plane,
            4,
            || 1_000,
            |ms| slept.push(ms),
        );
        assert_eq!(stop, HeartbeatLoopStop::TicksExhausted { ticks: 4 });
        assert_eq!(plane.beats, vec![1_000, 1_000, 1_000, 1_000]);
        assert_eq!(slept, vec![50, 50, 50], "no sleep after the final beat");
    }

    #[test]
    fn heartbeat_loop_stops_at_the_consecutive_failure_budget() {
        let loop_ctl = HeartbeatLoop::new(50, 2).expect("valid loop");
        // Beats 0 and 1 fail; the budget is 2, so the loop must stop at tick 2
        // and never reach the remaining two.
        let mut plane = FakePlane::new().failing_on(&[0, 1]);
        let stop = loop_ctl.run(&mut plane, 4, || 0, |_| {});
        assert_eq!(
            stop,
            HeartbeatLoopStop::Failure {
                ticks: 2,
                consecutive_failures: 2,
                error: ControlPlaneError::Disconnected,
            }
        );
        assert_eq!(plane.beats.len(), 2);
    }

    #[test]
    fn heartbeat_loop_treats_an_isolated_failure_as_transient_and_continues() {
        let loop_ctl = HeartbeatLoop::new(50, 3).expect("valid loop");
        // Only beat 1 fails; the budget is 3, so the loop recovers and finishes.
        let mut plane = FakePlane::new().failing_on(&[1]);
        let stop = loop_ctl.run(&mut plane, 4, || 0, |_| {});
        assert_eq!(stop, HeartbeatLoopStop::TicksExhausted { ticks: 4 });
        assert_eq!(plane.beats.len(), 4);
    }

    #[test]
    fn heartbeat_loop_of_zero_ticks_is_a_no_op() {
        let loop_ctl = HeartbeatLoop::new(50, 3).expect("valid loop");
        let mut plane = FakePlane::new();
        let stop = loop_ctl.run(&mut plane, 0, || 0, |_| panic!("must not sleep"));
        assert_eq!(stop, HeartbeatLoopStop::TicksExhausted { ticks: 0 });
        assert!(plane.beats.is_empty());
    }

    // --- End-to-end over a real socket pair ------------------------------

    /// Drive the host side of DT-3 over `host`, then service `beats`
    /// heartbeats. `close_after_beats` drops the socket instead of acking the
    /// next beat, which is the disconnect the guest must fail closed on.
    fn spawn_host(
        host: UnixStream,
        heartbeat_secs: u64,
        lease_timeout_secs: u64,
        beats: Vec<HeartbeatAck>,
        close_instead_of_ack: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut s = host;

            let (msg_type, payload, _) = read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("read handshake");
            assert_eq!(msg_type, MSG_HANDSHAKE);
            let opening: Handshake = ipc::decode_control(&payload).expect("decode handshake");
            let transcript = HandshakeTranscript {
                min_version: opening.min_version,
                max_version: opening.max_version,
                boot_id: opening.boot_id.clone(),
                distro_id: opening.distro_id.clone(),
                guest_nonce: opening.guest_nonce,
            };
            assert!(
                ipc::verify_guest_proof(SECRET, &transcript, &opening.guest_proof),
                "host must be able to verify the guest opening proof"
            );

            let host_nonce = [0x2Cu8; 32];
            let challenge = HostChallenge {
                accepted_version: ipc::IPC_VERSION_3,
                heartbeat_secs,
                lease_timeout_secs,
                host_nonce,
            };
            let ack = HandshakeAck {
                accepted_version: ipc::IPC_VERSION_3,
                heartbeat_secs,
                lease_timeout_secs,
                host_nonce,
                host_proof: ipc::host_proof(SECRET, &transcript, &challenge).expect("host proof"),
            };
            let payload = ipc::encode_control(&ack).expect("encode ack");
            write_frame(&mut s, MSG_HANDSHAKE_ACK, &payload, 1).expect("write ack");

            let (msg_type, payload, _) =
                read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("read finish");
            assert_eq!(msg_type, MSG_HANDSHAKE_FINISH);
            let finish: HandshakeFinish = ipc::decode_control(&payload).expect("decode finish");
            assert!(
                ipc::verify_guest_finish_proof(SECRET, &transcript, &challenge, &finish.guest_finish_proof),
                "host must be able to verify the guest finish proof"
            );

            let (msg_type, payload, _) =
                read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("read lease request");
            assert_eq!(msg_type, MSG_LEASE_REQUEST);
            let request: LeaseRequest = ipc::decode_control(&payload).expect("decode lease request");
            assert_eq!(request.nonce, opening.guest_nonce.to_vec());

            let granted = LeaseGranted {
                lease_id: 4242,
                deadline_ms: 60_000,
            };
            let payload = ipc::encode_control(&granted).expect("encode granted");
            write_frame(&mut s, MSG_LEASE_GRANTED, &payload, 3).expect("write granted");

            for (index, beat_ack) in beats.iter().enumerate() {
                if close_instead_of_ack && index + 1 == beats.len() {
                    drop(s);
                    return;
                }
                let (msg_type, payload, _) =
                    read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("read beat");
                assert_eq!(msg_type, MSG_HEARTBEAT, "beat {index}");
                let beat: Heartbeat = ipc::decode_control(&payload).expect("decode beat");
                assert!(beat.timestamp_ms > 0);
                let payload = ipc::encode_control(beat_ack).expect("encode beat ack");
                write_frame(&mut s, MSG_HEARTBEAT_ACK, &payload, 4).expect("write beat ack");
            }
        })
    }

    #[test]
    fn a_full_handshake_grants_a_lease_that_admits_cache() {
        let (guest, host) = UnixStream::pair().expect("pair");
        let host_thread = spawn_host(
            host,
            5,
            30,
            vec![HeartbeatAck {
                timestamp_ms: 0,
                lease_remaining_ms: 25_000,
            }],
            false,
        );

        let mut plane = plane_over(MockVsockStream(guest));
        plane.perform_handshake(1_000).expect("handshake");
        assert_eq!(plane.state(), ControlPlaneState::VsockLeased);
        assert_eq!(
            plane.cadence(),
            Some(NegotiatedCadence {
                heartbeat_secs: 5,
                lease_timeout_secs: 30
            })
        );
        assert_eq!(plane.lease_deadline_ms(), 60_000);
        assert!(plane.admit_cache(1_000).is_ok());
        assert!(plane.admit_origin_io().is_ok());

        plane.heartbeat(2_000).expect("beat");
        assert_eq!(
            plane.lease_deadline_ms(),
            25_000 + 2_000,
            "a beat renews the deadline from the ack"
        );

        host_thread.join().expect("host thread");
    }

    #[test]
    fn a_beat_that_reports_no_remaining_lease_moves_the_plane_to_origin_only() {
        let (guest, host) = UnixStream::pair().expect("pair");
        let host_thread = spawn_host(
            host,
            5,
            30,
            vec![HeartbeatAck {
                timestamp_ms: 0,
                lease_remaining_ms: 0,
            }],
            false,
        );

        let mut plane = plane_over(MockVsockStream(guest));
        plane.perform_handshake(1_000).expect("handshake");
        let err = plane.heartbeat(2_000).expect_err("lease must expire");
        assert_eq!(err, ControlPlaneError::LeaseExpired);
        assert_eq!(plane.state(), ControlPlaneState::OriginOnly);
        assert!(matches!(
            plane.admit_cache(2_000),
            Err(GateError::LeaseDenied(_))
        ));
        assert!(plane.admit_origin_io().is_ok());

        host_thread.join().expect("host thread");
    }

    #[test]
    fn a_transport_failure_on_a_beat_fails_closed_to_safe_mode() {
        let (guest, host) = UnixStream::pair().expect("pair");
        // One beat is expected; the host closes instead of acking it.
        let host_thread = spawn_host(
            host,
            5,
            30,
            vec![HeartbeatAck {
                timestamp_ms: 0,
                lease_remaining_ms: 25_000,
            }],
            true,
        );

        let mut plane = plane_over(MockVsockStream(guest));
        plane.perform_handshake(1_000).expect("handshake");
        assert_eq!(plane.state(), ControlPlaneState::VsockLeased);

        let err = plane.heartbeat(2_000).expect_err("must fail closed");
        assert!(
            matches!(
                err,
                ControlPlaneError::Frame(FrameError::TruncatedFrame { .. })
                    | ControlPlaneError::Frame(FrameError::StreamIo(_))
            ),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
        assert!(matches!(
            plane.admit_cache(2_000),
            Err(GateError::LeaseDenied(_))
        ));
        assert!(
            plane.admit_origin_io().is_ok(),
            "RF-3: origin I/O survives lease loss"
        );

        host_thread.join().expect("host thread");
    }

    #[test]
    fn a_host_thread_can_reject_the_lease_and_the_guest_keeps_no_authority() {
        let (guest, host) = UnixStream::pair().expect("pair");
        let host_thread = std::thread::spawn(move || {
            let mut s = host;
            let (_msg_type, payload, _) =
                read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("read handshake");
            let opening: Handshake = ipc::decode_control(&payload).expect("decode handshake");
            let transcript = HandshakeTranscript {
                min_version: opening.min_version,
                max_version: opening.max_version,
                boot_id: opening.boot_id.clone(),
                distro_id: opening.distro_id.clone(),
                guest_nonce: opening.guest_nonce,
            };
            let host_nonce = [0x2Cu8; 32];
            let challenge = HostChallenge {
                accepted_version: ipc::IPC_VERSION_3,
                heartbeat_secs: 5,
                lease_timeout_secs: 30,
                host_nonce,
            };
            let ack = HandshakeAck {
                accepted_version: ipc::IPC_VERSION_3,
                heartbeat_secs: 5,
                lease_timeout_secs: 30,
                host_nonce,
                host_proof: ipc::host_proof(SECRET, &transcript, &challenge).expect("host proof"),
            };
            let payload = ipc::encode_control(&ack).expect("encode ack");
            write_frame(&mut s, MSG_HANDSHAKE_ACK, &payload, 1).expect("write ack");

            // Drain the finish + request, then deny.
            let _ = read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("finish");
            let _ = read_frame(&mut s, ipc::MAX_CONTROL_PAYLOAD).expect("request");
            let denied = LeaseDenied {
                reason: "guardian proof stale".into(),
            };
            let payload = ipc::encode_control(&denied).expect("encode denied");
            write_frame(&mut s, MSG_LEASE_DENIED, &payload, 3).expect("write denied");
        });

        let mut plane = plane_over(MockVsockStream(guest));
        let err = plane.perform_handshake(1_000).expect_err("must be denied");
        assert!(
            matches!(err, ControlPlaneError::LeaseDenied(ref r) if r.contains("guardian")),
            "unexpected error: {err:?}"
        );
        assert_eq!(plane.state(), ControlPlaneState::SafeMode);
        assert!(matches!(
            plane.admit_cache(1_000),
            Err(GateError::LeaseDenied(_))
        ));

        host_thread.join().expect("host thread");
    }
}
