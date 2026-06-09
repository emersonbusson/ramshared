//! `swapoff` do tier (caminho de DEMOTE). Extraído do daemon para reuso pelos dois
//! transportes: o NBD (`main.rs`) e o worker DT-3 do ublk (`ublk_server.rs`). O
//! swapoff roda **fora** do caminho que serve o swap (thread separada) — Disciplina 3
//! (anti-deadlock): bloquear o servidor no swapoff travaria o próprio swap.

use std::sync::mpsc::Receiver;

/// Caminho absoluto do `swapoff` (#2c: um daemon root NAO deve depender do `$PATH`;
/// evita shim malicioso no ambiente). Fallback p/ `$PATH` so' como ultimo recurso.
pub fn swapoff_bin() -> &'static str {
    const CANDIDATES: &[&str] = &["/usr/sbin/swapoff", "/sbin/swapoff"];
    for c in CANDIDATES {
        if std::path::Path::new(c).exists() {
            return c;
        }
    }
    "swapoff"
}

/// Dispara `swapoff <dev>` numa thread separada (nao bloqueia o servidor) e devolve o
/// canal que confirma o resultado (`true` = sucesso). Caminho unico de DEMOTE (DT-8):
/// usado pela latencia por-request e pela sonda em cadencia.
pub fn spawn_swapoff(dev: &str) -> Receiver<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    let dev = dev.to_string();
    std::thread::spawn(move || {
        let ok = std::process::Command::new(swapoff_bin())
            .arg(&dev)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let _ = tx.send(ok);
    });
    rx
}
