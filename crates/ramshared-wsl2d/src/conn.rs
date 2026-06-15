//! Conexão NBD multi-conexão (§9.4 / H1): leitor e escritor dedicados por conexão,
//! ligados ao **worker CUDA único** (em `main`) por canais. O leitor drena o socket
//! e enfileira `Job`s; o worker processa (afinidade CUDA) e devolve `Reply`s pelo
//! canal **ilimitado** de réplica da conexão; o escritor escreve no socket.
//!
//! SPEC: `docs/daemon-multiconn/SPECv3.md` (DT-7/DT-8/DT-15/DT-16). Desenho determinístico:
//! `Opened` vem do acceptor (antes de spawnar o reader), `Closed` vem do reader (ao sair) —
//! o worker conta `live` e encerra quando todas as conexões abertas fecham.

use std::io::{BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel};
use std::thread::JoinHandle;

use ramshared_block::handshake::Export;
use ramshared_block::protocol::SIMPLE_REPLY_LEN;
use ramshared_block::{Command, Request, parse_request, protocol::REQUEST_LEN, server_handshake};

/// Capacidade do canal de mensagens do worker (`WMsg`): **único** ponto de backpressure.
/// O canal de réplica por conexão é ilimitado (DT-7), então o worker nunca bloqueia ao
/// responder — só os leitores fazem backpressure ao enfileirar `Job`s.
pub const CHAN_CAP: usize = 64;

/// Um request a processar pelo worker CUDA, com a rota de réplica da conexão de origem.
/// A latência do canário é medida no worker em volta do `serve()` (serve-only, DT-16
/// revisado): medir a espera na fila causava falso-positivo de DEMOTE sob carga normal.
pub struct Job {
    /// Índice do export (slice) negociado no handshake — qual janela o worker serve (RF-L1).
    pub export: usize,
    pub req: Request,
    pub payload: Vec<u8>,
    pub reply: Sender<Reply>,
}

/// Resultado do `serve()` a escrever no socket da conexão. `reply` é o cabeçalho NBD
/// de 16 bytes (array fixo `Copy`, sem alocação no hot path — DT-8).
pub struct Reply {
    pub reply: [u8; SIMPLE_REPLY_LEN],
    pub data: Vec<u8>,
    pub disconnect: bool,
}

/// Mensagem do canal do worker (DT-15). `Opened`/`Closed` controlam o término
/// determinístico; `Job` é trabalho; `ZeroExport` é a higiene de slice do broker (DT-17): o
/// worker (única thread dona do backend) zera a janela `[base, base+len)` e confirma por `done`.
pub enum WMsg {
    Opened,
    Job(Job),
    Closed,
    ZeroExport {
        base: u64,
        len: u64,
        done: Sender<bool>,
    },
}

/// Contagem de conexões vivas no worker (DT-15). `Opened` (do acceptor) sempre precede
/// `Closed` (do reader) por conexão, então `live` fica balanceado; o worker para quando
/// todas as conexões abertas fecharam. Lógica pura (testável sem GPU/sockets).
#[derive(Default)]
pub struct LiveCount {
    live: u32,
    opened: bool,
}

impl LiveCount {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_open(&mut self) {
        self.live += 1;
        self.opened = true;
    }

    /// Registra o fechamento de uma conexão; retorna `true` quando **todas** as conexões
    /// abertas já fecharam (o worker deve encerrar). `saturating_sub` evita underflow caso
    /// um `Closed` chegue desbalanceado (não deve, mas é defensivo).
    pub fn on_close(&mut self) -> bool {
        self.live = self.live.saturating_sub(1);
        self.live == 0 && self.opened
    }

    pub fn live(&self) -> u32 {
        self.live
    }
}

/// Thread escritora: drena `Reply`s e escreve no socket. Réplicas podem sair fora de
/// ordem (cada uma carrega o `handle` NBD). Encerra em erro de socket, em `disconnect`,
/// ou quando o canal fecha (leitor saiu e todas as réplicas foram drenadas).
pub fn spawn_writer<S: Write + Send + 'static>(
    stream: S,
    replies: Receiver<Reply>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut w = stream;
        for r in replies.iter() {
            if w.write_all(&r.reply).is_err() {
                break;
            }
            if !r.data.is_empty() && w.write_all(&r.data).is_err() {
                break;
            }
            if w.flush().is_err() {
                break;
            }
            if r.disconnect {
                break;
            }
        }
    })
}

