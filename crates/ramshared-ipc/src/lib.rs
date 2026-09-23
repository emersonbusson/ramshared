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
pub const MSG_MAX: u8 = 21;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Handshake {
    pub min_version: u32,
    pub max_version: u32,
    pub boot_id: String,
    pub distro_id: String,
    pub hmac: Vec<u8>,
    pub nonce: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandshakeAck {
    pub accepted_version: u32,
    pub heartbeat_secs: u64,
    pub lease_timeout_secs: u64,
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
        assert_eq!(MSG_MAX, MSG_SHUTDOWN_ACK);
        assert_eq!(FRAME_HEADER_LEN, 24);
        const {
            assert!(MAX_CONTROL_PAYLOAD < MAX_MANIFEST_PAYLOAD);
            assert!(MAX_MANIFEST_PAYLOAD <= MAX_PAYLOAD_LEN);
        }
    }
}
