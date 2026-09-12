#[cfg(windows)]
use std::io;
use std::time::{Duration, Instant};

/// Magic bytes identifying a ramshared IPC message ('RAMS').
pub const IPC_MAGIC: u32 = 0x52414D53; // 'RAMS'
/// Legacy IPC protocol version.
pub const IPC_VERSION_1: u32 = 1;
/// Current IPC protocol version with flags support.
pub const IPC_VERSION_2: u32 = 2;
/// Defense-in-depth maximum payload size limit (1MB).
pub const MAX_PAYLOAD_LEN: u32 = 1024 * 1024; // 1MB defense-in-depth

/// Backward-compatible IPC message version header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcMessageHeader {
    /// Magic identifier.
    pub magic: u32,
    /// Protocol version.
    pub version: u32,
    /// Payload length in bytes.
    pub payload_len: u32,
    /// Protocol flags (V2+ only).
    pub flags: u32,
}

/// Errors encountered during IPC message deserialization.
#[derive(Debug, PartialEq, Eq)]
pub enum IpcDeserializeError {
    /// Underlying IO error.
    Io(std::io::ErrorKind),
    /// Invalid magic bytes received.
    InvalidMagic(u32),
    /// Protocol version is outside the supported range.
    UnsupportedVersion(u32),
    /// Claimed payload length exceeds `MAX_PAYLOAD_LEN`.
    PayloadTooLarge(u32),
    /// Stream ended prematurely before header could be read.
    IncompleteMessage,
    /// Graceful stream disconnect.
    Disconnect,
}

impl std::fmt::Display for IpcDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(k) => write!(f, "IO error: {:?}", k),
            Self::InvalidMagic(m) => write!(f, "Invalid magic bytes: {:#X}", m),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported IPC version: {}", v),
            Self::PayloadTooLarge(l) => write!(f, "Payload too large: {} bytes", l),
            Self::IncompleteMessage => write!(f, "Incomplete message header"),
            Self::Disconnect => write!(f, "Client disconnected gracefully"),
        }
    }
}

impl std::error::Error for IpcDeserializeError {}

impl IpcMessageHeader {
    /// Creates a new IPC message header for the specified version.
    pub fn new(version: u32, payload_len: u32) -> Self {
        Self {
            magic: IPC_MAGIC,
            version,
            payload_len,
            flags: 0,
        }
    }

    /// Deserializes the header from a byte stream, handling backward compatibility.
    pub fn read_from<R: std::io::Read>(mut reader: R) -> Result<Self, IpcDeserializeError> {
        let mut first_byte = [0u8; 1];
        match reader.read_exact(&mut first_byte) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(IpcDeserializeError::Disconnect);
            }
            Err(e) => return Err(IpcDeserializeError::Io(e.kind())),
        }

        let mut remaining_magic = [0u8; 3];
        if let Err(e) = reader.read_exact(&mut remaining_magic) {
            return match e.kind() {
                std::io::ErrorKind::UnexpectedEof => Err(IpcDeserializeError::IncompleteMessage),
                _ => Err(IpcDeserializeError::Io(e.kind())),
            };
        }

        let mut magic_bytes = [0u8; 4];
        magic_bytes[0] = first_byte[0];
        magic_bytes[1..].copy_from_slice(&remaining_magic);
        let magic = u32::from_le_bytes(magic_bytes);
        if magic != IPC_MAGIC {
            return Err(IpcDeserializeError::InvalidMagic(magic));
        }

        let mut version_bytes = [0u8; 4];
        if let Err(e) = reader.read_exact(&mut version_bytes) {
            return match e.kind() {
                std::io::ErrorKind::UnexpectedEof => Err(IpcDeserializeError::IncompleteMessage),
                _ => Err(IpcDeserializeError::Io(e.kind())),
            };
        }
        let version = u32::from_le_bytes(version_bytes);
        if !(IPC_VERSION_1..=IPC_VERSION_2).contains(&version) {
            return Err(IpcDeserializeError::UnsupportedVersion(version));
        }

        let mut len_bytes = [0u8; 4];
        if let Err(e) = reader.read_exact(&mut len_bytes) {
            return match e.kind() {
                std::io::ErrorKind::UnexpectedEof => Err(IpcDeserializeError::IncompleteMessage),
                _ => Err(IpcDeserializeError::Io(e.kind())),
            };
        }
        let payload_len = u32::from_le_bytes(len_bytes);
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(IpcDeserializeError::PayloadTooLarge(payload_len));
        }

        let mut flags = 0;
        if version >= IPC_VERSION_2 {
            let mut flags_bytes = [0u8; 4];
            if let Err(e) = reader.read_exact(&mut flags_bytes) {
                return match e.kind() {
                    std::io::ErrorKind::UnexpectedEof => {
                        Err(IpcDeserializeError::IncompleteMessage)
                    }
                    _ => Err(IpcDeserializeError::Io(e.kind())),
                };
            }
            flags = u32::from_le_bytes(flags_bytes);
        }

        Ok(Self {
            magic,
            version,
            payload_len,
            flags,
        })
    }

    /// Serializes the header to a byte stream.
    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        writer.write_all(&self.magic.to_le_bytes())?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.payload_len.to_le_bytes())?;
        if self.version >= IPC_VERSION_2 {
            writer.write_all(&self.flags.to_le_bytes())?;
        }
        Ok(())
    }
}

