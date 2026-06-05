//! Máquina de estados do daemon (SPEC §7). Transições inválidas são rejeitadas;
//! `Failed` é alcançável de qualquer estado; `Demoted` (§9) tira a VRAM do pool
//! sem matar o processo.

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
    /// `true` se a transição `self → to` é permitida.
    pub fn can_transition(self, to: State) -> bool {
        use State::*;
        if to == Failed {
            return true; // erro duro de qualquer estado
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

    /// Tenta transicionar; retorna o novo estado ou `Err(self)` se inválida.
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
