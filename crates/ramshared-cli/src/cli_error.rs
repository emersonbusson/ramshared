use thiserror::Error;

/// Error types related to command line execution and parsing.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CliError {
    /// Command is not recognized or not implemented.
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),

    /// Missing or malformed options for a given command.
    #[error("invalid {command} option: {options}")]
    InvalidOption {
        command: &'static str,
        options: String,
    },

    /// No CUDA-compatible devices were found on the system.
    #[error("CUDA found no devices")]
    CudaNoDevices,

    /// An unexpected error occurred while interacting with the CUDA API.
    #[error("CUDA probe failed: {0}")]
    CudaProbe(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats_correctly() {
        let err = CliError::UnsupportedCommand("foo".to_string());
        assert_eq!(err.to_string(), "unsupported command: foo");

        let err2 = CliError::InvalidOption {
            command: "bar",
            options: "--baz".to_string(),
        };
        assert_eq!(err2.to_string(), "invalid bar option: --baz");

        let err3 = CliError::CudaNoDevices;
        assert_eq!(err3.to_string(), "CUDA found no devices");

        let err4 = CliError::CudaProbe("cudaErrorUnknown".to_string());
        assert_eq!(err4.to_string(), "CUDA probe failed: cudaErrorUnknown");
    }
}
