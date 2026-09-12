use crate::driver::{Context, CudaError};
use crate::ffi::CuContext;

/// A push-pop scope for a CUDA context.
/// Ensures the context is current on the calling thread for the duration of the scope,
/// and restores the previous context when dropped.
pub struct ContextScope<'c, 'a> {
    ctx: &'c Context<'a>,
    popped: bool,
}

impl<'c, 'a> ContextScope<'c, 'a> {
    pub(crate) fn new(ctx: &'c Context<'a>) -> Result<Self, CudaError> {
        let syms = &ctx.cuda.syms;
        // SAFETY: push the raw context onto the thread's stack.
        // It becomes current for this thread, while previous context is pushed down.
        let r = unsafe { (syms.ctx_push_current)(ctx.raw) };
        crate::driver::check(syms, r, "cuCtxPushCurrent_v2")?;
        Ok(Self { ctx, popped: false })
    }

    /// Explicitly pop the context early.
    pub fn pop(mut self) -> Result<(), CudaError> {
        let syms = &self.ctx.cuda.syms;
        let mut popped_raw: CuContext = core::ptr::null_mut();
        // SAFETY: pop the current context from the thread's stack.
        let r = unsafe { (syms.ctx_pop_current)(&mut popped_raw) };
        crate::driver::check(syms, r, "cuCtxPopCurrent_v2")?;
        self.popped = true;
        Ok(())
    }
}

impl Drop for ContextScope<'_, '_> {
    fn drop(&mut self) {
        if !self.popped {
            let syms = &self.ctx.cuda.syms;
            let mut popped_raw: CuContext = core::ptr::null_mut();
            // SAFETY: pop the current context from the thread's stack.
            // Best effort error ignoring during drop.
            unsafe {
                let _ = (syms.ctx_pop_current)(&mut popped_raw);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use crate::Cuda;

    #[test]
    #[ignore = "requires a working CUDA GPU (run with --ignored on a GPU host)"]
    fn test_context_scope_push_pop() {
        let cuda = Cuda::load().expect("libcuda must load");
        if cuda.device_count().unwrap() < 1 {
            return;
        }
        let dev = cuda.device(0).unwrap();
        let ctx = cuda.create_context(&dev).unwrap();

        // Push scope
        let scope = ctx.push_scope().expect("push_scope should succeed");

        // Pop early explicitly
        scope.pop().expect("explicit pop should succeed");
    }
}
