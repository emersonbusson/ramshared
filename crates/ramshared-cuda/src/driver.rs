//! Wrappers seguros (RAII) sobre a CUDA Driver API. SPEC §4, §8.
//!
//! Modelo de posse:
//! - [`Cuda`] possui o handle `dlopen` + a tabela de símbolos (vive mais que tudo).
//! - [`Context`] empresta `&Cuda`; faz `cuCtxDestroy` no `Drop`.
//! - [`DeviceMem`] empresta `&Context`; faz `cuMemFree` no `Drop`.
//!
//! A ordem de `Drop` garante a ordem inversa de alocação exigida pelo CUDA
//! (liberar memória → destruir contexto → `dlclose`), o idioma `goto out_err` do
//! kernel traduzido para o borrow checker.

use core::ffi::{CStr, c_char, c_void};
use core::fmt;

use crate::ffi::{CUDA_SUCCESS, CuContext, CuDevice, CuDevicePtr, CuResult, Syms};

/// Erros da camada CUDA. Sem `panic`/`unwrap` em produção (regra `coding.md`).
#[derive(Debug)]
pub enum CudaError {
    /// `dlopen` não encontrou nenhuma `libcuda`.
    Load(String),
    /// `dlsym` falhou para um símbolo obrigatório.
    Symbol(String),
    /// Uma chamada da Driver API retornou erro.
    Driver {
        op: &'static str,
        code: i32,
        msg: String,
    },
    /// Acesso fora da faixa da região de VRAM (offset+len > tamanho).
    OutOfRange { off: usize, len: usize, size: usize },
}

impl fmt::Display for CudaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CudaError::Load(s) => write!(f, "falha ao carregar libcuda: {s}"),
            CudaError::Symbol(s) => write!(f, "símbolo CUDA ausente: {s}"),
            CudaError::Driver { op, code, msg } => {
                write!(f, "{op} falhou (CUresult={code}): {msg}")
            }
            CudaError::OutOfRange { off, len, size } => {
                write!(f, "acesso fora da faixa: off={off} len={len} > size={size}")
            }
        }
    }
}

impl core::error::Error for CudaError {}

/// Handle RAII do `dlopen`: faz `dlclose` no `Drop`.
struct Lib(*mut c_void);

impl Drop for Lib {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: self.0 veio de um open bem-sucedido e não foi fechado antes.
            unsafe { crate::loader::close(self.0) };
        }
    }
}

/// CUDA carregada e inicializada (`cuInit(0)`).
pub struct Cuda {
    _lib: Lib,
    syms: Syms,
}

#[cfg(unix)]
const CANDIDATES: &[&CStr] = &[
    c"libcuda.so.1",
    c"/usr/lib/wsl/lib/libcuda.so.1",
    c"libcuda.so",
    c"/usr/lib/wsl/lib/libcuda.so",
    c"/usr/lib/x86_64-linux-gnu/libcuda.so.1",
];

#[cfg(windows)]
const CANDIDATES: &[&CStr] = &[
    c"nvcuda.dll",
];

impl Cuda {
    /// Carrega a `libcuda` (candidatos WSL2 + padrão) e roda `cuInit(0)`.
    pub fn load() -> Result<Self, CudaError> {
        let mut handle: *mut c_void = core::ptr::null_mut();
        for cand in CANDIDATES {
            // SAFETY: cand é um &CStr válido (null-terminated).
            let h = unsafe { crate::loader::open(cand.as_ptr()) };
            if !h.is_null() {
                handle = h;
                break;
            }
        }
        if handle.is_null() {
            return Err(CudaError::Load(crate::loader::error()));
        }
        let lib = Lib(handle);

        // SAFETY: handle é uma libcuda aberta; cada símbolo é uma fn da Driver API
        // com a assinatura declarada em ffi.rs.
        let syms = unsafe {
            Syms {
                init: load_sym(handle, c"cuInit")?,
                device_get_count: load_sym(handle, c"cuDeviceGetCount")?,
                device_get: load_sym(handle, c"cuDeviceGet")?,
                device_get_name: load_sym(handle, c"cuDeviceGetName")?,
                ctx_create: load_sym(handle, c"cuCtxCreate_v2")?,
                ctx_destroy: load_sym(handle, c"cuCtxDestroy_v2")?,
                ctx_synchronize: load_sym(handle, c"cuCtxSynchronize")?,
                mem_alloc: load_sym(handle, c"cuMemAlloc_v2")?,
                mem_free: load_sym(handle, c"cuMemFree_v2")?,
                memcpy_htod: load_sym(handle, c"cuMemcpyHtoD_v2")?,
                memcpy_dtoh: load_sym(handle, c"cuMemcpyDtoH_v2")?,
                memset_d8: load_sym(handle, c"cuMemsetD8_v2")?,
                mem_get_info: load_sym(handle, c"cuMemGetInfo_v2")?,
                get_error_string: load_sym_opt(handle, c"cuGetErrorString"),
            }
        };

        // SAFETY: símbolo init resolvido acima.
        let r = unsafe { (syms.init)(0) };
        check(&syms, r, "cuInit")?;

        Ok(Cuda { _lib: lib, syms })
    }

