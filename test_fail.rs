use std::fmt::Display;

pub enum StateTransitionError {
    IllegalTransition {
        expected: Option<String>,
        actual: String,
    }
}

impl core::fmt::Display for StateTransitionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "error")
    }
}

pub enum FailureReason {
    StateTransition(StateTransitionError),
}

fn something() -> Result<(), FailureReason> {
    Err(FailureReason::StateTransition(
        StateTransitionError::IllegalTransition {
            expected: None,
            actual: "test".to_string(),
        },
    ))
}

fn main() {
    let result = something();
    match result {
        Err(FailureReason::StateTransition(e)) => {
            println!("{}", e); // prints "error" correctly
        }
        _ => {}
    }
}
