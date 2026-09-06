FINDING_ONLY
The objective to map Windows named pipe errors to semantic WinBrokerError variants is already fully and perfectly implemented in crates/ramshared-winbroker/src/lib.rs.
Evidence:
pub enum WinBrokerError {
    PipeBusy,
    NoData,
    BrokenPipe,
    Other(std::io::Error),
}
impl From<std::io::Error> for WinBrokerError {
    fn from(error: std::io::Error) -> Self {
        match error.raw_os_error() {
            Some(231) => Self::PipeBusy,   // ERROR_PIPE_BUSY
            Some(232) => Self::NoData,     // ERROR_NO_DATA
            Some(109) => Self::BrokenPipe, // ERROR_BROKEN_PIPE
            _ => Self::Other(error),
        }
    }
}
