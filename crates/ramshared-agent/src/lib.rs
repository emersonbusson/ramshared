//! Biblioteca do agente RamShared (tenant): coleta de pressão (PSI), execução de swap
//! sobre NBD e watchdog de sessão. A lógica pura (parsing de `/proc`, montagem de argv,
//! janela do watchdog) vive aqui e é coberta por testes unitários; o `main.rs` apenas
//! costura essas peças com os sockets/threads (DT-27).
//!
//! SPEC: docs/memory-broker/SPECv2.md (ITEM-9). Sem `unsafe`.
#![forbid(unsafe_code)]

pub mod psi;
pub mod swap;
pub mod watchdog;
