//! vsock transport abstraction for host-guest IPC.
//!
//! Guest side: `AF_VSOCK` stream socket to `VMADDR_CID_HOST`.
//! Host side: `AF_HYPERV` stream socket on a well-known GUID.
//!
//! SPEC: docs/specs/no-milestone/native-vsock-host-guest-control-plane/SPEC.md §DT-1

use std::io::{Read, Write};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", test))]
const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(any(target_os = "linux", test))]
fn bounded_connect_timeout(timeout: Duration) -> Duration {
    timeout.min(MAX_CONNECT_TIMEOUT)
}

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
    AcceptTimeout,
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
            Self::AcceptTimeout => write!(f, "vsock accept timed out"),
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
    cid: u32,
    port: u32,
    timeout: Duration,
) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
    #[cfg(target_os = "linux")]
    {
        connect_vsock_linux(cid, port, timeout)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cid, port, timeout);
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

    if port == 0 {
        return Err(VsockError::ConnectFailed(
            "vsock port must be nonzero".into(),
        ));
    }

    let fd = unsafe {
        libc::socket(
            libc::AF_VSOCK,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(VsockError::ConnectFailed(
            std::io::Error::last_os_error().to_string(),
        ));
    }

    // SAFETY: `socket` returned a fresh owned descriptor. `UnixStream` only
    // supplies generic stream I/O and timeout operations; Linux accepts any
    // connected SOCK_STREAM descriptor here, including AF_VSOCK.
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    let addr = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as libc::sa_family_t,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: cid,
        svm_zero: [0; 4],
    };

    let result = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_vm).cast::<libc::sockaddr>(),
            std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINPROGRESS | libc::EALREADY | libc::EINTR) => {
                wait_for_connect_until(
                    bounded_connect_timeout(timeout),
                    |remaining| poll_connect_ready(fd, remaining),
                    || socket_connect_error(fd),
                )?;
            }
            _ => return Err(VsockError::ConnectFailed(error.to_string())),
        }
    }

    stream
        .set_nonblocking(false)
        .map_err(|error| VsockError::ConnectFailed(error.to_string()))?;
    Ok(Box::new(UnixVsockStream(stream)))
}

#[cfg(target_os = "linux")]
fn poll_connect_ready(fd: std::os::fd::RawFd, remaining: Duration) -> std::io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLOUT,
        revents: 0,
    };
    let timeout_ms = remaining
        .as_secs()
        .saturating_mul(1_000)
        .saturating_add(u64::from(remaining.subsec_nanos().div_ceil(1_000_000)))
        .min(i32::MAX as u64) as i32;
    let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
    if result < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result > 0)
    }
}

#[cfg(target_os = "linux")]
fn socket_connect_error(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    let mut error_code: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&error_code) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut error_code as *mut libc::c_int).cast::<libc::c_void>(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if error_code != 0 {
        return Err(std::io::Error::from_raw_os_error(error_code));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn wait_for_connect_until(
    timeout: Duration,
    mut wait_for_writable: impl FnMut(Duration) -> std::io::Result<bool>,
    mut read_socket_error: impl FnMut() -> std::io::Result<()>,
) -> Result<(), VsockError> {
    let started = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(VsockError::ConnectTimeout);
        }
        match wait_for_writable(remaining) {
            Ok(false) => return Err(VsockError::ConnectTimeout),
            Ok(true) if started.elapsed() >= timeout => return Err(VsockError::ConnectTimeout),
            Ok(true) => {
                return read_socket_error()
                    .map_err(|error| VsockError::ConnectFailed(error.to_string()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(VsockError::ConnectFailed(error.to_string())),
        }
    }
}

#[cfg(target_os = "linux")]
struct UnixVsockStream(std::os::unix::net::UnixStream);

#[cfg(target_os = "linux")]
impl Read for UnixVsockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(target_os = "linux")]
impl Write for UnixVsockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[cfg(target_os = "linux")]
impl VsockStreamTrait for UnixVsockStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    fn shutdown_both(&self) -> std::io::Result<()> {
        self.0.shutdown(std::net::Shutdown::Both)
    }
}

/// Bind the host side of a Hyper-V socket service.
///
/// `guid` is the service GUID in canonical UUID byte order. Hyper-V requires
/// the service to be registered in the host GuestCommunicationServices registry.
pub fn listen_hyperv(guid: [u8; 16]) -> Result<VsockListener, VsockError> {
    let port =
        service_port_from_guid(guid).map_err(|error| VsockError::ListenFailed(error.into()))?;
    #[cfg(windows)]
    {
        windows_hyperv::listen(guid, port)
    }
    #[cfg(not(windows))]
    {
        let _ = (guid, port);
        Err(VsockError::Unsupported)
    }
}

/// vsock listener (host side).
pub struct VsockListener {
    endpoint: VsockEndpoint,
    #[cfg(windows)]
    inner: windows_hyperv::Listener,
}

impl VsockListener {
    /// Return the endpoint metadata used to create this listener.
    pub fn endpoint(&self) -> VsockEndpoint {
        self.endpoint
    }

    /// Accept one connection, returning `AcceptTimeout` when no peer arrives by
    /// the deadline. The returned stream owns the accepted socket.
    pub fn accept(&self, timeout: Duration) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
        #[cfg(windows)]
        {
            self.inner.accept(timeout)
        }
        #[cfg(not(windows))]
        {
            let _ = timeout;
            Err(VsockError::Unsupported)
        }
    }
}

