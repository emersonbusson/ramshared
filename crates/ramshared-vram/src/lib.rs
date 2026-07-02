//! `ramshared-vram` — abstração de backend de VRAM (RF-G1, prep da P3).
//!
//! Separa o **plano de controle** de VRAM (ciclo de vida + alocação + wipe + free-floor) do
//! backend concreto (hoje CUDA; amanhã Vulkan). O **plano de dados** (I/O de bloco) já é
//! abstraído por `ramshared_block::BlockBackend`; aqui ficam só as operações específicas de VRAM.
//!
//! Sem `unsafe` e sem dependência de driver: o trait é hardware-agnóstico. A impl CUDA vive no
//! `ramshared-cuda` (que re-exporta os tipos + impl); um futuro `ramshared-vulkan` faria o mesmo.
//!
//! SPEC: docs/vram-provider/SPEC.md.
#![forbid(unsafe_code)]

use std::fmt;

/// Erro de uma operação de VRAM (mapeado do erro do backend; ex.: `CudaError`).
#[derive(Debug)]
pub enum VramError {
    /// Falha do backend: init/driver/alocação (mensagem do backend).
    Provider(String),
    /// Acesso fora da região alocada.
    OutOfRange { off: u64, len: u64, size: u64 },
}

impl fmt::Display for VramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VramError::Provider(m) => write!(f, "vram provider: {m}"),
            VramError::OutOfRange { off, len, size } => {
                write!(f, "vram out-of-range: off={off} len={len} size={size}")
            }
        }
    }
}

impl std::error::Error for VramError {}

/// Uma região de VRAM alocada. Operações síncronas (o `zero` faz wipe + sincroniza, DT-17/§11).
///
/// **Afinidade de thread:** a impl pode ser thread-local (CUDA é). Use na mesma thread que a
/// alocou — por isso o daemon roda todo I/O de VRAM numa thread só. Por isso o trait NÃO exige `Send`.
pub trait VramMemory {
    /// Tamanho da região em bytes.
    fn len(&self) -> usize;
    /// `true` se a região tem 0 bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Zera toda a região (wipe seguro + sincroniza). DT-17/§11.
    fn zero(&mut self) -> Result<(), VramError>;
    /// Lê `dst.len()` bytes a partir de `off`.
    fn read_at(&self, off: u64, dst: &mut [u8]) -> Result<(), VramError>;
    /// Escreve `src` a partir de `off`.
    fn write_at(&mut self, off: u64, src: &[u8]) -> Result<(), VramError>;
}

/// Provedor de VRAM (contexto thread-afim já criado): aloca regiões e reporta free/total.
///
/// O ciclo de vida (load do driver + seleção de device + criação de contexto) é responsabilidade
/// do construtor concreto do backend (ex.: `Cuda::load()` + `create_context()`), pois difere por
/// backend; o daemon recebe um provider pronto e fala só por este trait no caminho genérico.
pub trait VramProvider {
    /// Tipo da região alocada (GAT: empresta `&self`, preservando a afinidade de thread sem `Arc`).
    type Mem<'p>: VramMemory
    where
        Self: 'p;

    /// Reserva `bytes` de VRAM. A região é liberada quando cai (RAII).
    fn alloc(&self, bytes: usize) -> Result<Self::Mem<'_>, VramError>;

    /// VRAM livre/total em bytes (free-floor da residência — DT-3/9/11).
    fn mem_info(&self) -> Result<(u64, u64), VramError>;
}
