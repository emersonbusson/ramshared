use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, LookupAccountNameW, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    SECURITY_ATTRIBUTES, SID_NAME_USE, TOKEN_GROUPS, TOKEN_QUERY, TokenGroups,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, ImpersonateNamedPipeClient,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentThread, OpenThreadToken, WaitForSingleObject,
};

pub const PRODUCT_PIPE: &str = r"\\.\pipe\RamSharedBroker.v1";
pub const STATUS_PIPE: &str = r"\\.\pipe\RamSharedBrokerStatus.v1";
pub const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
pub const STATUS_BUFFER_BYTES: u32 = 4 * 1024;
pub const MAX_PIPE_INSTANCES: u32 = 4;

#[derive(Debug)]
pub enum PipeAuthError {
    Io(io::Error),
    Refused,
    Deadline,
    Stopping,
}

impl From<io::Error> for PipeAuthError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct OwnedSid(Vec<u8>);

impl OwnedSid {
    fn as_ptr(&self) -> PSID {
        self.0.as_ptr().cast_mut().cast()
    }
}

pub fn resolve_service_sid(account: &str) -> Result<OwnedSid, PipeAuthError> {
    let account: Vec<u16> = account.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sid_len = 0;
    let mut domain_len = 0;
    let mut usage: SID_NAME_USE = 0;
    unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            account.as_ptr(),
            std::ptr::null_mut(),
            &mut sid_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut usage,
        );
    }
    if sid_len == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut sid = vec![0u8; sid_len as usize];
    let mut domain = vec![0u16; domain_len as usize];
    if unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            account.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut usage,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(OwnedSid(sid))
}

struct LocalMemory(*mut std::ffi::c_void);

impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

fn sid_string(sid: &OwnedSid) -> Result<String, PipeAuthError> {
    let mut sid_string = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid.as_ptr(), &mut sid_string) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let sid_memory = LocalMemory(sid_string.cast());
    let mut len = 0usize;
    while unsafe { *sid_string.add(len) } != 0 {
        len += 1;
    }
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_string, len) });
    drop(sid_memory);
    Ok(value)
}

fn security_descriptor(
    owner: &OwnedSid,
    expected: &OwnedSid,
    status: bool,
) -> Result<LocalMemory, PipeAuthError> {
    let controller_sid = sid_string(owner)?;
    let expected_sid = sid_string(expected)?;
    let sddl = if status {
        format!("D:P(A;;RCWDWO;;;{controller_sid})(A;;GA;;;BA)(A;;GA;;;{expected_sid})")
    } else {
        format!("D:P(A;;RCWDWO;;;{controller_sid})(A;;GA;;;SY)(A;;GA;;;{expected_sid})")
    };
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(LocalMemory(descriptor.cast()))
}

pub struct PipeServer {
    handle: HANDLE,
    expected_sid: OwnedSid,
    verify_expected_sid: bool,
}

impl PipeServer {
    pub fn bind_product(
        owner_service_sid: &str,
        expected_service_sid: &str,
    ) -> Result<Self, PipeAuthError> {
        Self::bind(
            PRODUCT_PIPE,
            PIPE_BUFFER_BYTES,
            owner_service_sid,
            expected_service_sid,
            false,
        )
    }

    pub fn bind_status(
        owner_service_sid: &str,
        expected_service_sid: &str,
    ) -> Result<Self, PipeAuthError> {
        Self::bind(
            STATUS_PIPE,
            STATUS_BUFFER_BYTES,
            owner_service_sid,
            expected_service_sid,
            true,
        )
    }

    fn bind(
        name: &str,
        buffer: u32,
        owner_service_sid: &str,
        expected_service_sid: &str,
        status: bool,
    ) -> Result<Self, PipeAuthError> {
        let owner_sid = resolve_service_sid(owner_service_sid)?;
        let expected_sid = resolve_service_sid(expected_service_sid)?;
        let descriptor = security_descriptor(&owner_sid, &expected_sid, status)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                MAX_PIPE_INSTANCES,
                buffer,
                buffer,
                10_000,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        Ok(Self {
            handle,
            expected_sid,
            verify_expected_sid: !status,
        })
    }

    pub fn accept_authenticated(
        self,
        stop: &AtomicBool,
        deadline: Instant,
    ) -> Result<AuthenticatedPipe, PipeAuthError> {
        run_connect(self.handle, stop, deadline)?;
        let handle = self.handle;
        let expected_sid = if self.verify_expected_sid {
            // Ownership moves into the connected pipe; `self` is forgotten below.
            Some(unsafe { std::ptr::read(&self.expected_sid) })
        } else {
            None
        };
        std::mem::forget(self);
        Ok(AuthenticatedPipe {
            handle,
            expected_sid,
        })
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn run_connect(handle: HANDLE, stop: &AtomicBool, deadline: Instant) -> Result<(), PipeAuthError> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid handle").into());
    }
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    let event = Handle(event);
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.0;
    if unsafe { ConnectNamedPipe(handle, &mut overlapped) } == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(error as i32).into());
        }
    }
    loop {
        if stop.load(Ordering::Acquire) {
            cancel_and_wait(handle, &overlapped);
            return Err(PipeAuthError::Stopping);
        }
        if Instant::now() >= deadline {
            cancel_and_wait(handle, &overlapped);
            return Err(PipeAuthError::Deadline);
        }
        match unsafe { WaitForSingleObject(event.0, 100) } {
            WAIT_OBJECT_0 => break,
            WAIT_TIMEOUT => continue,
            _ => return Err(io::Error::last_os_error().into()),
        }
    }
    let mut transferred = 0;
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_OPERATION_ABORTED {
            return Err(io::Error::from_raw_os_error(error as i32).into());
        }
    }
    Ok(())
}

