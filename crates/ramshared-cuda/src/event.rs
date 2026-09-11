//! CUDA Events for timing DMA transfer latencies.
//!
//! SPEC: §8 (CUDA wrappers) - RAII wrappers for `CuEvent`.

use crate::driver::{Context, CudaError, check};
use crate::ffi::{CU_EVENT_BLOCKING_SYNC, CU_EVENT_DEFAULT, CuEvent};

/// A CUDA event used for timing and synchronization.
/// `Drop` implementation calls `cuEventDestroy_v2`.
pub struct Event<'c, 'a> {
    ctx: &'c Context<'a>,
    raw: CuEvent,
}

impl<'c, 'a> Event<'c, 'a> {
    /// Creates a new timing event with default flags (blocking sync).
    pub fn new(ctx: &'c Context<'a>) -> Result<Self, CudaError> {
        Self::with_flags(ctx, CU_EVENT_DEFAULT | CU_EVENT_BLOCKING_SYNC)
    }

    /// Creates a new event with specific flags.
    pub fn with_flags(ctx: &'c Context<'a>, flags: u32) -> Result<Self, CudaError> {
        let mut raw: CuEvent = core::ptr::null_mut();
        // SAFETY: raw points to a valid local; CUDA context is current on the calling thread.
        let r = unsafe { (ctx.cuda.syms.event_create)(&mut raw, flags) };
        check(&ctx.cuda.syms, r, "cuEventCreate")?;
        Ok(Self { ctx, raw })
    }

    /// Records the event in the default stream (stream 0).
    pub fn record(&mut self) -> Result<(), CudaError> {
        let syms = &self.ctx.cuda.syms;
        // SAFETY: raw handle was returned by cuEventCreate; stream is 0 (default).
        let r = unsafe { (syms.event_record)(self.raw, core::ptr::null_mut()) };
        check(syms, r, "cuEventRecord")
    }

    /// Synchronizes on the event, blocking until it completes.
    pub fn synchronize(&self) -> Result<(), CudaError> {
        let syms = &self.ctx.cuda.syms;
        // SAFETY: raw handle was returned by cuEventCreate.
        let r = unsafe { (syms.event_synchronize)(self.raw) };
        check(syms, r, "cuEventSynchronize")
    }

    /// Computes the elapsed time in milliseconds between two recorded events.
    pub fn elapsed_time_ms(&self, end: &Event<'_, '_>) -> Result<f32, CudaError> {
        let mut ms = 0.0f32;
        let syms = &self.ctx.cuda.syms;
        // SAFETY: ms is a valid local; both handles are valid events.
        let r = unsafe { (syms.event_elapsed_time)(&mut ms, self.raw, end.raw) };
        check(syms, r, "cuEventElapsedTime")?;
        Ok(ms)
    }
}

impl Drop for Event<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: raw handle was returned by cuEventCreate and has not been destroyed.
        unsafe {
            let _ = (self.ctx.cuda.syms.event_destroy)(self.raw);
        }
    }
}
