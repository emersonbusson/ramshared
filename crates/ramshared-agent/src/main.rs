//! `ramshared-agent` — agent (tenant) of the Memory Broker. Connects to the broker via TCP, reports
//! PSI/swaps 1×/s and executes `SwapOn`/`SwapOff`/`DemoteAll` commands over NBD (DT-27).
//!
//! 3-thread architecture with **single writer** (DT-27/R8):
//! - **reader**: blocks on `read_msg(socket)` and forwards each `Msg` to the main loop;
//! - **exec**: executes `attach`/`detach` (blocking) out of the socket path and returns the
//!   result via channel — this way a slow `swapon` never blocks the heartbeat;
//! - **main**: owner of the write socket — sends `Psi`, dispatches commands to exec, drains the
//!   results back as `SwapOnDone`/`SwapOffDone` and arms the watchdog (DT-18).
//!
//! SPEC: docs/specs/no-milestone/memory-broker/SPEC.md (ITEM-9). Without `unsafe`.
#![forbid(unsafe_code)]

use ramshared_agent::agent_cmd::{self, CliExit, ParsedArgs};
use ramshared_agent::psi;

fn run(args: &[String]) -> Result<(), CliExit> {
    let cfg = match agent_cmd::parse_args(args).map_err(CliExit::Usage)? {
        ParsedArgs::Help => return Err(CliExit::Help),
        ParsedArgs::Config(cfg) => cfg,
    };

    if cfg.status_only {
        return agent_cmd::run_status(&cfg).map_err(|error| CliExit::Runtime(error.to_string()));
    }
    if cfg.tenant.is_empty() {
        return Err(CliExit::Usage(format!(
            "--tenant is required in agent mode
{}",
            agent_cmd::usage()
        )));
    }

    // DT-26: swap requires privilege. Reads euid via /proc (no libc) and refuses early, with number.
    let euid = psi::read_euid().map_err(|error| CliExit::Runtime(error.to_string()))?;
    if euid != 0 {
        return Err(CliExit::Runtime(format!(
            "root is required for swap (current euid={euid}, expected 0)"
        )));
    }

    agent_cmd::run_agent(&cfg).map_err(|error| CliExit::Runtime(error.to_string()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => {}
        Err(CliExit::Help) => println!("{}", agent_cmd::usage()),
        Err(CliExit::Usage(message)) => {
            eprintln!("{}", agent_cmd::usage_diagnostic(&message));
            std::process::exit(2);
        }
        Err(CliExit::Runtime(message)) => {
            eprintln!("[agent] error: {message}");
            std::process::exit(1);
        }
    }
}