#[cfg(any(windows, test))]
fn accept_until<T>(
    timeout: Duration,
    mut try_accept: impl FnMut() -> Result<Option<T>, VsockError>,
) -> Result<T, VsockError> {
    let started = Instant::now();
    loop {
        if let Some(stream) = try_accept()? {
            return Ok(stream);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(VsockError::AcceptTimeout);
        }
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

#[cfg(any(windows, test))]
fn guid_components(bytes: [u8; 16]) -> (u32, u16, u16, [u8; 8]) {
    (
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        [
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ],
    )
}

fn service_port_from_guid(guid: [u8; 16]) -> Result<u32, &'static str> {
    const LINUX_VSOCK_GUID_SUFFIX: [u8; 12] = [
        0xfa, 0xcb, 0x11, 0xe6, 0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79, 0x86, 0xd3,
    ];
    if guid[4..] != LINUX_VSOCK_GUID_SUFFIX {
        return Err("service GUID must use the Linux Hyper-V socket template");
    }
    let port = u32::from_be_bytes([guid[0], guid[1], guid[2], guid[3]]);
    if port == 0 {
        return Err("service GUID must encode a nonzero vsock port");
    }
    Ok(port)
}

#[cfg(windows)]
mod windows_hyperv {
    use super::*;
    use std::mem::MaybeUninit;
    use std::net::TcpStream;
    use std::os::windows::io::{FromRawSocket, RawSocket};
    use std::sync::Arc;
    use windows_sys::Win32::Networking::WinSock::{
        AF_HYPERV, FIONBIO, INVALID_SOCKET, SOCK_STREAM, SOCKET, SOMAXCONN, WSACleanup, WSADATA,
        WSAEINTR, WSAEWOULDBLOCK, WSAGetLastError, WSAStartup, accept, bind, closesocket,
        ioctlsocket, listen as winsock_listen, socket,
    };
    use windows_sys::Win32::System::Hypervisor::{HV_PROTOCOL_RAW, SOCKADDR_HV};
    use windows_sys::core::GUID;

    struct WinsockRuntime;

    impl WinsockRuntime {
        fn start() -> Result<Arc<Self>, VsockError> {
            let mut data = MaybeUninit::<WSADATA>::uninit();
            let result = unsafe { WSAStartup(0x0202, data.as_mut_ptr()) };
            if result != 0 {
                return Err(VsockError::ListenFailed(format!(
                    "WSAStartup failed with error {result}"
                )));
            }
            Ok(Arc::new(Self))
        }
    }

    impl Drop for WinsockRuntime {
        fn drop(&mut self) {
            let _ = unsafe { WSACleanup() };
        }
    }

    pub struct Listener {
        socket: SOCKET,
        runtime: Arc<WinsockRuntime>,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = unsafe { closesocket(self.socket) };
        }
    }

    struct Stream {
        stream: TcpStream,
        _runtime: Arc<WinsockRuntime>,
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.stream.read(buf)
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.stream.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.stream.flush()
        }
    }

    impl VsockStreamTrait for Stream {
        fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
            self.stream.set_read_timeout(timeout)
        }

        fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
            self.stream.set_write_timeout(timeout)
        }

        fn shutdown_both(&self) -> std::io::Result<()> {
            self.stream.shutdown(std::net::Shutdown::Both)
        }
    }

    pub fn listen(guid: [u8; 16], port: u32) -> Result<VsockListener, VsockError> {
        let runtime = WinsockRuntime::start()?;
        let socket = unsafe { socket(AF_HYPERV as i32, SOCK_STREAM, HV_PROTOCOL_RAW as i32) };
        if socket == INVALID_SOCKET {
            return Err(VsockError::ListenFailed(format!(
                "socket(AF_HYPERV) failed with Winsock error {}",
                unsafe { WSAGetLastError() }
            )));
        }

        let (data1, data2, data3, data4) = guid_components(guid);
        let address = SOCKADDR_HV {
            Family: AF_HYPERV,
            Reserved: 0,
            VmId: GUID::default(),
            ServiceId: GUID {
                data1,
                data2,
                data3,
                data4,
            },
        };
        let bound = unsafe {
            bind(
                socket,
                (&address as *const SOCKADDR_HV).cast(),
                std::mem::size_of::<SOCKADDR_HV>() as i32,
            )
        };
        if bound != 0 {
            let error = unsafe { WSAGetLastError() };
            unsafe { closesocket(socket) };
            return Err(VsockError::ListenFailed(format!(
                "bind(AF_HYPERV) failed with Winsock error {error}"
            )));
        }
        if unsafe { winsock_listen(socket, SOMAXCONN as i32) } != 0 {
            let error = unsafe { WSAGetLastError() };
            unsafe { closesocket(socket) };
            return Err(VsockError::ListenFailed(format!(
                "listen(AF_HYPERV) failed with Winsock error {error}"
            )));
        }

        let mut nonblocking = 1_u32;
        if unsafe { ioctlsocket(socket, FIONBIO, &mut nonblocking) } != 0 {
            let error = unsafe { WSAGetLastError() };
            unsafe { closesocket(socket) };
            return Err(VsockError::ListenFailed(format!(
                "nonblocking AF_HYPERV setup failed with Winsock error {error}"
            )));
        }

        Ok(VsockListener {
            endpoint: VsockEndpoint { cid: 0, port, guid },
            inner: Listener { socket, runtime },
        })
    }

    impl Listener {
        pub fn accept(&self, timeout: Duration) -> Result<Box<dyn VsockStreamTrait>, VsockError> {
            let runtime = Arc::clone(&self.runtime);
            accept_until(timeout, || {
                let client =
                    unsafe { accept(self.socket, std::ptr::null_mut(), std::ptr::null_mut()) };
                if client == INVALID_SOCKET {
                    let error = unsafe { WSAGetLastError() };
                    return if error == WSAEWOULDBLOCK || error == WSAEINTR {
                        Ok(None)
                    } else {
                        Err(VsockError::AcceptFailed(format!(
                            "accept(AF_HYPERV) failed with Winsock error {error}"
                        )))
                    };
                }

                let mut blocking = 0_u32;
                if unsafe { ioctlsocket(client, FIONBIO, &mut blocking) } != 0 {
                    let error = unsafe { WSAGetLastError() };
                    unsafe { closesocket(client) };
                    return Err(VsockError::AcceptFailed(format!(
                        "blocking accepted socket setup failed with Winsock error {error}"
                    )));
                }

                // SAFETY: Winsock returned a connected stream socket. Ownership is
                // transferred to TcpStream, which closes it when the wrapper drops.
                let stream = unsafe { TcpStream::from_raw_socket(client as RawSocket) };
                Ok(Some(Box::new(Stream {
                    stream,
                    _runtime: Arc::clone(&runtime),
                }) as Box<dyn VsockStreamTrait>))
            })
        }
    }
}

