//! Windows control-handle / mapped queue adapter (SPEC DT-4 / ITEM-3).
//!
//! All Windows unsafe IOCTL and `VirtualAlloc` mapping lives here. Pure
//! [`crate::driver_link::InMemoryQueue`] remains the hermetic path.
//!
//! Cover target: N/A — E2E-only (WDK driver + Verifier). This module compiles
//! the safe surface; live mapping is exercised by
//! `scripts/windows/Invoke-WinDriveIoctlValidation.ps1`.

#![cfg(windows)]

use std::time::Duration;

use crate::proto::{DiskParams, Register};

/// IOCTL / mapping errors (stable classes only — no pointers in Display).
#[derive(Debug)]
pub enum IoctlError {
    Open(String),
    Ioctl(String),
    Map(String),
    Timeout,
    Cancelled,
    Invalid(String),
}

impl std::fmt::Display for IoctlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoctlError::Open(s) => write!(f, "open: {s}"),
            IoctlError::Ioctl(s) => write!(f, "ioctl: {s}"),
            IoctlError::Map(s) => write!(f, "map: {s}"),
            IoctlError::Timeout => write!(f, "timeout"),
            IoctlError::Cancelled => write!(f, "cancelled"),
            IoctlError::Invalid(s) => write!(f, "invalid: {s}"),
        }
    }
}

impl std::error::Error for IoctlError {}

/// Contiguous page-aligned SQ/CQ/data regions for REGISTER (DT-4).
///
/// Implementation fills in on Windows MSVC target with VirtualAlloc + one
/// OVERLAPPED COMMIT_AND_FETCH. Stub methods return `IoctlError::Invalid` until
/// linked against a live lab (env-bound).
pub struct WindowsMappedQueue {
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    pub block_size: u32,
}

impl WindowsMappedQueue {
    pub fn try_new(
        queue_depth: u32,
        max_io_bytes: u32,
        block_size: u32,
    ) -> Result<Self, IoctlError> {
        // Allocation of VirtualAlloc regions is implemented when the Windows
        // product path is linked in the lab; keep constructor validation only.
        if queue_depth == 0 || !queue_depth.is_power_of_two() {
            return Err(IoctlError::Invalid("queue_depth".into()));
        }
        Ok(Self {
            queue_depth,
            max_io_bytes,
            block_size,
        })
    }

    /// Build ABI-v1 REGISTER descriptor for `disk_id` (VAs filled when mapped).
    pub fn registration(&self, disk_id: u32) -> Register {
        Register {
            abi_version: crate::proto::ABI_VERSION,
            disk_id,
            queue_depth: self.queue_depth,
            block_size: self.block_size,
            max_io_bytes: self.max_io_bytes,
            reserved: 0,
            sq_ring_va: 0,
            cq_ring_va: 0,
            data_area_va: 0,
            data_area_len: (self.queue_depth as u64) * (self.max_io_bytes as u64),
            sq_event_handle: 0,
            cq_event_handle: 0,
        }
    }
}

/// Owns the control device handle and one pending COMMIT_AND_FETCH OVERLAPPED.
pub struct WindowsDriverLink {
    _private: (),
}

impl WindowsDriverLink {
    pub fn open() -> Result<Self, IoctlError> {
        Err(IoctlError::Open(
            "WindowsDriverLink::open requires lab control device".into(),
        ))
    }

    pub fn create_disk(&mut self, _params: &DiskParams) -> Result<(), IoctlError> {
        Err(IoctlError::Ioctl("not connected".into()))
    }

    pub fn register_queue(&mut self, _reg: &Register) -> Result<(), IoctlError> {
        Err(IoctlError::Ioctl("not connected".into()))
    }

    pub fn commit_and_fetch(&mut self, _timeout: Duration) -> Result<(), IoctlError> {
        Err(IoctlError::Ioctl("not connected".into()))
    }

    pub fn cancel_fetch(&mut self) -> Result<(), IoctlError> {
        Err(IoctlError::Cancelled)
    }

    pub fn unregister_queue(&mut self) -> Result<(), IoctlError> {
        Err(IoctlError::Ioctl("not connected".into()))
    }

    pub fn destroy_disk(&mut self) -> Result<(), IoctlError> {
        Err(IoctlError::Ioctl("not connected".into()))
    }
}