/// Thread leitora (genérica sobre o stream — Unix ou TCP, RF-L2): handshake na própria thread
/// (DT-15 — erro confinado à conexão), negocia o export pelo nome (RF-L1) e enfileira `Job`s com
/// o índice do export. `hs_writer` é o handle de escrita (clone feito pelo acceptor) usado só no
/// handshake. Ao sair (EOF/erro/handshake falho) envia `WMsg::Closed` para balancear o `Opened`.
pub fn spawn_reader<S: Read + Send + 'static, W2: Write + Send + 'static>(
    stream: S,
    mut hs_writer: W2,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
    reply_tx: Sender<Reply>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let idx = match server_handshake(&mut reader, &mut hs_writer, &exports, tx_flags) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[ramsharedd] conn: handshake falhou: {e}");
                let _ = jobs.send(WMsg::Closed);
                return;
            }
        };
        drop(hs_writer); // handshake concluído; daqui só o writer thread escreve réplicas.
        let export_size = exports[idx].size; // anti-DoS pelo export negociado (RF-L1)

        let mut hdr = [0u8; REQUEST_LEN];
        loop {
            if reader.read_exact(&mut hdr).is_err() {
                break; // EOF ou erro de socket
            }
            let req = match parse_request(&hdr) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[ramsharedd] conn: request malformado: {e}; desconectando");
                    break;
                }
            };
            // Anti-DoS: um WRITE nunca pode exceder o export negociado (evita alocar gigabytes).
            if req.cmd == Command::Write && req.len as u64 > export_size {
                eprintln!(
                    "[ramsharedd] conn: WRITE len {} excede o export; desconectando",
                    req.len
                );
                break;
            }
            let payload = if req.cmd == Command::Write {
                let mut p = vec![0u8; req.len as usize];
                if reader.read_exact(&mut p).is_err() {
                    break;
                }
                p
            } else {
                Vec::new()
            };
            let job = Job {
                export: idx,
                req,
                payload,
                reply: reply_tx.clone(),
            };
            if jobs.send(WMsg::Job(job)).is_err() {
                break; // worker encerrou
            }
        }
        let _ = jobs.send(WMsg::Closed);
    })
}

/// Fia uma conexão aceita ao worker: `WMsg::Opened` **antes** de spawnar o reader (balanço do
/// `live`, DT-15), canal de réplica **ilimitado** (DT-7), writer + reader. Genérico sobre os
/// handles (Unix/TCP). Retorna `false` se o worker encerrou (o acceptor deve parar).
fn wire_conn<RS, WS>(
    rstream: RS,
    wstream: WS,
    hs_writer: WS,
    exports: &Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: &SyncSender<WMsg>,
) -> bool
where
    RS: Read + Send + 'static,
    WS: Write + Send + 'static,
{
    if jobs.send(WMsg::Opened).is_err() {
        return false; // worker encerrou
    }
    let (reply_tx, reply_rx) = channel::<Reply>(); // ilimitado (DT-7)
    spawn_writer(wstream, reply_rx);
    spawn_reader(
        rstream,
        hs_writer,
        Arc::clone(exports),
        tx_flags,
        jobs.clone(),
        reply_tx,
    );
    true
}

/// Acceptor **Unix**: aceita conexões em laço (N-agnóstico) e fia cada uma ao worker, negociando
/// o export por nome via `exports` (RF-L1). Cada conexão precisa de 2 clones do stream (writer +
/// handshake) além do handle de leitura.
pub fn spawn_acceptor(
    listener: UnixListener,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] accept falhou: {e}");
                    break;
                }
            };
            let (wstream, hs_writer) = match (stream.try_clone(), stream.try_clone()) {
                (Ok(w), Ok(h)) => (w, h),
                _ => {
                    eprintln!("[ramsharedd] try_clone (unix) falhou; pulando conexão");
                    continue;
                }
            };
            if !wire_conn(stream, wstream, hs_writer, &exports, tx_flags, &jobs) {
                break;
            }
        }
    })
}

