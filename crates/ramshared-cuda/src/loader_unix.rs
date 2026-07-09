//! Loader específico para sistemas Unix (usando `libdl`).
//!
//! Requisitos cobertos: RF-4, DT-5.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CStr;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 2;

/// Abre a biblioteca dinâmica especificada.
///
/// # Safety
/// O ponteiro `filename` deve apontar para uma C-string válida (null-terminated).
pub unsafe fn open(filename: *const c_char) -> *mut c_void {
    unsafe { dlopen(filename, RTLD_NOW) }
}

/// Carrega o símbolo especificado da biblioteca.
///
/// # Safety
/// `handle` deve ser um ponteiro válido retornado por `open`.
/// `symbol` deve ser um ponteiro para uma C-string válida terminada em nul.
pub unsafe fn sym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    unsafe { dlsym(handle, symbol) }
}

/// Fecha a biblioteca dinâmica.
///
/// # Safety
/// `handle` deve ser um ponteiro válido retornado por `open` que ainda não foi fechado.
pub unsafe fn close(handle: *mut c_void) -> c_int {
    unsafe { dlclose(handle) }
}

/// Retorna a última mensagem de erro gerada pelo loader.
pub fn error() -> String {
    let err = unsafe { dlerror() };
    if err.is_null() {
        "unknown dlopen error".to_string()
    } else {
        // SAFETY: err é garantido pelo sistema como uma C-string estática válida.
        unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned()
    }
}
