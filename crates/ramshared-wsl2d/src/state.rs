//! Daemon state machine (SPEC §7). Invalid transitions are rejected;
//! `Failed` is reachable from any state; `Demoted` (§9) removes VRAM from the pool
//! without killing the process.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Init,
    PreflightOk,
    MemoryLocked,
    CudaReady,
    VramAllocated,
    ResidencyArmed,
    BlockReady,
    SwapActive,
    Demoted,
    Stopping,
    Failed,
}

impl State {
    /// `true` if the transition `self → to` is allowed.
    pub fn can_transition(self, to: State) -> bool {
        use State::*;
        if to == Failed {
            return true; // hard error from any state
        }
        matches!(
            (self, to),
            (Init, PreflightOk)
                | (PreflightOk, MemoryLocked)
                | (MemoryLocked, CudaReady)
                | (CudaReady, VramAllocated)
                | (VramAllocated, ResidencyArmed)
                | (ResidencyArmed, BlockReady)
                | (BlockReady, SwapActive)
                | (BlockReady, Demoted)
                | (SwapActive, Demoted)
                | (BlockReady, Stopping)
                | (SwapActive, Stopping)
                | (Demoted, Stopping)
                | (Stopping, Init)
        )
    }

    /// Tries to transition; returns the new state or `Err(self)` if invalid.
    pub fn step(self, to: State) -> Result<State, State> {
        if self.can_transition(to) {
            Ok(to)
        } else {
            Err(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::State::*;

    #[test]
    fn happy_path_is_allowed() {
        let path = [
            Init,
            PreflightOk,
            MemoryLocked,
            CudaReady,
            VramAllocated,
            ResidencyArmed,
            BlockReady,
            SwapActive,
        ];
        for w in path.windows(2) {
            assert!(w[0].can_transition(w[1]), "{:?} -> {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn illegal_jumps_rejected() {
        assert!(!Init.can_transition(BlockReady));
        assert!(!CudaReady.can_transition(SwapActive));
        assert!(!Init.can_transition(Stopping));
    }

    #[test]
    fn failed_reachable_from_any() {
        for s in [Init, CudaReady, BlockReady, SwapActive, Demoted, Stopping] {
            assert!(s.can_transition(Failed));
        }
    }

    #[test]
    fn demote_from_active_or_blockready() {
        assert!(SwapActive.can_transition(Demoted));
        assert!(BlockReady.can_transition(Demoted));
        assert_eq!(SwapActive.step(Demoted), Ok(Demoted));
        assert_eq!(Init.step(SwapActive), Err(Init));
    }
}
