//! ramshared-wsl2d — daemon do tier VRAM (SPEC §4).
//!
//! Esqueleto. A sequência real (preflight → `mlockall` → `oom_score_adj` → CUDA
//! alloc com backoff → canário §9 → device NBD) entra nos próximos incrementos;
//! o núcleo já existe: `VramBackend` (lib) + `ramshared-block` + `ramshared-cuda`.

fn main() {
    eprintln!(
        "ramshared-wsl2d (esqueleto): orquestre via a CLI `ramshared`.\n\
         Próximo: preflight + mlockall + CUDA alloc + canário (§9) + NBD device wiring."
    );
}