    /// Número de devices CUDA visíveis.
    pub fn device_count(&self) -> Result<i32, CudaError> {
        let mut count: i32 = 0;
        // SAFETY: count é out-param válido.
        let r = unsafe { (self.syms.device_get_count)(&mut count) };
        check(&self.syms, r, "cuDeviceGetCount")?;
        Ok(count)
    }

    /// Obtém o device de índice `ordinal` (0 = primeiro), com nome.
    pub fn device(&self, ordinal: i32) -> Result<Device, CudaError> {
        let mut raw: CuDevice = 0;
        // SAFETY: raw é out-param válido.
        let r = unsafe { (self.syms.device_get)(&mut raw, ordinal) };
        check(&self.syms, r, "cuDeviceGet")?;

        let mut buf = [0_i8; 128];
        // SAFETY: buf comporta `len` bytes; a API escreve C-string terminada.
        let r = unsafe {
            (self.syms.device_get_name)(buf.as_mut_ptr() as *mut c_char, buf.len() as i32, raw)
        };
        check(&self.syms, r, "cuDeviceGetName")?;
        buf[buf.len() - 1] = 0; // garante terminação mesmo se a API preencher os 128 bytes
        // SAFETY: buf preenchido pela API e com NUL final garantido acima.
        let name = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned();

        Ok(Device { raw, name })
    }

    /// Cria um contexto CUDA no device (vira corrente na thread atual).
    pub fn create_context<'a>(&'a self, device: &Device) -> Result<Context<'a>, CudaError> {
        let mut raw: CuContext = core::ptr::null_mut();
        // SAFETY: raw é out-param; device.raw é um CUdevice válido.
        let r = unsafe { (self.syms.ctx_create)(&mut raw, 0, device.raw) };
        check(&self.syms, r, "cuCtxCreate")?;
        Ok(Context { cuda: self, raw })
    }
}

/// Um device CUDA (ordinal + nome).
#[derive(Clone, Debug)]
pub struct Device {
    raw: CuDevice,
    name: String,
}

impl Device {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Contexto CUDA. `Drop` faz `cuCtxDestroy`.
///
/// **Afinidade de thread:** a corrente do contexto CUDA é *thread-local*. Use este
/// `Context` (e os `DeviceMem` derivados) **na mesma thread** que o criou — por isso
/// o daemon roda todo o I/O de VRAM numa única thread. Chamar de outra thread exigiria
/// `cuCtxSetCurrent` (não feito aqui). A thread de DEMOTE só roda `swapoff`, não CUDA.
pub struct Context<'a> {
    cuda: &'a Cuda,
    raw: CuContext,
}

impl<'a> Context<'a> {
    /// VRAM livre/total em bytes (`cuMemGetInfo`).
    pub fn mem_info(&self) -> Result<(usize, usize), CudaError> {
        let (mut free, mut total) = (0_usize, 0_usize);
        // SAFETY: ambos out-params válidos; contexto corrente.
        let r = unsafe { (self.cuda.syms.mem_get_info)(&mut free, &mut total) };
        check(&self.cuda.syms, r, "cuMemGetInfo")?;
        Ok((free, total))
    }

    /// Reserva `bytes` de VRAM. A região é liberada quando o `DeviceMem` cai.
    pub fn alloc(&self, bytes: usize) -> Result<DeviceMem<'_, 'a>, CudaError> {
        let mut ptr: CuDevicePtr = 0;
        // SAFETY: ptr é out-param; contexto corrente.
        let r = unsafe { (self.cuda.syms.mem_alloc)(&mut ptr, bytes) };
        check(&self.cuda.syms, r, "cuMemAlloc")?;
        Ok(DeviceMem {
            ctx: self,
            ptr,
            len: bytes,
        })
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        // SAFETY: raw veio de cuCtxCreate e não foi destruído antes. Best-effort.
        unsafe {
            let _ = (self.cuda.syms.ctx_destroy)(self.raw);
        }
    }
}

