//! Watchdog de sessão do agente: se o broker para de responder (sem `Ack`/comando por
//! `deadline`), a sessão é considerada morta e o agente faz cleanup + reconecta (DT-18/DT-27).
//!
//! Puro e com relógio injetado (`Instant`) para ser testável de forma determinística — o
//! `main.rs` passa `Instant::now()`. Nada de I/O aqui.

use std::time::{Duration, Instant};

/// Acompanha o último sinal vindo do broker. `expired(now)` indica sessão morta.
#[derive(Debug, Clone, Copy)]
pub struct Watchdog {
    deadline: Duration,
    last: Instant,
}

impl Watchdog {
    /// Cria o watchdog "tocado" em `now` (início da sessão conta como sinal fresco).
    pub fn new(deadline: Duration, now: Instant) -> Self {
        Self {
            deadline,
            last: now,
        }
    }

    /// Registra um sinal do broker (qualquer mensagem, inclusive `Ack`).
    pub fn touch(&mut self, now: Instant) {
        self.last = now;
    }

    /// `true` se passou `deadline` desde o último sinal.
    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.last) >= self.deadline
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn fresh_watchdog_not_expired() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0);
        assert!(!wd.expired(t0));
        assert!(!wd.expired(t0 + Duration::from_secs(89)));
    }

    #[test]
    fn expires_after_deadline() {
        let t0 = Instant::now();
        let wd = Watchdog::new(Duration::from_secs(90), t0);
        assert!(wd.expired(t0 + Duration::from_secs(90)));
        assert!(wd.expired(t0 + Duration::from_secs(120)));
    }

    #[test]
    fn touch_resets_the_clock() {
        let t0 = Instant::now();
        let mut wd = Watchdog::new(Duration::from_secs(90), t0);
        let t1 = t0 + Duration::from_secs(80);
        wd.touch(t1);
        // 80s + 89s = 169s do início, mas só 89s do último toque → ainda vivo.
        assert!(!wd.expired(t1 + Duration::from_secs(89)));
        assert!(wd.expired(t1 + Duration::from_secs(90)));
    }
}