fn cancel_and_wait(handle: HANDLE, overlapped: &OVERLAPPED) {
    unsafe {
        CancelIoEx(handle, overlapped);
        let mut transferred = 0;
        GetOverlappedResult(handle, overlapped, &mut transferred, 1);
    }
}

fn verify_client_service_sid(pipe: HANDLE, expected: &OwnedSid) -> Result<(), PipeAuthError> {
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let _revert = RevertGuard;
    let mut token = std::ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let token = Handle(token);
    let mut required = 0;
    unsafe {
        GetTokenInformation(token.0, TokenGroups, std::ptr::null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut groups = vec![0u8; required as usize];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenGroups,
            groups.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let groups_ptr = groups.as_ptr().cast::<TOKEN_GROUPS>();
    let count = unsafe { (*groups_ptr).GroupCount } as usize;
    let first = unsafe { (*groups_ptr).Groups.as_ptr() };
    for index in 0..count {
        let group = unsafe { *first.add(index) };
        if group.Attributes & SE_GROUP_ENABLED as u32 != 0
            && group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0
            && unsafe { EqualSid(group.Sid, expected.as_ptr()) } != 0
        {
            return Ok(());
        }
    }
    Err(PipeAuthError::Refused)
}

struct RevertGuard;
impl Drop for RevertGuard {
    fn drop(&mut self) {
        unsafe {
            RevertToSelf();
        }
    }
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct AuthenticatedPipe {
    handle: HANDLE,
    expected_sid: Option<OwnedSid>,
}

impl AuthenticatedPipe {
    pub fn read_first_authenticated(&self, buffer: &mut [u8]) -> Result<usize, PipeAuthError> {
        let read = self
            .read_frame_deadline(buffer)
            .map_err(PipeAuthError::Io)?;
        if let Some(expected) = &self.expected_sid {
            verify_client_service_sid(self.handle, expected)?;
        }
        Ok(read)
    }

    pub fn read_first_authenticated_stoppable(
        &self,
        buffer: &mut [u8],
        stop: &AtomicBool,
    ) -> Result<usize, PipeAuthError> {
        let read = overlapped_io(
            self.handle,
            buffer.as_mut_ptr(),
            buffer.len(),
            false,
            Some(stop),
        )
        .map_err(PipeAuthError::Io)?;
        if let Some(expected) = &self.expected_sid {
            verify_client_service_sid(self.handle, expected)?;
        }
        Ok(read)
    }

    pub fn read_frame_deadline(&self, buffer: &mut [u8]) -> io::Result<usize> {
        overlapped_io(self.handle, buffer.as_mut_ptr(), buffer.len(), false, None)
    }

    pub fn read_frame_stoppable(&self, buffer: &mut [u8], stop: &AtomicBool) -> io::Result<usize> {
        overlapped_io(
            self.handle,
            buffer.as_mut_ptr(),
            buffer.len(),
            false,
            Some(stop),
        )
    }

    pub fn write_frame_deadline(&self, buffer: &[u8]) -> io::Result<usize> {
        overlapped_io(
            self.handle,
            buffer.as_ptr().cast_mut(),
            buffer.len(),
            true,
            None,
        )
    }

    pub fn cancel_and_quiesce(&self) {
        unsafe {
            CancelIoEx(self.handle, std::ptr::null());
        }
    }
}

impl Drop for AuthenticatedPipe {
    fn drop(&mut self) {
        unsafe {
            DisconnectNamedPipe(self.handle);
            CloseHandle(self.handle);
        }
    }
}

fn overlapped_io(
    handle: HANDLE,
    buffer: *mut u8,
    len: usize,
    write: bool,
    stop: Option<&AtomicBool>,
) -> io::Result<usize> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid handle"));
    }
    let length = u32::try_from(len).unwrap_or(u32::MAX);
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if event.is_null() {
        return Err(io::Error::last_os_error());
    }
    let event = Handle(event);
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.0;
    let started = unsafe {
        if write {
            WriteFile(
                handle,
                buffer.cast_const(),
                length,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        } else {
            ReadFile(
                handle,
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
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match unsafe { WaitForSingleObject(event.0, 100) } {
            WAIT_OBJECT_0 => break,
            WAIT_TIMEOUT if stop.is_some_and(|flag| flag.load(Ordering::Acquire)) => {
                cancel_and_wait(handle, &overlapped);
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "named-pipe operation stopped",
                ));
            }
            WAIT_TIMEOUT if Instant::now() < deadline => continue,
            _ => {
                cancel_and_wait(handle, &overlapped);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "named-pipe frame deadline",
                ));
            }
        }
    }
    let mut transferred = 0;
    if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(transferred as usize)
}