/// Região de VRAM. `Drop` faz `cuMemFree`. Empresta o [`Context`] (libera antes).
pub struct DeviceMem<'c, 'a> {
    ctx: &'c Context<'a>,
    ptr: CuDevicePtr,
    len: usize,
}

impl DeviceMem<'_, '_> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Zera toda a região (`cuMemsetD8` + sincroniza). SPEC §6.2/§11 (zerar VRAM).
    pub fn zero(&mut self) -> Result<(), CudaError> {
        let syms = &self.ctx.cuda.syms;
        // SAFETY: ptr/len descrevem a região alocada por este DeviceMem.
        let r = unsafe { (syms.memset_d8)(self.ptr, 0, self.len) };
        check(syms, r, "cuMemsetD8")?;
        // SAFETY: sem args.
        let r = unsafe { (syms.ctx_synchronize)() };
        check(syms, r, "cuCtxSynchronize")
    }

    /// Copia `src` para a VRAM em `off` (Host→Device, síncrono).
    pub fn write_at(&mut self, off: usize, src: &[u8]) -> Result<(), CudaError> {
        self.bounds(off, src.len())?;
        let syms = &self.ctx.cuda.syms;
        // SAFETY: faixa validada por bounds(); src é um slice válido de src.len() bytes.
        let r = unsafe {
            (syms.memcpy_htod)(
                self.ptr + off as u64,
                src.as_ptr() as *const c_void,
                src.len(),
            )
        };
        check(syms, r, "cuMemcpyHtoD")
    }

    /// Copia da VRAM em `off` para `dst` (Device→Host, síncrono).
    pub fn read_at(&self, off: usize, dst: &mut [u8]) -> Result<(), CudaError> {
        self.bounds(off, dst.len())?;
        let syms = &self.ctx.cuda.syms;
        // SAFETY: faixa validada; dst é um slice mutável válido de dst.len() bytes.
        let r = unsafe {
            (syms.memcpy_dtoh)(
                dst.as_mut_ptr() as *mut c_void,
                self.ptr + off as u64,
                dst.len(),
            )
        };
        check(syms, r, "cuMemcpyDtoH")
    }

    fn bounds(&self, off: usize, len: usize) -> Result<(), CudaError> {
        match off.checked_add(len) {
            Some(end) if end <= self.len => Ok(()),
            _ => Err(CudaError::OutOfRange {
                off,
                len,
                size: self.len,
            }),
        }
    }
}

impl Drop for DeviceMem<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: ptr veio de cuMemAlloc deste DeviceMem e não foi liberado antes.
        unsafe {
            let _ = (self.ctx.cuda.syms.mem_free)(self.ptr);
        }
    }
}

// --- helpers internos ---

/// SAFETY: `handle` é uma libcuda aberta; `name` é um símbolo cujo tipo `T` é um
/// ponteiro de função C de tamanho de ponteiro.
unsafe fn load_sym<T: Copy>(handle: *mut c_void, name: &CStr) -> Result<T, CudaError> {
    // SAFETY: contrato da função (handle válido, name null-terminated).
    let sym = unsafe { crate::loader::sym(handle, name.as_ptr()) };
    if sym.is_null() {
        return Err(CudaError::Symbol(name.to_string_lossy().into_owned()));
    }
    const {
        assert!(core::mem::size_of::<T>() == core::mem::size_of::<*mut c_void>());
    }
    // SAFETY: T é fn-pointer C do mesmo tamanho de um data pointer; materializa o endereço.
    Ok(unsafe { core::mem::transmute_copy::<*mut c_void, T>(&sym) })
}

/// Versão opcional (símbolo pode faltar em stubs antigas).
fn load_sym_opt<T: Copy>(handle: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: mesmas pré-condições de load_sym.
    unsafe { load_sym(handle, name).ok() }
}

fn check(syms: &Syms, r: CuResult, op: &'static str) -> Result<(), CudaError> {
    if r == CUDA_SUCCESS {
        Ok(())
    } else {
        Err(CudaError::Driver {
            op,
            code: r,
            msg: err_string(syms, r),
        })
    }
}

fn err_string(syms: &Syms, r: CuResult) -> String {
    if let Some(f) = syms.get_error_string {
        let mut p: *const c_char = core::ptr::null();
        // SAFETY: f é cuGetErrorString resolvida; p é out-param válido.
        unsafe { f(r, &mut p) };
        if !p.is_null() {
            // SAFETY: p aponta para uma string estática da CUDA terminada em nul.
            return unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        }
    }
    format!("CUresult={r}")
}

// dl_error foi removida pois o erro agora vem de crate::loader::error().
