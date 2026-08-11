#[cfg(windows)]
use std::io;
use std::time::{Duration, Instant};

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
}
