use super::*;
use core::fmt;

/// Host ownership marker in an observation record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authority {
    /// The record was issued by the host-owned contract.
    Host,
    /// A guest-originated value is never sufficient for N3 authority.
    Guest,
    /// Unknown authority fails closed.
    Unknown,
}

/// Bounded opaque identity.  The guest compares bytes but never interprets
/// them as CUDA ordinals, PFNs, adapter indexes, or host pointers.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct OpaqueId(pub(crate) Vec<u8>);

impl OpaqueId {
    /// Creates an opaque identity after applying the contract size bound.
    pub fn new<B: AsRef<[u8]>>(bytes: B) -> Result<Self, FailureReason> {
        let bytes = bytes.as_ref();
        if bytes.is_empty() || bytes.len() > MAX_OPAQUE_ID_BYTES {
            return Err(FailureReason::MalformedRecord);
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Returns the identity bytes for exact equality checks.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for OpaqueId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueId")
            .field("length", &self.0.len())
            .finish()
    }
}

/// Lease identity is opaque and host-issued.
pub type LeaseId = OpaqueId;
/// Event identity is opaque and host-issued.
pub type EventId = OpaqueId;
/// Adapter identity is opaque and host-issued.
pub type AdapterId = OpaqueId;

/// One host-issued generation checkpoint retained across a process restart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationCheckpoint {
    pub lease_id: LeaseId,
    pub generation: u64,
}

/// Bounded, canonical restart input supplied by the host authority.
///
/// This type only parses and serializes caller-owned bytes. It neither reads
/// nor writes durable storage, and it makes no claim that an external caller
/// authenticated the bytes before supplying them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestartRecord {
    schema_version: u16,
    authority: Authority,
    host_epoch: u64,
    checkpoints: Vec<GenerationCheckpoint>,
}

impl RestartRecord {
    /// Builds a canonical host-authoritative record from bounded checkpoints.
    pub fn host(
        host_epoch: u64,
        mut checkpoints: Vec<GenerationCheckpoint>,
    ) -> Result<Self, FailureReason> {
        checkpoints.sort_by(|left, right| left.lease_id.as_bytes().cmp(right.lease_id.as_bytes()));
        let record = Self {
            schema_version: N3_SCHEMA_VERSION,
            authority: Authority::Host,
            host_epoch,
            checkpoints,
        };
        record.validate()?;
        Ok(record)
    }

    /// Serializes the validated canonical record into bounded caller-owned bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            RESTART_RECORD_HEADER_BYTES
                + self
                    .checkpoints
                    .iter()
                    .map(|checkpoint| 1 + checkpoint.lease_id.as_bytes().len() + 8)
                    .sum::<usize>(),
        );
        bytes.extend_from_slice(RESTART_RECORD_MAGIC);
        bytes.extend_from_slice(&self.schema_version.to_be_bytes());
        bytes.push(authority_marker(self.authority));
        bytes.extend_from_slice(&self.host_epoch.to_be_bytes());
        bytes.extend_from_slice(&(self.checkpoints.len() as u16).to_be_bytes());
        for checkpoint in &self.checkpoints {
            bytes.push(checkpoint.lease_id.as_bytes().len() as u8);
            bytes.extend_from_slice(checkpoint.lease_id.as_bytes());
            bytes.extend_from_slice(&checkpoint.generation.to_be_bytes());
        }
        bytes
    }

    /// Parses a complete canonical host restart record without changing model state.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FailureReason> {
        if !(RESTART_RECORD_HEADER_BYTES..=MAX_RESTART_RECORD_BYTES).contains(&bytes.len())
            || bytes.get(..4) != Some(RESTART_RECORD_MAGIC.as_slice())
        {
            return Err(FailureReason::MalformedRecord);
        }
        let mut cursor = 4;
        let schema_version = read_u16(bytes, &mut cursor)?;
        let authority = read_authority(bytes, &mut cursor)?;
        let host_epoch = read_u64(bytes, &mut cursor)?;
        let checkpoint_count = usize::from(read_u16(bytes, &mut cursor)?);
        if checkpoint_count > MAX_GENERATION_HISTORY {
            return Err(FailureReason::MalformedRecord);
        }

        let mut checkpoints = Vec::with_capacity(checkpoint_count);
        for _ in 0..checkpoint_count {
            let identity_length = usize::from(read_u8(bytes, &mut cursor)?);
            if identity_length == 0 || identity_length > MAX_OPAQUE_ID_BYTES {
                return Err(FailureReason::MalformedRecord);
            }
            let identity = read_exact(bytes, &mut cursor, identity_length)?;
            let lease_id = LeaseId::new(identity)?;
            let generation = read_u64(bytes, &mut cursor)?;
            checkpoints.push(GenerationCheckpoint {
                lease_id,
                generation,
            });
        }
        if cursor != bytes.len() {
            return Err(FailureReason::MalformedRecord);
        }

        let record = Self {
            schema_version,
            authority,
            host_epoch,
            checkpoints,
        };
        record.validate()?;
        Ok(record)
    }

    /// Returns the non-zero host epoch carried by this validated record.
    pub fn host_epoch(&self) -> u64 {
        self.host_epoch
    }

    /// Returns canonical lease-generation checkpoints without granting a lease.
    pub fn checkpoints(&self) -> &[GenerationCheckpoint] {
        &self.checkpoints
    }

    /// Consumes the record and returns its inner checkpoints without cloning.
    pub fn into_checkpoints(self) -> Vec<GenerationCheckpoint> {
        self.checkpoints
    }

    pub(crate) fn validate(&self) -> Result<(), FailureReason> {
        if self.schema_version != N3_SCHEMA_VERSION {
            return Err(FailureReason::UnknownSchema);
        }
        if self.authority != Authority::Host {
            return Err(FailureReason::HostAuthorityRequired);
        }
        if self.host_epoch == 0 || self.checkpoints.len() > MAX_GENERATION_HISTORY {
            return Err(FailureReason::MalformedRecord);
        }
        for checkpoint in &self.checkpoints {
            if checkpoint.lease_id.as_bytes().is_empty() || checkpoint.generation == 0 {
                return Err(FailureReason::MalformedRecord);
            }
        }
        if self
            .checkpoints
            .windows(2)
            .any(|pair| pair[0].lease_id.as_bytes() >= pair[1].lease_id.as_bytes())
        {
            return Err(FailureReason::MalformedRecord);
        }
        if self.to_bytes().len() > MAX_RESTART_RECORD_BYTES {
            return Err(FailureReason::MalformedRecord);
        }
        Ok(())
    }
}

fn authority_marker(authority: Authority) -> u8 {
    match authority {
        Authority::Host => HOST_AUTHORITY_MARKER,
        Authority::Guest => GUEST_AUTHORITY_MARKER,
        Authority::Unknown => 0,
    }
}

fn read_exact<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], FailureReason> {
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or(FailureReason::MalformedRecord)?;
    let output = &bytes[*cursor..end];
    *cursor = end;
    Ok(output)
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, FailureReason> {
    read_exact(bytes, cursor, 1).map(|value| value[0])
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, FailureReason> {
    let value = read_exact(bytes, cursor, 2)?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, FailureReason> {
    let value = read_exact(bytes, cursor, 8)?;
    Ok(u64::from_be_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

fn read_authority(bytes: &[u8], cursor: &mut usize) -> Result<Authority, FailureReason> {
    match read_u8(bytes, cursor)? {
        HOST_AUTHORITY_MARKER => Ok(Authority::Host),
        GUEST_AUTHORITY_MARKER => Ok(Authority::Guest),
        _ => Ok(Authority::Unknown),
    }
}