pub trait BrokerStream: std::io::BufRead + std::io::Write {}

#[derive(Debug, PartialEq, Eq)]
pub enum BrokerConnectError {
    Deadline,
    NonTransient(i32),
}

pub fn retryable_pipe_error(code: i32) -> bool {
    matches!(code, 2 | 231)
}

pub fn retry_until(
    deadline: Instant,
    mut open: impl FnMut() -> Result<(), i32>,
) -> Result<(), BrokerConnectError> {
    loop {
        match open() {
            Ok(()) => return Ok(()),
            Err(code) if retryable_pipe_error(code) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(code) if retryable_pipe_error(code) => return Err(BrokerConnectError::Deadline),
            Err(code) => return Err(BrokerConnectError::NonTransient(code)),
        }
    }
}

#[cfg(windows)]
pub struct NamedPipeBrokerStream {
    reader: std::io::BufReader<OwnedPipeHandle>,
    writer: OwnedPipeHandle,
}

#[cfg(windows)]
impl NamedPipeBrokerStream {
    pub fn connect_product_pipe(deadline: Instant) -> Result<Self, BrokerConnectError> {
        Self::connect_named_pipe(r"\\.\pipe\RamSharedBroker.v1", deadline)
    }

    pub fn connect_status_pipe(deadline: Instant) -> Result<Self, BrokerConnectError> {
        Self::connect_named_pipe(r"\\.\pipe\RamSharedBrokerStatus.v1", deadline)
    }

    fn connect_named_pipe(name: &str, deadline: Instant) -> Result<Self, BrokerConnectError> {
        use windows_sys::Win32::Foundation::{
            GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
        };
        let mut opened = None;
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        retry_until(deadline, || {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_NONE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                Err(unsafe { GetLastError() } as i32)
            } else {
                opened = Some(OwnedPipeHandle(handle));
                Ok(())
            }
        })?;
        let writer = opened.take().ok_or(BrokerConnectError::Deadline)?;
        let reader = std::io::BufReader::new(writer.try_clone().map_err(|error| {
            BrokerConnectError::NonTransient(error.raw_os_error().unwrap_or(-1))
        })?);
        Ok(Self { reader, writer })
    }
}

#[cfg(windows)]
struct OwnedPipeHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl OwnedPipeHandle {
    fn try_clone(&self) -> io::Result<Self> {
        use windows_sys::Win32::Foundation::DuplicateHandle;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let process = unsafe { GetCurrentProcess() };
        let mut clone = std::ptr::null_mut();
        let ok = unsafe { DuplicateHandle(process, self.0, process, &mut clone, 0, 0, 2) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(clone))
        }
    }

    fn overlapped_io(&self, buffer: *mut u8, length: u32, write: bool) -> io::Result<u32> {
        use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
        use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
        use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = OwnedPipeHandle(event);
        let mut overlapped = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let started = unsafe {
            if write {
                WriteFile(
                    self.0,
                    buffer.cast_const(),
                    length,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            } else {
                ReadFile(
                    self.0,
                    buffer,
                    length,
                    std::ptr::null_mut(),
                    &mut overlapped,
                )
            }
        };
        if started == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                return Err(error);
            }
        }
        match unsafe { WaitForSingleObject(event.0, 10_000) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                unsafe {
                    CancelIoEx(self.0, &overlapped);
                    let mut ignored = 0;
                    GetOverlappedResult(self.0, &overlapped, &mut ignored, 1);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "named-pipe frame deadline",
                ));
            }
            _ => return Err(io::Error::last_os_error()),
        }
        let mut transferred = 0;
        if unsafe { GetOverlappedResult(self.0, &overlapped, &mut transferred, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred)
    }
}

#[cfg(windows)]
impl Drop for OwnedPipeHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
impl std::io::Read for OwnedPipeHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let length = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        self.overlapped_io(buf.as_mut_ptr(), length, false)
            .map(|n| n as usize)
    }
}

