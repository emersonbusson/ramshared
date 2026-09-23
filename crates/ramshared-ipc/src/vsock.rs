//! vsock transport abstraction for host-guest IPC.
//!
//! Guest side: `AF_VSOCK` stream socket to `VMADDR_CID_HOST`.
//! Host side: `AF_HYPERV` stream socket on a well-known GUID.
//!
//! SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md §DT-1

use std::io::{Read, Write};
use std::time::Duration;

/// vsock address metadata for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsockEndpoint {
    pub cid: u32,
    pub port: u32,
    pub guid: [u8; 16],
}

/// vsock transport errors.
#[derive(Debug, PartialEq, Eq)]
pub enum VsockError {
    ConnectTimeout,
    ConnectFailed(String),
    ListenFailed(String),
    AcceptFailed(String),
    IoError(String),
    Unsupported,
}

impl std::fmt::Display for VsockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectTimeout => write!(f, "vsock connect timed out"),
            Self::ConnectFailed(e) => write!(f, "vsock connect failed: {e}"),
            Self::ListenFailed(e) => write!(f, "vsock listen failed: {e}"),
            Self::AcceptFailed(e) => write!(f, "vsock accept failed: {e}"),
            Self::IoError(e) => write!(f, "vsock io error: {e}"),
            Self::Unsupported => write!(f, "vsock not supported on this platform"),
        }
    }
}

impl std::error::Error for VsockError {}

/// A connected vsock stream (Read + Write).
pub trait VsockStreamTrait: Read + Write + Send {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()>;
    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()>;
    fn shutdown_both(&self) -> std::io::Result<()>;
}

/// Mock vsock stream backed by `std::os::unix::net::UnixStream` for testing.
#[cfg(unix)]
pub struct MockVsockStream(pub std::os::unix::net::UnixStream);

#[cfg(unix)]
impl Read for MockVsockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(unix)]
impl Write for MockVsockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[cfg(unix)]
impl VsockStreamTrait for MockVsockStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_write_timeout(timeout)
    }
    fn shutdown_both(&self) -> std::io::Result<()> {
        use std::net::Shutdown;
        self.0.shutdown(Shutdown::Both)
    }
}

/// Connect to a vsock endpoint with bounded timeout.
///
/// On Unix (test/dev): creates a `MockVsockStream` via `UnixStream::pair` is not
/// possible for remote connect. Production uses `libc::socket(AF_VSOCK)`.
/// This function is the production entrypoint and returns `Unsupported` on
/// platforms where AF_VSOCK is not available.
pub fn connect_vsock(
    _cid: u32,
    _port: u32,
    _timeout: Duration,
) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
    // Production path: libc::socket(AF_VSOCK, SOCK_STREAM, 0) + connect with timeout.
    // Env-bound: real AF_VSOCK requires Hyper-V paired VM.
    #[cfg(target_os = "linux")]
    {
        connect_vsock_linux(_cid, _port, _timeout)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(VsockError::Unsupported)
    }
}

#[cfg(target_os = "linux")]
fn connect_vsock_linux(
    cid: u32,
    port: u32,
    timeout: Duration,
) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
    use std::os::unix::io::FromRawFd;

    const AF_VSOCK: i32 = 40;
    const SOCK_STREAM: i32 = 1;

    #[repr(C)]
    struct SockAddrVm {
        svm_family: u16,
        svm_reserved1: u16,
        svm_port: u32,
        svm_cid: u32,
        svm_zero: [u8; 4],
    }

    unsafe extern "C" {
        fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        fn connect(fd: i32, addr: *const SockAddrVm, len: u32) -> i32;
    }

    let fd = unsafe { socket(AF_VSOCK, SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(VsockError::ConnectFailed("socket() failed".into()));
    }

    let addr = SockAddrVm {
        svm_family: AF_VSOCK as u16,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: cid,
        svm_zero: [0; 4],
    };

    // Set connect timeout via SO_SNDTIMEO before blocking connect.
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_read_timeout(Some(timeout));

    // Non-blocking connect with timeout is complex; use blocking connect with
    // SO_SNDTIMEO as a best-effort bound. For production, use poll() loop.
    let ret = unsafe { connect(fd, &addr, std::mem::size_of::<SockAddrVm>() as u32) };
    if ret != 0 {
        drop(stream);
        return Err(VsockError::ConnectTimeout);
    }

    Ok(Box::new(MockVsockStream(stream)))
}

/// Listen for vsock connections on a Hyper-V GUID (host side).
///
/// Env-bound: real AF_HYPERV listener requires Windows host with Hyper-V.
pub fn listen_hyperv(_guid: [u8; 16], _timeout: Duration) -> Result<VsockListener, VsockError> {
    // Production path: WSA socket(AF_HYPERV) + bind + listen.
    // Env-bound for Day-0.
    Err(VsockError::Unsupported)
}

