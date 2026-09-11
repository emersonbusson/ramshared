use std::io;

#[derive(Debug)]
pub enum WinBrokerError {
    PipeBusy,
    NoData,
    BrokenPipe,
    Other(io::Error),
}

impl std::error::Error for WinBrokerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(error) => Some(error),
            _ => None,
        }
    }
}

impl std::fmt::Display for WinBrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PipeBusy => write!(f, "Pipe is busy (-EBUSY)"),
            Self::NoData => write!(f, "No data available (-ENODATA)"),
            Self::BrokenPipe => write!(f, "Broken pipe (-EPIPE)"),
            Self::Other(error) => write!(f, "Broker I/O error: {}", error),
        }
    }
}

impl From<io::Error> for WinBrokerError {
    fn from(error: io::Error) -> Self {
        match error.raw_os_error() {
            Some(231) => Self::PipeBusy,   // ERROR_PIPE_BUSY
            Some(232) => Self::NoData,     // ERROR_NO_DATA
            Some(109) => Self::BrokenPipe, // ERROR_BROKEN_PIPE
            _ => Self::Other(error),
        }
    }
}

impl WinBrokerError {
    /// Reports an error string to the Windows Event Log.
    /// This method logs the string as an error in the Event Log under the RamSharedBroker source.
    pub fn report_to_event_log(message: &str) {
        #[cfg(windows)]
        {
            use std::ptr;
            use windows_sys::Win32::System::EventLog::{
                DeregisterEventSource, EVENTLOG_ERROR_TYPE, RegisterEventSourceW, ReportEventW,
            };

            let source: Vec<u16> = "RamSharedBroker\0".encode_utf16().collect();
            let msg: Vec<u16> = format!("Broker Initialization Error: {}\0", message)
                .encode_utf16()
                .collect();

            // SAFETY: Safe wrappers around Win32 API. Passing correctly null-terminated UTF-16 strings to the API.
            struct EventSourceHandle(isize);
            impl Drop for EventSourceHandle {
                fn drop(&mut self) {
                    if self.0 != 0 {
                        // SAFETY: Deregistering a valid event source handle.
                        unsafe {
                            let _ = DeregisterEventSource(self.0);
                        }
                    }
                }
            }

            // SAFETY: Safe wrappers around Win32 API. Passing correctly null-terminated UTF-16 strings to the API.
            unsafe {
                let handle = RegisterEventSourceW(ptr::null(), source.as_ptr());
                if !handle.is_null() {
                    let _guard = EventSourceHandle(handle);
                    let strings = [msg.as_ptr()];
                    let _ = ReportEventW(
                        handle,
                        EVENTLOG_ERROR_TYPE,
                        0,
                        1000, // Using 1000 for generic init error
                        ptr::null_mut(),
                        1,
                        0,
                        strings.as_ptr(),
                        ptr::null(),
                    );
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = message;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::WinBrokerError;
    use std::io;

    #[test]
    fn winbrokererror_mapping() {
        let e = io::Error::from_raw_os_error(231);
        assert!(matches!(WinBrokerError::from(e), WinBrokerError::PipeBusy));

        let e = io::Error::from_raw_os_error(232);
        assert!(matches!(WinBrokerError::from(e), WinBrokerError::NoData));

        let e = io::Error::from_raw_os_error(109);
        assert!(matches!(
            WinBrokerError::from(e),
            WinBrokerError::BrokenPipe
        ));

        let e = io::Error::from_raw_os_error(5); // Access denied
        assert!(matches!(WinBrokerError::from(e), WinBrokerError::Other(_)));
    }

    #[test]
    fn event_log_reporting() {
        // Just verify it doesn't panic.
        WinBrokerError::report_to_event_log("test error");
    }
}