#[cfg(windows)]
impl std::io::Write for OwnedPipeHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let length = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        self.overlapped_io(buf.as_ptr().cast_mut(), length, true)
            .map(|n| n as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
impl std::io::Read for NamedPipeBrokerStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}
#[cfg(windows)]
impl std::io::BufRead for NamedPipeBrokerStream {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.reader.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.reader.consume(amount)
    }
}
#[cfg(windows)]
impl std::io::Write for NamedPipeBrokerStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
#[cfg(windows)]
impl BrokerStream for NamedPipeBrokerStream {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn only_not_found_and_busy_retry() {
        assert!(retryable_pipe_error(2));
        assert!(retryable_pipe_error(231));
        assert!(!retryable_pipe_error(5));
    }
    #[test]
    fn deadline_stops_retry() {
        let result = retry_until(Instant::now(), || Err(2));
        assert_eq!(result, Err(BrokerConnectError::Deadline));
    }
    #[cfg(windows)]
    #[test]
    fn connect_product_pipe_deadline_stops_retry() {
        let result = NamedPipeBrokerStream::connect_product_pipe(Instant::now());
        assert!(matches!(
            result,
            Err(BrokerConnectError::Deadline) | Err(BrokerConnectError::NonTransient(_))
        ));
    }
    #[cfg(windows)]
    #[test]
    fn connect_status_pipe_deadline_stops_retry() {
        let result = NamedPipeBrokerStream::connect_status_pipe(Instant::now());
        assert!(matches!(
            result,
            Err(BrokerConnectError::Deadline) | Err(BrokerConnectError::NonTransient(_))
        ));
    }

    #[test]
    fn ipc_header_serialization_v1() {
        let header = IpcMessageHeader::new(IPC_VERSION_1, 42);
        let mut buffer = Vec::new();
        header.write_to(&mut buffer).unwrap();
        assert_eq!(buffer.len(), 12);

        let read_header = IpcMessageHeader::read_from(&buffer[..]).unwrap();
        assert_eq!(read_header.magic, IPC_MAGIC);
        assert_eq!(read_header.version, IPC_VERSION_1);
        assert_eq!(read_header.payload_len, 42);
        assert_eq!(read_header.flags, 0); // V1 doesn't have flags on wire, defaults to 0
    }

    #[test]
    fn ipc_header_serialization_v2() {
        let mut header = IpcMessageHeader::new(IPC_VERSION_2, 42);
        header.flags = 0xDEADBEEF;
        let mut buffer = Vec::new();
        header.write_to(&mut buffer).unwrap();
        assert_eq!(buffer.len(), 16);

        let read_header = IpcMessageHeader::read_from(&buffer[..]).unwrap();
        assert_eq!(read_header.magic, IPC_MAGIC);
        assert_eq!(read_header.version, IPC_VERSION_2);
        assert_eq!(read_header.payload_len, 42);
        assert_eq!(read_header.flags, 0xDEADBEEF);
    }

    #[test]
    fn ipc_header_backward_compatibility() {
        // V1 payload
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IPC_MAGIC.to_le_bytes());
        buffer.extend_from_slice(&IPC_VERSION_1.to_le_bytes());
        buffer.extend_from_slice(&100u32.to_le_bytes());

        // Reading a V1 payload using the general read_from (which supports up to V2)
        let read_header = IpcMessageHeader::read_from(&buffer[..]).unwrap();
        assert_eq!(read_header.version, IPC_VERSION_1);
        assert_eq!(read_header.payload_len, 100);
        assert_eq!(read_header.flags, 0);
    }

    #[test]
    fn ipc_header_invalid_magic() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&0xBADBADu32.to_le_bytes());
        buffer.extend_from_slice(&IPC_VERSION_1.to_le_bytes());
        buffer.extend_from_slice(&100u32.to_le_bytes());

        let err = IpcMessageHeader::read_from(&buffer[..]).unwrap_err();
        assert_eq!(err, IpcDeserializeError::InvalidMagic(0xBADBAD));
    }

    #[test]
    fn ipc_header_unsupported_version() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IPC_MAGIC.to_le_bytes());
        buffer.extend_from_slice(&999u32.to_le_bytes());
        buffer.extend_from_slice(&100u32.to_le_bytes());

        let err = IpcMessageHeader::read_from(&buffer[..]).unwrap_err();
        assert_eq!(err, IpcDeserializeError::UnsupportedVersion(999));
    }

    #[test]
    fn ipc_header_payload_too_large() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&IPC_MAGIC.to_le_bytes());
        buffer.extend_from_slice(&IPC_VERSION_1.to_le_bytes());
        buffer.extend_from_slice(&(MAX_PAYLOAD_LEN + 1).to_le_bytes());

        let err = IpcMessageHeader::read_from(&buffer[..]).unwrap_err();
        assert_eq!(
            err,
            IpcDeserializeError::PayloadTooLarge(MAX_PAYLOAD_LEN + 1)
        );
    }

    #[test]
    fn ipc_header_incomplete_message() {
        let buffer = IPC_MAGIC.to_le_bytes(); // Only magic, missing version and len

        let err = IpcMessageHeader::read_from(&buffer[..]).unwrap_err();
        assert_eq!(err, IpcDeserializeError::IncompleteMessage);
    }
}
