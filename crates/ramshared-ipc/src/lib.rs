//! Shared host-guest IPC protocol for RamShared vsock control plane.
//!
//! Binary framed protocol with magic `0x52414D53` ("RAMS"), versioned headers,
//! bounded payloads, and HMAC-authenticated handshake.
//!
//! SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md

pub mod vsock;

use serde::{Deserialize, Serialize};

pub const IPC_MAGIC: u32 = 0x52414D53;
pub const IPC_VERSION_3: u32 = 3;
pub const IPC_MIN_VERSION: u32 = 2;
pub const MAX_PAYLOAD_LEN: u32 = 1024 * 1024;
pub const MAX_CONTROL_PAYLOAD: u32 = 4 * 1024;
pub const MAX_MANIFEST_PAYLOAD: u32 = 64 * 1024;
pub const FRAME_HEADER_LEN: usize = 24;

pub const MSG_HANDSHAKE: u8 = 1;
pub const MSG_HANDSHAKE_ACK: u8 = 2;
pub const MSG_HEARTBEAT: u8 = 3;
pub const MSG_HEARTBEAT_ACK: u8 = 4;
pub const MSG_LEASE_REQUEST: u8 = 5;
pub const MSG_LEASE_GRANTED: u8 = 6;
pub const MSG_LEASE_DENIED: u8 = 7;
pub const MSG_LEASE_RELEASE: u8 = 8;
pub const MSG_ORIGIN_MANIFEST: u8 = 9;
pub const MSG_ORIGIN_MANIFEST_ACK: u8 = 10;
pub const MSG_SAFE_MODE_GATE: u8 = 11;
pub const MSG_SAFE_MODE_GATE_ACK: u8 = 12;
pub const MSG_GUARDIAN_HEALTH: u8 = 13;
pub const MSG_GUARDIAN_HEALTH_ACK: u8 = 14;
pub const MSG_VHDX_ATTACH: u8 = 15;
pub const MSG_VHDX_ATTACH_ACK: u8 = 16;
pub const MSG_VHDX_DETACH: u8 = 17;
pub const MSG_VHDX_DETACH_ACK: u8 = 18;
pub const MSG_TELEMETRY: u8 = 19;
pub const MSG_SHUTDOWN: u8 = 20;
pub const MSG_SHUTDOWN_ACK: u8 = 21;
pub const MSG_HANDSHAKE_FINISH: u8 = 22;
pub const MSG_MAX: u8 = MSG_HANDSHAKE_FINISH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsockFrameHeader {
    pub magic: u32,
    pub version: u32,
    pub payload_len: u32,
    pub flags: u32,
    pub correlation_id: u64,
}

impl VsockFrameHeader {
    pub fn new(msg_type: u8, payload_len: u32, correlation_id: u64) -> Self {
        Self {
            magic: IPC_MAGIC,
            version: IPC_VERSION_3,
            payload_len,
            flags: msg_type as u32,
            correlation_id,
        }
    }

    pub fn msg_type(&self) -> u8 {
        (self.flags & 0xFF) as u8
    }

    pub fn encode(&self) -> [u8; FRAME_HEADER_LEN] {
        let mut buf = [0u8; FRAME_HEADER_LEN];
        buf[0..4].copy_from_slice(&self.magic.to_le_bytes());
        buf[4..8].copy_from_slice(&self.version.to_le_bytes());
        buf[8..12].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[12..16].copy_from_slice(&self.flags.to_le_bytes());
        buf[16..24].copy_from_slice(&self.correlation_id.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8; FRAME_HEADER_LEN]) -> Result<Self, FrameError> {
        let mut b4 = [0u8; 4];
        b4.copy_from_slice(&buf[0..4]);
        let magic = u32::from_le_bytes(b4);
        if magic != IPC_MAGIC {
            return Err(FrameError::InvalidMagic(magic));
        }
        b4.copy_from_slice(&buf[4..8]);
        let version = u32::from_le_bytes(b4);
        if !(IPC_MIN_VERSION..=IPC_VERSION_3).contains(&version) {
            return Err(FrameError::UnsupportedVersion(version));
        }
        b4.copy_from_slice(&buf[8..12]);
        let payload_len = u32::from_le_bytes(b4);
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(FrameError::PayloadTooLarge(payload_len));
        }
        b4.copy_from_slice(&buf[12..16]);
        let flags = u32::from_le_bytes(b4);
        let msg_type = (flags & 0xFF) as u8;
        if msg_type == 0 || msg_type > MSG_MAX {
            return Err(FrameError::UnknownMessageType(msg_type));
        }
        let mut b8 = [0u8; 8];
        b8.copy_from_slice(&buf[16..24]);
        let correlation_id = u64::from_le_bytes(b8);
        Ok(Self {
            magic,
            version,
            payload_len,
            flags,
            correlation_id,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    InvalidMagic(u32),
    UnsupportedVersion(u32),
    PayloadTooLarge(u32),
    UnknownMessageType(u8),
    PayloadDeserialization(String),
    PayloadExceedsCap(u32),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic(m) => write!(f, "invalid magic: {m:#010X}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {v}"),
            Self::PayloadTooLarge(l) => write!(f, "payload too large: {l} bytes"),
            Self::UnknownMessageType(t) => write!(f, "unknown message type: {t}"),
            Self::PayloadDeserialization(e) => write!(f, "payload deserialization failed: {e}"),
            Self::PayloadExceedsCap(l) => write!(f, "control payload exceeds 4KB cap: {l} bytes"),
        }
    }
}