/// Detect whether vsock is available on this system.
pub fn vsock_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::FromRawFd;
        let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            false
        } else {
            // SAFETY: a successful socket call returned a fresh owned descriptor.
            drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
            true
        }
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
    fn vsock_connect_finishes_within_deadline() {
        // CID 2 (host) with a high port — should fail or timeout on most systems.
        // On systems with vsock and no listener, connect returns ECONNREFUSED.
        // On systems without vsock, connect returns ENODEV/EINVAL.
        // On systems with a listener (unlikely on random port), connect succeeds.
        let start = Instant::now();
        let result = connect_vsock(2, 50000, Duration::from_millis(200));
        let elapsed = start.elapsed();

        // Must complete within the bounded window regardless of outcome.
        assert!(
            elapsed < Duration::from_millis(500),
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

    #[cfg(target_os = "linux")]
    #[test]
    fn vsock_connect_rejects_zero_port_before_socket_io() {
        let result = connect_vsock(2, 0, Duration::from_millis(100));
        assert!(matches!(
            result,
            Err(VsockError::ConnectFailed(message)) if message.contains("nonzero")
        ));
    }

    #[test]
    fn connect_wait_enforces_deadline_when_waiter_returns_late() {
        let started = Instant::now();
        let result = wait_for_connect_until(
            Duration::from_millis(20),
            |_| {
                std::thread::sleep(Duration::from_millis(35));
                Ok(true)
            },
            || Ok(()),
        );

        assert!(matches!(result, Err(VsockError::ConnectTimeout)));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn connect_wait_reports_socket_error_after_writable() {
        let result = wait_for_connect_until(
            Duration::from_millis(50),
            |_| Ok(true),
            || Err(std::io::Error::from_raw_os_error(111)),
        );

        assert!(matches!(result, Err(VsockError::ConnectFailed(_))));
    }

    #[test]
    fn connect_wait_accepts_success_before_deadline() {
        let result = wait_for_connect_until(Duration::from_millis(50), |_| Ok(true), || Ok(()));

        assert!(result.is_ok());
    }

    #[test]
    fn connect_timeout_is_capped_at_spec_limit() {
        assert_eq!(
            bounded_connect_timeout(Duration::MAX),
            Duration::from_secs(5)
        );
        assert_eq!(
            bounded_connect_timeout(Duration::from_millis(125)),
            Duration::from_millis(125)
        );
    }

    #[test]
    #[cfg(unix)]
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
    #[cfg(unix)]
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
    #[cfg(unix)]
    fn mock_vsock_stream_write_and_flush() {
        let (client, _server) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut stream = MockVsockStream(client);
        let data = b"hello vsock";
        let n = stream.write(data).unwrap();
        assert_eq!(n, data.len());
        assert!(stream.flush().is_ok());
    }

    #[test]
    #[cfg(unix)]
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

    #[cfg(not(windows))]
    #[test]
    fn listen_hyperv_returns_unsupported_off_windows() {
        let result = listen_hyperv([
            0, 0, 0x0a, 0xc9, 0xfa, 0xcb, 0x11, 0xe6, 0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79, 0x86,
            0xd3,
        ]);
        assert!(matches!(result, Err(VsockError::Unsupported)));
    }

    #[test]
    fn vsock_accept_timeout_is_bounded() {
        let started = Instant::now();
        let result = accept_until(Duration::from_millis(20), || {
            Ok::<_, VsockError>(None::<()>)
        });
        assert!(matches!(result, Err(VsockError::AcceptTimeout)));
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn vsock_accept_returns_first_connection() {
        let mut next = Some(42_u8);
        let result = accept_until(Duration::from_millis(50), || Ok(next.take()));
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn vsock_accept_propagates_socket_error() {
        let result = accept_until(Duration::from_millis(50), || {
            Err::<Option<()>, _>(VsockError::AcceptFailed("injected".into()))
        });
        assert!(matches!(result, Err(VsockError::AcceptFailed(message)) if message == "injected"));
    }

    #[test]
    fn hyperv_guid_uses_canonical_uuid_byte_order() {
        let bytes = [
            0x00, 0x00, 0x0a, 0xc9, 0xfa, 0xcb, 0x11, 0xe6, 0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79,
            0x86, 0xd3,
        ];
        assert_eq!(
            guid_components(bytes),
            (
                2761,
                0xfacb,
                0x11e6,
                [0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79, 0x86, 0xd3]
            )
        );
    }

    #[test]
    fn hyperv_linux_service_guid_requires_the_port_template() {
        let valid = [
            0x00, 0x00, 0x0a, 0xc9, 0xfa, 0xcb, 0x11, 0xe6, 0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79,
            0x86, 0xd3,
        ];
        assert_eq!(service_port_from_guid(valid), Ok(2761));

        let mut wrong_template = valid;
        wrong_template[4] = 0;
        assert!(service_port_from_guid(wrong_template).is_err());

        let no_port = [
            0, 0, 0, 0, 0xfa, 0xcb, 0x11, 0xe6, 0xbd, 0x58, 0x64, 0x00, 0x6a, 0x79, 0x86, 0xd3,
        ];
        assert!(service_port_from_guid(no_port).is_err());
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