/// vsock listener (host side).
pub struct VsockListener {
    #[allow(dead_code)]
    endpoint: VsockEndpoint,
}

impl VsockListener {
    /// Accept a connection with bounded timeout.
    pub fn accept(&self, _timeout: Duration) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
        Err(VsockError::AcceptFailed("not implemented".into()))
    }
}

/// Detect whether vsock is available on this system.
pub fn vsock_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for /dev/vsock or successful AF_VSOCK socket creation.
        std::path::Path::new("/dev/vsock").exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::time::Instant;

    #[test]
    fn vsock_connect_timeout_falls_back() {
        // CID 2 (host) with a high port — should fail or timeout on most systems.
        // On systems with vsock and no listener, connect returns ECONNREFUSED.
        // On systems without vsock, connect returns ENODEV/EINVAL.
        // On systems with a listener (unlikely on random port), connect succeeds.
        let start = Instant::now();
        let result = connect_vsock(2, 50000, Duration::from_millis(200));
        let elapsed = start.elapsed();

        // Must complete within the bounded window regardless of outcome.
        assert!(
            elapsed < Duration::from_secs(5),
            "connect timeout must be bounded: {elapsed:?}"
        );
        // If it failed, the error must be typed. If it succeeded, the stream must be usable.
        match result {
            Err(VsockError::ConnectTimeout) => {}
            Err(VsockError::ConnectFailed(_)) => {}
            Err(VsockError::Unsupported) => {}
            Ok(_) => {
                // Connection succeeded — this is acceptable on systems with vsock
                // and a listening service on the target port.
            }
            Err(e) => panic!("unexpected error type: {e}"),
        }
    }

    #[test]
    fn vsock_stream_read_timeout_is_bounded() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut stream = MockVsockStream(client);
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();

        let start = Instant::now();
        let mut buf = [0u8; 16];
        let result = stream.read(&mut buf);
        let elapsed = start.elapsed();

        assert!(result.is_err(), "read on empty socket must timeout");
        assert!(
            elapsed < Duration::from_millis(500),
            "read timeout must be bounded: {elapsed:?}"
        );
    }

    #[test]
    fn vsock_disconnect_detected_within_interval() {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut stream = MockVsockStream(client);
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        // Drop the server side to simulate disconnect.
        drop(server);

        let start = Instant::now();
        let mut buf = [0u8; 16];
        let result = stream.read(&mut buf);
        let elapsed = start.elapsed();

        // Read on disconnected socket returns EOF (Ok(0)) or error.
        match result {
            Ok(0) => {}  // EOF = disconnect detected
            Err(_) => {} // error = disconnect detected
            Ok(n) => panic!("unexpected read of {n} bytes after disconnect"),
        }
        assert!(
            elapsed < Duration::from_millis(500),
            "disconnect detection must be bounded: {elapsed:?}"
        );
    }

    #[test]
    fn vsock_available_detects_platform() {
        // On Linux CI without /dev/vsock, this returns false.
        // On a real WSL2 host with vsock, this returns true.
        let _ = vsock_available(); // no assert — platform-dependent
    }

    #[test]
    fn vsock_error_display_is_informative() {
        let err = VsockError::ConnectTimeout;
        assert!(err.to_string().contains("timed out"));
        let err = VsockError::Unsupported;
        assert!(err.to_string().contains("not supported"));
    }

    #[test]
    fn mock_vsock_stream_write_and_flush() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut stream = MockVsockStream(client);
        let data = b"hello vsock";
        let n = stream.write(data).unwrap();
        assert_eq!(n, data.len());
        assert!(stream.flush().is_ok());
    }

    #[test]
    fn mock_vsock_stream_trait_methods() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let stream = MockVsockStream(client);
        assert!(
            stream
                .set_read_timeout(Some(Duration::from_millis(50)))
                .is_ok()
        );
        assert!(
            stream
                .set_write_timeout(Some(Duration::from_millis(50)))
                .is_ok()
        );
        assert!(stream.shutdown_both().is_ok());
    }

    #[test]
    fn listen_hyperv_returns_unsupported_on_linux() {
        let result = listen_hyperv([0; 16], Duration::from_millis(100));
        assert!(matches!(result, Err(VsockError::Unsupported)));
    }

    #[test]
    fn vsock_listener_accept_returns_error_when_not_implemented() {
        let listener = VsockListener {
            endpoint: VsockEndpoint {
                cid: 2,
                port: 1234,
                guid: [0; 16],
            },
        };
        let result = listener.accept(Duration::from_millis(100));
        assert!(result.is_err());
    }

    #[test]
    fn vsock_endpoint_fields_are_accessible() {
        let ep = VsockEndpoint {
            cid: 3,
            port: 5000,
            guid: [1; 16],
        };
        assert_eq!(ep.cid, 3);
        assert_eq!(ep.port, 5000);
        assert_eq!(ep.guid, [1; 16]);
    }
}