/// Acceptor **TCP** (RF-L2): mesmo desenho do Unix sobre `TcpListener`, alimentando o MESMO
/// canal `jobs` (o worker é único). `TCP_NODELAY` por conexão (latência de swap).
pub fn spawn_acceptor_tcp(
    listener: TcpListener,
    exports: Arc<Vec<Export>>,
    tx_flags: u16,
    jobs: SyncSender<WMsg>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] accept tcp falhou: {e}");
                    break;
                }
            };
            let _ = stream.set_nodelay(true); // TCP_NODELAY: latência de swap
            let (wstream, hs_writer) = match (stream.try_clone(), stream.try_clone()) {
                (Ok(w), Ok(h)) => (w, h),
                _ => {
                    eprintln!("[ramsharedd] try_clone (tcp) falhou; pulando conexão");
                    continue;
                }
            };
            if !wire_conn(stream, wstream, hs_writer, &exports, tx_flags, &jobs) {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::mpsc::sync_channel;

    fn dummy_req() -> Request {
        Request {
            flags: 0,
            cmd: Command::Read,
            handle: 1,
            offset: 0,
            len: 0,
        }
    }

    #[test]
    fn job_reply_roundtrip() {
        let (tx, _rx) = channel::<Reply>();
        let job = Job {
            export: 0,
            req: dummy_req(),
            payload: vec![1, 2, 3],
            reply: tx,
        };
        assert_eq!(job.req.handle, 1);
        assert_eq!(job.payload, vec![1, 2, 3]);
        let rep = Reply {
            reply: [0u8; SIMPLE_REPLY_LEN],
            data: vec![9, 8, 7],
            disconnect: false,
        };
        assert_eq!(rep.data, vec![9, 8, 7]);
        assert!(!rep.disconnect);
    }

    #[test]
    fn chan_cap_is_bounded() {
        let (tx, _rx) = sync_channel::<u8>(2);
        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());
        assert!(
            tx.try_send(3).is_err(),
            "deve recusar além do cap (backpressure)"
        );
    }

    // DT-18 / F-3/F-5: término determinístico — para exatamente quando live==0.
    #[test]
    fn live_count_terminates_on_all_closed() {
        let mut lc = LiveCount::new();
        lc.on_open(); // live=1
        lc.on_open(); // live=2
        assert!(!lc.on_close(), "live=1 ainda"); // live=1
        assert!(lc.on_close(), "live=0 + opened -> para"); // live=0
    }

    // DT-18 / F-6: handshake falho = Opened (acceptor) + Closed (reader) imediato; balanceado.
    #[test]
    fn live_count_balanced_open_then_close() {
        let mut lc = LiveCount::new();
        lc.on_open();
        assert!(lc.on_close(), "1 conexão abriu e fechou -> para");
    }

    #[test]
    fn live_count_never_stops_before_any_open() {
        let mut lc = LiveCount::new();
        assert!(!lc.on_close(), "sem Opened não para espuriamente");
        assert_eq!(lc.live(), 0);
    }

    // DT-7 / DT-18: réplica ilimitada — worker progride mesmo com o writer parado.
    // Se a réplica fosse limitada e o writer não drenasse, o worker bloquearia →
    // canal de Jobs encheria → reader bloquearia → deadlock (este teste travaria).
    #[test]
    fn slow_writer_does_not_deadlock() {
        let (jobs_tx, jobs_rx) = sync_channel::<WMsg>(2); // canal de Jobs pequeno
        let (reply_tx, reply_rx) = channel::<Reply>(); // réplica ILIMITADA (DT-7)
        let _writer_parado = reply_rx; // segura sem drenar (simula socket travado)

        let worker = std::thread::spawn(move || {
            let mut served = 0u32;
            for m in jobs_rx.iter() {
                if let WMsg::Job(job) = m {
                    // worker nunca bloqueia: réplica ilimitada
                    let _ = job.reply.send(Reply {
                        reply: [0u8; SIMPLE_REPLY_LEN],
                        data: Vec::new(),
                        disconnect: false,
                    });
                    served += 1;
                    if served >= 10 {
                        break;
                    }
                }
            }
            served
        });

        for _ in 0..10 {
            jobs_tx
                .send(WMsg::Job(Job {
                    export: 0,
                    req: dummy_req(),
                    payload: Vec::new(),
                    reply: reply_tx.clone(),
                }))
                .unwrap();
        }
        assert_eq!(
            worker.join().unwrap(),
            10,
            "worker processou tudo sem deadlock"
        );
    }
}
