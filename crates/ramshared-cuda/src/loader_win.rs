//! Loader específico para sistemas Windows (usando APIs Win32).
//!
//! Requisitos cobertos: RF-4, DT-5.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CStr;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::LibraryLoader::{FreeLibrary, GetProcAddress, LoadLibraryW};

/// Abre a biblioteca dinâmica especificada (converte do formato CStr para UTF-16).
///
/// # Safety
/// O ponteiro `filename` deve apontar para uma C-string válida (null-terminated).
pub unsafe fn open(filename: *const c_char) -> *mut c_void {
    let cstr = unsafe { CStr::from_ptr(filename) };
    let string_str = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return core::ptr::null_mut(),
    };

    // Converte para UTF-16 wide string terminada em NUL
    let wide: Vec<u16> = string_str.encode_utf16().chain(Some(0)).collect();

    let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
    handle as *mut c_void
}

/// Carrega o símbolo especificado da biblioteca.
///
/// # Safety
/// `handle` deve ser um ponteiro válido retornado por `open`.
/// `symbol` deve ser um ponteiro para uma C-string válida terminada em nul.
pub unsafe fn sym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    if handle.is_null() {
        return core::ptr::null_mut();
    }
    let addr = unsafe { GetProcAddress(handle as _, symbol as _) };
    addr as *mut c_void
}

/// Fecha a biblioteca dinâmica.
///
/// # Safety
/// `handle` deve ser um ponteiro válido retornado por `open` que ainda não foi fechado.
pub unsafe fn close(handle: *mut c_void) -> c_int {
    if handle.is_null() {
        return 0;
    }
    let ok = unsafe { FreeLibrary(handle as _) };
    if ok != 0 {
        0 // Retorna 0 para sucesso de fechamento, alinhado com dlclose
    } else {
        -1 // Falha
    }
}

/// Retorna a última mensagem de erro gerada pelo loader usando GetLastError().
pub fn error() -> String {
    let code = unsafe { GetLastError() };
    format!("Windows error code: 0x{:08X}", code)
}
