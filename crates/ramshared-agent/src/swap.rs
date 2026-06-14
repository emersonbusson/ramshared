//! Execução de swap sobre NBD: conecta `nbd-client`, formata com `mkswap` (DT-16) e ativa
//! com `swapon` (prioridade DT-7); o caminho inverso desliga e desconecta. A montagem de
//! argv é pura/testável; as funções que disparam processos são finas (`Command`) e validadas
//! ao vivo no drill qemu / civm (não no WSL2 — regra de sessão).
//!
//! DT-14: `nbd-client` SEMPRE com `-timeout 30` e NUNCA `-persist` (sem auto-reconnect; o
//! broker reassina). DT-16: `mkswap` é obrigatório a cada attach (a VRAM volta zerada/suja).

use std::io::{Error, Result};
use std::process::Command;

use ramshared_broker::protocol::NbdEndpoint;

/// Monta o argv do `nbd-client` para anexar `export` em `dev` (DT-14: `-timeout 30`, sem
/// `-persist`). Unix usa `-unix <path>`; TCP usa `<host> <port>` posicionais.
pub fn nbd_args(endpoint: &NbdEndpoint, export: &str, dev: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["-N".into(), export.into()];
    match endpoint {
        NbdEndpoint::Unix { path } => {
            a.push("-unix".into());
            a.push(path.clone());
            a.push(dev.into());
        }
        NbdEndpoint::Tcp { host, port } => {
            a.push(host.clone());
            a.push(port.to_string());
            a.push(dev.into());
        }
    }
    a.push("-timeout".into());
    a.push("30".into());
    a
}

/// Monta o argv do `swapon` (DT-7: só emite `-p <prio>` quando a prioridade é definida).
pub fn swapon_args(dev: &str, prio: Option<i32>) -> Vec<String> {
    let mut a = Vec::new();
    if let Some(p) = prio {
        a.push("-p".to_string());
        a.push(p.to_string());
    }
    a.push(dev.to_string());
    a
}

/// Roda um comando e converte saída não-zero em `Err` com detalhe (nunca engole o erro).
fn run(cmd: &str, args: &[String]) -> Result<()> {
    let status = Command::new(cmd).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::other(format!(
            "{cmd} {} -> {status}",
            args.join(" ")
        )))
    }
}

/// Sequência completa de attach (DT-16): `nbd-client` → `mkswap` → `swapon`. Em falha,
/// devolve a mensagem para o agente reportar via `SwapOnDone{ok:false,detail}`.
pub fn attach_swap(
    endpoint: &NbdEndpoint,
    export: &str,
    dev: &str,
    prio: Option<i32>,
) -> std::result::Result<(), String> {
    run("nbd-client", &nbd_args(endpoint, export, dev)).map_err(|e| format!("nbd-client: {e}"))?;
    // DT-16: a VRAM exportada volta suja/zerada; o cabeçalho de swap precisa ser reescrito.
    run("mkswap", &[dev.to_string()]).map_err(|e| format!("mkswap: {e}"))?;
    run("swapon", &swapon_args(dev, prio)).map_err(|e| format!("swapon: {e}"))?;
    Ok(())
}

/// Sequência de detach: `swapoff` → `nbd-client -d`. Best-effort no desconnect (o device pode
/// já ter caído); o que importa para a integridade é o `swapoff` ter saído.
pub fn detach_swap(dev: &str) -> std::result::Result<(), String> {
    run("swapoff", &[dev.to_string()]).map_err(|e| format!("swapoff: {e}"))?;
    let _ = run("nbd-client", &["-d".to_string(), dev.to_string()]);
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn nbd_args_tcp_has_timeout_no_persist() {
        let ep = NbdEndpoint::Tcp {
            host: "10.0.0.1".into(),
            port: 10809,
        };
        let a = nbd_args(&ep, "s0", "/dev/nbd0");
        assert_eq!(
            a,
            vec![
                "-N",
                "s0",
                "10.0.0.1",
                "10809",
                "/dev/nbd0",
                "-timeout",
                "30"
            ]
        );
        assert!(!a.iter().any(|x| x == "-persist"), "DT-14: nunca -persist");
    }

    #[test]
    fn nbd_args_unix_uses_unix_flag() {
        let ep = NbdEndpoint::Unix {
            path: "/run/r.sock".into(),
        };
        let a = nbd_args(&ep, "s2", "/dev/nbd2");
        assert_eq!(
            a,
            vec![
                "-N",
                "s2",
                "-unix",
                "/run/r.sock",
                "/dev/nbd2",
                "-timeout",
                "30"
            ]
        );
        assert!(!a.iter().any(|x| x == "-persist"));
    }

    #[test]
    fn swapon_args_emits_prio_only_when_set() {
        assert_eq!(
            swapon_args("/dev/nbd0", Some(-5)),
            vec!["-p", "-5", "/dev/nbd0"]
        );
        assert_eq!(swapon_args("/dev/nbd0", None), vec!["/dev/nbd0"]);
    }
}
