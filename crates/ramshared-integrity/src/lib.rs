//! ramshared-integrity — verificação de integridade por bloco (SPEC §8.1, §14.2).
//!
//! Para o modo `--debug-checksum`: hash não-cripto rápido + tabela de checksum
//! pré-alocada por índice de bloco (detecta corrupção/leitura torn na VRAM) e
//! padrões reprodutíveis para o `test-integrity`. Lógica pura, sem root.
#![forbid(unsafe_code)]

pub mod hash;
pub mod pattern;

pub use hash::{ChecksumTable, block_hash};
pub use pattern::{Pattern, fill_block, verify_block};