impl std::error::Error for FrameError {}

// --- Message payload types ---

/// Guest `Handshake` (message type 1).
///
/// `boot_id` and `distro_id` are claims/metadata only — they are not
/// authenticated identity by themselves. Identity comes from the
/// role-separated HMAC transcript (DT-3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handshake {
    pub min_version: u32,
    pub max_version: u32,
    pub boot_id: String,
    pub distro_id: String,
    pub guest_nonce: [u8; 32],
    pub guest_proof: [u8; 32],
}

/// Host `HandshakeAck` (message type 2).
///
/// Carries the host challenge: a fresh nonce plus a host proof over both
/// nonces and the complete transcript. The guest accepts authority only
/// after this proof validates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeAck {
    pub accepted_version: u32,
    pub heartbeat_secs: u64,
    pub lease_timeout_secs: u64,
    pub host_nonce: [u8; 32],
    pub host_proof: [u8; 32],
}

/// Guest `HandshakeFinish` (message type 22).
///
/// Proves the same transcript back to the host. No manifest or lease is
/// sent until the host validates this proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeFinish {
    pub guest_finish_proof: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heartbeat {
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatAck {
    pub timestamp_ms: u64,
    pub lease_remaining_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseRequest {
    pub nonce: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseGranted {
    pub lease_id: u32,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseDenied {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseRelease {
    pub lease_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginManifestPayload {
    pub sha256_hex: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OriginManifestAck {
    pub sealed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafeModeGate {
    pub boot_id: String,
    pub safe_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafeModeGateAck {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianHealth {
    pub timestamp_ms: u64,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianHealthAck {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VhdxAttachRequest {
    pub path: String,
    pub partuuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VhdxAttachAck {
    pub attached: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VhdxDetachRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VhdxDetachAck {
    pub detached: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Telemetry {
    pub control_plane_state: String,
    pub heartbeat_rtt_us: u64,
    pub lease_remaining_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Shutdown {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShutdownAck {
    pub acknowledged: bool,
}

// --- Serialization helpers ---

pub fn encode_control<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let json =
        serde_json::to_vec(msg).map_err(|e| FrameError::PayloadDeserialization(e.to_string()))?;
    if json.len() as u32 > MAX_CONTROL_PAYLOAD {
        return Err(FrameError::PayloadExceedsCap(json.len() as u32));
    }
    Ok(json)
}

pub fn decode_control<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, FrameError> {
    if payload.len() as u32 > MAX_CONTROL_PAYLOAD {
        return Err(FrameError::PayloadExceedsCap(payload.len() as u32));
    }
    serde_json::from_slice(payload).map_err(|e| FrameError::PayloadDeserialization(e.to_string()))
}

pub fn encode_manifest(m: &OriginManifestPayload) -> Result<Vec<u8>, FrameError> {
    let total = 2 + m.sha256_hex.len() + m.data.len();
    if total as u32 > MAX_MANIFEST_PAYLOAD {
        return Err(FrameError::PayloadExceedsCap(total as u32));
    }
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&(m.sha256_hex.len() as u16).to_le_bytes());
    buf.extend_from_slice(m.sha256_hex.as_bytes());
    buf.extend_from_slice(&m.data);
    Ok(buf)
}

pub fn decode_manifest(payload: &[u8]) -> Result<OriginManifestPayload, FrameError> {
    if payload.len() < 2 {
        return Err(FrameError::PayloadDeserialization(
            "manifest payload too short".into(),
        ));
    }
    let mut hl = [0u8; 2];
    hl.copy_from_slice(&payload[0..2]);
    let hex_len = u16::from_le_bytes(hl) as usize;
    if payload.len() < 2 + hex_len {
        return Err(FrameError::PayloadDeserialization(
            "manifest hex length exceeds payload".into(),
        ));
    }
    let sha256_hex = std::str::from_utf8(&payload[2..2 + hex_len])
        .map_err(|e| FrameError::PayloadDeserialization(e.to_string()))?
        .to_string();
    let data = payload[2 + hex_len..].to_vec();
    Ok(OriginManifestPayload { sha256_hex, data })
}

/// Compute HMAC-SHA256 using SHA-256 (manual implementation to avoid hmac crate version conflicts).
pub fn compute_hmac(secret: &[u8], data: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    const BLOCK_SIZE: usize = 64;
    let mut key = [0u8; BLOCK_SIZE];
    if secret.len() > BLOCK_SIZE {
        let mut hasher = Sha256::new();
        hasher.update(secret);
        let hash = hasher.finalize();
        key[..32].copy_from_slice(&hash);
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().to_vec()
}

pub fn verify_hmac(secret: &[u8], data: &[u8], expected: &[u8]) -> bool {
    let computed = compute_hmac(secret, data);
    if computed.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

// --- Role-separated handshake transcript (DT-3) ---

/// Domain-separation label for the guest's opening proof.
pub const PROOF_LABEL_GUEST: &[u8] = b"ramshared-vsock-guest-proof-v1";
/// Domain-separation label for the host's challenge proof.
pub const PROOF_LABEL_HOST: &[u8] = b"ramshared-vsock-host-proof-v1";
/// Domain-separation label for the guest's finish proof.
pub const PROOF_LABEL_FINISH: &[u8] = b"ramshared-vsock-finish-proof-v1";

/// Errors from nonce generation and transcript assembly.
#[derive(Debug, PartialEq, Eq)]
pub enum HandshakeError {
    NonceGenerationFailed,
    NonceNotFresh,
    EmptyIdentityClaim,
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonceGenerationFailed => write!(f, "OS CSPRNG nonce generation failed"),
            Self::NonceNotFresh => write!(f, "handshake nonce is all-zero (not fresh)"),
            Self::EmptyIdentityClaim => write!(f, "handshake identity claim is empty"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Guest claims that enter the authenticated transcript.
///
/// `boot_id` and `distro_id` are metadata; the proof binds them but does not
/// make them an identity oracle on their own (DT-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeTranscript {
    pub min_version: u32,
    pub max_version: u32,
    pub boot_id: String,
    pub distro_id: String,
    pub guest_nonce: [u8; 32],
}

/// Host challenge that enters the authenticated transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostChallenge {
    pub accepted_version: u32,
    pub heartbeat_secs: u64,
    pub lease_timeout_secs: u64,
    pub host_nonce: [u8; 32],
}

/// Fill a 32-byte nonce from the OS CSPRNG (`getrandom(2)` on Unix,
/// `SystemFunction036` on Windows). An all-zero result is refused: it means
/// the CSPRNG did not produce a fresh value.
pub fn random_nonce() -> Result<[u8; 32], HandshakeError> {
    let mut nonce = [0u8; 32];
    #[cfg(target_os = "linux")]
    {
        let mut filled = 0usize;
        while filled < nonce.len() {
            // SAFETY: `nonce[filled..]` is a valid mutable byte slice of the
            // requested length; `getrandom` writes at most that many bytes.
            let n = unsafe {
                libc::getrandom(
                    nonce[filled..].as_mut_ptr().cast::<libc::c_void>(),
                    nonce.len() - filled,
                    0,
                )
            };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(HandshakeError::NonceGenerationFailed);
            }
            if n == 0 {
                return Err(HandshakeError::NonceGenerationFailed);
            }
            filled += n as usize;
        }
    }
    #[cfg(windows)]
    {
        // SystemFunction036 (RtlGenRandom) — the OS CSPRNG used by advapi32.
        #[link(name = "advapi32")]
        extern "system" {
            fn SystemFunction036(
                random_buffer: *mut core::ffi::c_void,
                random_buffer_length: u32,
            ) -> u8;
        }
        // SAFETY: `nonce` is a valid 32-byte buffer; the API writes exactly
        // `random_buffer_length` bytes and returns non-zero on success.
        let ok = unsafe { SystemFunction036(nonce.as_mut_ptr().cast(), nonce.len() as u32) };
        if ok == 0 {
            return Err(HandshakeError::NonceGenerationFailed);
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        return Err(HandshakeError::NonceGenerationFailed);
    }
    if nonce == [0u8; 32] {
        return Err(HandshakeError::NonceNotFresh);
    }
    Ok(nonce)
}

fn proof32(secret: &[u8], data: &[u8]) -> [u8; 32] {
    let mac = compute_hmac(secret, data);
    let mut out = [0u8; 32];
    let n = mac.len().min(32);
    out[..n].copy_from_slice(&mac[..n]);
    out
}

fn append_u32(dst: &mut Vec<u8>, v: u32) {
    dst.extend_from_slice(&v.to_le_bytes());
}

fn append_u64(dst: &mut Vec<u8>, v: u64) {
    dst.extend_from_slice(&v.to_le_bytes());
}

fn append_str(dst: &mut Vec<u8>, s: &str) {
    append_u32(dst, s.len() as u32);
    dst.extend_from_slice(s.as_bytes());
}

fn guest_transcript_bytes(t: &HandshakeTranscript) -> Result<Vec<u8>, HandshakeError> {
    if t.boot_id.is_empty() || t.distro_id.is_empty() {
        return Err(HandshakeError::EmptyIdentityClaim);
    }
    if t.guest_nonce == [0u8; 32] {
        return Err(HandshakeError::NonceNotFresh);
    }
    let mut buf = Vec::with_capacity(16 + t.boot_id.len() + t.distro_id.len() + 32);
    append_u32(&mut buf, t.min_version);
    append_u32(&mut buf, t.max_version);
    append_str(&mut buf, &t.boot_id);
    append_str(&mut buf, &t.distro_id);
    buf.extend_from_slice(&t.guest_nonce);
    Ok(buf)
}

fn host_transcript_bytes(
    t: &HandshakeTranscript,
    c: &HostChallenge,
) -> Result<Vec<u8>, HandshakeError> {
    if c.host_nonce == [0u8; 32] {
        return Err(HandshakeError::NonceNotFresh);
    }
    let mut buf = guest_transcript_bytes(t)?;
    buf.extend_from_slice(&c.host_nonce);
    append_u32(&mut buf, c.accepted_version);
    append_u64(&mut buf, c.heartbeat_secs);
    append_u64(&mut buf, c.lease_timeout_secs);
    Ok(buf)
}

/// Guest opening proof: HMAC over its claims and its fresh nonce, under the
/// guest role label. Cannot validate as a host or finish proof.
pub fn guest_proof(secret: &[u8], t: &HandshakeTranscript) -> Result<[u8; 32], HandshakeError> {
    let mut data = PROOF_LABEL_GUEST.to_vec();
    data.extend_from_slice(&guest_transcript_bytes(t)?);
    Ok(proof32(secret, &data))
}

/// Host proof: HMAC over both nonces and the complete transcript, under the
/// host role label. The guest accepts authority only when this validates.
pub fn host_proof(
    secret: &[u8],
    t: &HandshakeTranscript,
    c: &HostChallenge,
) -> Result<[u8; 32], HandshakeError> {
    let mut data = PROOF_LABEL_HOST.to_vec();
    data.extend_from_slice(&host_transcript_bytes(t, c)?);
    Ok(proof32(secret, &data))
}

/// Guest finish proof: proves the same transcript back to the host. Binding
/// both fresh nonces prevents replay of a previously captured transcript.
pub fn guest_finish_proof(
    secret: &[u8],
    t: &HandshakeTranscript,
    c: &HostChallenge,
) -> Result<[u8; 32], HandshakeError> {
    let mut data = PROOF_LABEL_FINISH.to_vec();
    data.extend_from_slice(&host_transcript_bytes(t, c)?);
    Ok(proof32(secret, &data))
}

/// Constant-time equality over two 32-byte proofs.
fn ct_eq32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Constant-time validation of the guest opening proof.
pub fn verify_guest_proof(secret: &[u8], t: &HandshakeTranscript, proof: &[u8; 32]) -> bool {
    match guest_proof(secret, t) {
        Ok(expected) => ct_eq32(&expected, proof),
        Err(_) => false,
    }
}

/// Constant-time validation of the host challenge proof.
pub fn verify_host_proof(
    secret: &[u8],
    t: &HandshakeTranscript,
    c: &HostChallenge,
    proof: &[u8; 32],
) -> bool {
    match host_proof(secret, t, c) {
        Ok(expected) => ct_eq32(&expected, proof),
        Err(_) => false,
    }
}

/// Constant-time validation of the guest finish proof.
pub fn verify_guest_finish_proof(
    secret: &[u8],
    t: &HandshakeTranscript,
    c: &HostChallenge,
    proof: &[u8; 32],
) -> bool {
    match guest_finish_proof(secret, t, c) {
        Ok(expected) => ct_eq32(&expected, proof),
        Err(_) => false,
    }
}

pub fn negotiate_version(guest_min: u32, guest_max: u32) -> Option<u32> {
    let lo = guest_min.max(IPC_MIN_VERSION);
    let hi = guest_max.min(IPC_VERSION_3);
    if lo <= hi { Some(hi) } else { None }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn frame_round_trip() {
        let hdr = VsockFrameHeader::new(MSG_HEARTBEAT, 16, 42);
        let encoded = hdr.encode();
        let decoded = VsockFrameHeader::decode(&encoded).expect("decode");
        assert_eq!(decoded, hdr);
        assert_eq!(decoded.msg_type(), MSG_HEARTBEAT);
    }

    #[test]
    fn frame_rejects_bad_magic() {
        let mut buf = [0u8; FRAME_HEADER_LEN];
        buf[0] = 0xDE;
        assert!(matches!(
            VsockFrameHeader::decode(&buf),
            Err(FrameError::InvalidMagic(_))
        ));
    }

    #[test]
    fn frame_rejects_oversized_payload() {
        let hdr = VsockFrameHeader {
            magic: IPC_MAGIC,
            version: IPC_VERSION_3,
            payload_len: MAX_PAYLOAD_LEN + 1,
            flags: MSG_HEARTBEAT as u32,
            correlation_id: 0,
        };
        let encoded = hdr.encode();
        assert!(matches!(
            VsockFrameHeader::decode(&encoded),
            Err(FrameError::PayloadTooLarge(_))
        ));
    }

    #[test]
    fn frame_rejects_unknown_message_type() {
        let hdr = VsockFrameHeader {
            magic: IPC_MAGIC,
            version: IPC_VERSION_3,
            payload_len: 0,
            flags: 99,
            correlation_id: 0,
        };
        let encoded = hdr.encode();
        assert!(matches!(
            VsockFrameHeader::decode(&encoded),
            Err(FrameError::UnknownMessageType(99))
        ));
    }

    /// DT-2: type 22 (`HandshakeFinish`) is a legal frame; type 23 is not.
    #[test]
    fn frame_accepts_handshake_finish_and_rejects_type_above_max() {
        let finish_hdr = VsockFrameHeader::new(MSG_HANDSHAKE_FINISH, 0, 1);
        let encoded = finish_hdr.encode();
        let decoded = VsockFrameHeader::decode(&encoded).expect("finish type decodes");
        assert_eq!(decoded.msg_type(), MSG_HANDSHAKE_FINISH);
        assert_eq!(MSG_MAX, MSG_HANDSHAKE_FINISH);

        let over = VsockFrameHeader {
            magic: IPC_MAGIC,
            version: IPC_VERSION_3,
            payload_len: 0,
            flags: u32::from(MSG_HANDSHAKE_FINISH) + 1,
            correlation_id: 0,
        };
        let encoded = over.encode();
        assert!(matches!(
            VsockFrameHeader::decode(&encoded),
            Err(FrameError::UnknownMessageType(23))
        ));
    }

    #[test]
    fn version_negotiation_selects_highest_mutual() {
        assert_eq!(negotiate_version(2, 3), Some(3));
        assert_eq!(negotiate_version(2, 2), Some(2));
        assert_eq!(negotiate_version(3, 3), Some(3));
        assert_eq!(negotiate_version(4, 5), None);
        assert_eq!(negotiate_version(1, 1), None);
    }

    #[test]
    fn handshake_hmac_validates() {
        let secret = b"test-secret-key";
        let data = b"boot-id-1234Ubuntu-24.04nonce";
        let hmac_val = compute_hmac(secret, data);
        assert!(verify_hmac(secret, data, &hmac_val));
        assert!(!verify_hmac(b"wrong", data, &hmac_val));
        assert!(!verify_hmac(secret, b"tampered", &hmac_val));
    }

    fn sample_transcript() -> HandshakeTranscript {
        HandshakeTranscript {
            min_version: IPC_MIN_VERSION,
            max_version: IPC_VERSION_3,
            boot_id: "boot-abc".into(),
            distro_id: "Ubuntu-24.04".into(),
            guest_nonce: [7u8; 32],
        }
    }

    fn sample_challenge() -> HostChallenge {
        HostChallenge {
            accepted_version: IPC_VERSION_3,
            heartbeat_secs: 5,
            lease_timeout_secs: 30,
            host_nonce: [9u8; 32],
        }
    }

    /// Kahneman #13/#17: the three-message flow must require fresh,
    /// role-separated proofs. A proof from the wrong role, a stale nonce, or
    /// a zeroed proof must not authenticate anything.
    #[test]
    fn mutual_handshake_requires_fresh_role_bound_proofs() {
        let secret = b"shared-hmac-key";
        let other = b"attacker-key";
        let t = sample_transcript();
        let c = sample_challenge();

        // Legitimate flow: each role's own proof validates under that role.
        let g = guest_proof(secret, &t).expect("guest proof");
        let h = host_proof(secret, &t, &c).expect("host proof");
        let f = guest_finish_proof(secret, &t, &c).expect("finish proof");
        assert!(verify_guest_proof(secret, &t, &g));
        assert!(verify_host_proof(secret, &t, &c, &h));
        assert!(verify_guest_finish_proof(secret, &t, &c, &f));

        // Role separation: a guest proof is not a host proof and not a finish.
        assert!(!verify_host_proof(secret, &t, &c, &g));
        assert!(!verify_guest_finish_proof(secret, &t, &c, &g));
        // A host proof is not a guest proof and not a finish.
        assert!(!verify_guest_proof(secret, &t, &h));
        assert!(!verify_guest_finish_proof(secret, &t, &c, &h));
        // A finish proof is not a guest proof and not a host proof.
        assert!(!verify_guest_proof(secret, &t, &f));
        assert!(!verify_host_proof(secret, &t, &c, &f));

        // Wrong shared secret never authenticates.
        assert!(!verify_guest_proof(other, &t, &g));
        assert!(!verify_host_proof(other, &t, &c, &h));
        assert!(!verify_guest_finish_proof(other, &t, &c, &f));

        // Stale/foreign guest nonce: the captured proof no longer matches.
        let mut stale = t.clone();
        stale.guest_nonce = [1u8; 32];
        assert!(!verify_guest_proof(secret, &stale, &g));
        assert!(!verify_host_proof(secret, &stale, &c, &h));
        assert!(!verify_guest_finish_proof(secret, &stale, &c, &f));

        // Stale/foreign host nonce: host and finish proofs stop validating.
        let mut stale_c = c.clone();
        stale_c.host_nonce = [2u8; 32];
        assert!(!verify_host_proof(secret, &t, &stale_c, &h));
        assert!(!verify_guest_finish_proof(secret, &t, &stale_c, &f));

        // Zeroed proof and zeroed nonces are refused.
        let zero = [0u8; 32];
        assert!(!verify_guest_proof(secret, &t, &zero));
        let mut zero_nonce = t.clone();
        zero_nonce.guest_nonce = [0u8; 32];
        assert!(guest_proof(secret, &zero_nonce).is_err());
        let mut zero_host = c.clone();
        zero_host.host_nonce = [0u8; 32];
        assert!(host_proof(secret, &t, &zero_host).is_err());

        // Both proofs must be present and fresh before authority moves.
        assert_ne!(g, h);
        assert_ne!(h, f);
        assert_ne!(g, f);
    }

    /// Kahneman #13/#17: a finish proof bound to one host challenge must not
    /// authenticate against a replayed or refreshed host nonce. Binding the
    /// finish to both fresh nonces is what prevents transcript replay.
    #[test]
    fn handshake_finish_rejects_replayed_host_challenge() {
        let secret = b"shared-hmac-key";
        let t = sample_transcript();
        let challenge_a = sample_challenge();
        let mut challenge_b = sample_challenge();
        challenge_b.host_nonce = [0xA5u8; 32];

        // Finish computed against challenge A.
        let finish_a = guest_finish_proof(secret, &t, &challenge_a).expect("finish a");
        assert!(verify_guest_finish_proof(
            secret,
            &t,
            &challenge_a,
            &finish_a
        ));

        // Replay of finish A against a different host challenge is rejected.
        assert!(!verify_guest_finish_proof(
            secret,
            &t,
            &challenge_b,
            &finish_a
        ));

        // A finish computed for B does not stand in for A either.
        let finish_b = guest_finish_proof(secret, &t, &challenge_b).expect("finish b");
        assert!(!verify_guest_finish_proof(
            secret,
            &t,
            &challenge_a,
            &finish_b
        ));
        assert!(verify_guest_finish_proof(
            secret,
            &t,
            &challenge_b,
            &finish_b
        ));

        // Replaying the whole captured host challenge with a new guest nonce
        // (the common transcript-replay shape) is also rejected.
        let mut fresh_guest = t.clone();
        fresh_guest.guest_nonce = [0x5Au8; 32];
        assert!(!verify_guest_finish_proof(
            secret,
            &fresh_guest,
            &challenge_a,
            &finish_a
        ));

        // And a replayed host proof from challenge A does not authorize
        // challenge B on the guest side.
        let host_a = host_proof(secret, &t, &challenge_a).expect("host a");
        assert!(!verify_host_proof(secret, &t, &challenge_b, &host_a));
    }

    #[test]
    fn control_round_trip() {
        let hb = Heartbeat {
            timestamp_ms: 12345,
        };
        let encoded = encode_control(&hb).expect("encode");
        let decoded: Heartbeat = decode_control(&encoded).expect("decode");
        assert_eq!(decoded, hb);
    }

    #[test]
    fn control_payload_cap_enforced() {
        let big = Telemetry {
            control_plane_state: "x".repeat(5000),
            heartbeat_rtt_us: 0,
            lease_remaining_ms: 0,
        };
        assert!(matches!(
            encode_control(&big),
            Err(FrameError::PayloadExceedsCap(_))
        ));
    }

    #[test]
    fn manifest_round_trip() {
        let m = OriginManifestPayload {
            sha256_hex: "abc123".into(),
            data: vec![1, 2, 3, 4, 5],
        };
        let encoded = encode_manifest(&m).expect("encode");
        let decoded = decode_manifest(&encoded).expect("decode");
        assert_eq!(decoded, m);
    }

    #[test]
    fn manifest_rejects_short_payload() {
        assert!(matches!(
            decode_manifest(&[0x01]),
            Err(FrameError::PayloadDeserialization(_))
        ));
    }

    #[test]
    fn lib_exports_and_constants_are_consistent() {
        assert_eq!(MSG_MAX, MSG_HANDSHAKE_FINISH);
        assert_eq!(MSG_HANDSHAKE_FINISH, 22);
        assert_eq!(FRAME_HEADER_LEN, 24);
        const {
            assert!(MAX_CONTROL_PAYLOAD < MAX_MANIFEST_PAYLOAD);
            assert!(MAX_MANIFEST_PAYLOAD <= MAX_PAYLOAD_LEN);
        }
    }

    /// Version negotiation is enforced in the header: anything outside
    /// `IPC_MIN_VERSION..=IPC_VERSION_3` is refused before the payload is read.
    #[test]
    fn frame_rejects_unsupported_version() {
        for version in [1u32, 4] {
            let hdr = VsockFrameHeader {
                magic: IPC_MAGIC,
                version,
                payload_len: 0,
                flags: MSG_HEARTBEAT as u32,
                correlation_id: 0,
            };
            let encoded = hdr.encode();
            assert!(matches!(
                VsockFrameHeader::decode(&encoded),
                Err(FrameError::UnsupportedVersion(v)) if v == version
            ));
        }
    }

    /// Every `FrameError` arm is total and distinguishable — telemetry must
    /// never fail to describe a protocol refusal.
    #[test]
    fn frame_error_display_is_total() {
        assert!(
            FrameError::InvalidMagic(0xDEAD_BEEF)
                .to_string()
                .contains("invalid magic")
        );
        assert!(
            FrameError::UnsupportedVersion(4)
                .to_string()
                .contains("unsupported version")
        );
        assert!(
            FrameError::PayloadTooLarge(9)
                .to_string()
                .contains("payload too large")
        );
        assert!(
            FrameError::UnknownMessageType(23)
                .to_string()
                .contains("unknown message type")
        );
        assert!(
            FrameError::PayloadDeserialization("boom".into())
                .to_string()
                .contains("deserialization")
        );
        assert!(
            FrameError::PayloadExceedsCap(5000)
                .to_string()
                .contains("4KB cap")
        );
    }

    /// Every `HandshakeError` arm is total and distinguishable.
    #[test]
    fn handshake_error_display_is_total() {
        assert!(
            HandshakeError::NonceGenerationFailed
                .to_string()
                .contains("CSPRNG")
        );
        assert!(
            HandshakeError::NonceNotFresh
                .to_string()
                .contains("all-zero")
        );
        assert!(
            HandshakeError::EmptyIdentityClaim
                .to_string()
                .contains("empty")
        );
    }

    /// Security rule: oversized payloads are refused at the control and
    /// manifest layers too, not only in the frame header.
    #[test]
    fn control_and_manifest_caps_are_enforced() {
        let oversize_control = vec![0u8; MAX_CONTROL_PAYLOAD as usize + 1];
        assert!(matches!(
            decode_control::<Heartbeat>(&oversize_control),
            Err(FrameError::PayloadExceedsCap(_))
        ));

        let big_manifest = OriginManifestPayload {
            sha256_hex: "ab".repeat(64),
            data: vec![0u8; MAX_MANIFEST_PAYLOAD as usize],
        };
        assert!(matches!(
            encode_manifest(&big_manifest),
            Err(FrameError::PayloadExceedsCap(_))
        ));

        // The hex-length field claims more bytes than the payload holds.
        let mut short = Vec::new();
        short.extend_from_slice(&100u16.to_le_bytes());
        short.extend_from_slice(b"ab");
        assert!(matches!(
            decode_manifest(&short),
            Err(FrameError::PayloadDeserialization(_))
        ));
    }

    /// HMAC key material longer than the SHA-256 block size is hashed first
    /// (RFC 2104), and a truncated expected MAC is refused by length alone.
    #[test]
    fn hmac_long_secret_and_length_mismatch_are_handled() {
        let long_secret = [0x5Au8; 100];
        let data = b"bound-transcript";
        let mac = compute_hmac(&long_secret, data);
        assert_eq!(mac.len(), 32);
        assert!(verify_hmac(&long_secret, data, &mac));
        assert!(!verify_hmac(&long_secret, data, &mac[..16]));
    }

    /// DT-3: nonces come from the OS CSPRNG and must be fresh and non-zero.
    #[test]
    fn random_nonce_is_fresh_and_nonzero() {
        let a = random_nonce().expect("OS CSPRNG nonce");
        let b = random_nonce().expect("OS CSPRNG nonce");
        assert_ne!(a, [0u8; 32]);
        assert_ne!(b, [0u8; 32]);
        assert_ne!(a, b, "two nonces must not collide");
    }

    /// DT-3: empty identity claims and all-zero nonces are refused, and a
    /// verifier returns `false` (never a panic) when the transcript itself is
    /// unusable.
    #[test]
    fn handshake_refuses_empty_claims_and_bad_nonces() {
        let secret = b"shared-hmac-key";
        let mut empty_boot = sample_transcript();
        empty_boot.boot_id.clear();
        assert_eq!(
            guest_proof(secret, &empty_boot),
            Err(HandshakeError::EmptyIdentityClaim)
        );
        assert!(!verify_guest_proof(secret, &empty_boot, &[1u8; 32]));

        let mut empty_distro = sample_transcript();
        empty_distro.distro_id.clear();
        assert_eq!(
            guest_proof(secret, &empty_distro),
            Err(HandshakeError::EmptyIdentityClaim)
        );

        let mut zero_guest = sample_transcript();
        zero_guest.guest_nonce = [0u8; 32];
        assert_eq!(
            guest_proof(secret, &zero_guest),
            Err(HandshakeError::NonceNotFresh)
        );
        assert!(!verify_guest_proof(secret, &zero_guest, &[1u8; 32]));

        let t = sample_transcript();
        let mut zero_host = sample_challenge();
        zero_host.host_nonce = [0u8; 32];
        assert_eq!(
            host_proof(secret, &t, &zero_host),
            Err(HandshakeError::NonceNotFresh)
        );
        assert!(!verify_host_proof(secret, &t, &zero_host, &[1u8; 32]));
        assert!(!verify_guest_finish_proof(
            secret, &t, &zero_host, &[1u8; 32]
        ));
    }
}
