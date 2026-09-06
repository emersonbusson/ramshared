pub enum PreflightState {
    HostUnavailable,
}

pub enum StateTag {
    Absent,
}

pub enum StateTransitionError {
    IllegalTransition {
        expected: Option<StateTag>,
        actual: StateTag,
    },
    IllegalPreflight {
        expected: Option<PreflightState>,
        actual: PreflightState,
    },
    StaleGeneration {
        provided: u64,
        expected: u64,
    },
}

pub enum FailureReason {
    StateTransition(StateTransitionError),
}

fn do_transition() -> Result<(), FailureReason> {
    Err(FailureReason::StateTransition(
        StateTransitionError::IllegalTransition {
            expected: Some(StateTag::Absent),
            actual: StateTag::Absent,
        },
    ))
}
fn main() {
    let res = do_transition();
    match res {
        Err(FailureReason::StateTransition(e)) => return Err(e).unwrap(),
        _ => (),
    }
}
