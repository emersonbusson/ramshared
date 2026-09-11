//! Shell completion generation for `ramshared` CLI using `clap`.
#![forbid(unsafe_code)]

use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

/// CLI parser for completions mapping exactly to main.rs legacy args.
#[derive(Parser)]
#[command(name = "ramshared")]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommands,
}

#[derive(Subcommand)]
pub enum CliCommands {
    /// Show ramshared version.
    Version,

    /// Run a supervised workload.
    Run {
        #[arg(long, help = "interactive|build|browser-test|batch")]
        class: String,
        #[arg(long, help = "Memory limit in MiB")]
        memory_max: Option<usize>,
        #[arg(last = true, help = "Command and arguments")]
        args: Vec<String>,
    },

    /// Open an interactive shell session in a reserved scope.
    Session {
        #[arg(long, help = "interactive|build|browser-test|batch")]
        class: String,
        #[arg(long, help = "Memory limit in MiB")]
        memory_max: Option<usize>,
    },

    /// Run supervisor daemon.
    Supervise {
        #[arg(long, help = "Run a single pass then exit")]
        once: bool,
    },

    /// Recover crashed workloads or view recovery status.
    Recover {
        #[arg(long, group = "mode", help = "View status")]
        status: bool,
        #[arg(long, group = "mode", help = "Resume recovered workloads")]
        resume: bool,
    },

    /// Check system dependencies and readiness.
    Check {
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },

    /// Advanced diagnostics.
    Doctor {
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },

    /// Setup and activate cascade.
    Up {
        #[arg(long, help = "VRAM size in MiB")]
        vram: Option<usize>,
        #[arg(long, help = "ZRAM size in MiB")]
        zram: Option<usize>,
        #[arg(long, help = "Path to daemon binary")]
        daemon: Option<String>,
    },

    /// Deactivate cascade and teardown.
    Down,

    /// Show current status of cascade.
    Status {
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },

    /// Real-time monitoring and telemetry.
    Monitor {
        #[arg(long, help = "Output JSON lines")]
        jsonl: bool,
        #[arg(long, help = "Compact mode")]
        compact: bool,
        #[arg(long, default_value_t = 1000, help = "Interval in ms")]
        interval_ms: u64,
        #[arg(long, default_value_t = 300, help = "History length in seconds")]
        history_seconds: u64,
        #[arg(long, help = "Output file path")]
        output: Option<String>,
        #[arg(long, help = "Heartbeat file path")]
        heartbeat: Option<String>,
        #[arg(long, help = "Run once and exit")]
        once: bool,
    },

    /// Diagnose historical events.
    Diagnose {
        #[arg(long, help = "Path to events log")]
        events: String,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },

    /// Memory stress test.
    Stress {
        #[arg(long, help = "Start memory %")]
        start: Option<u64>,
        #[arg(long, help = "Target memory %")]
        target: Option<u64>,
        #[arg(long, help = "Step memory %")]
        step: Option<u64>,
        #[arg(long, help = "Interval in ms")]
        interval_ms: Option<u64>,
        #[arg(long, help = "Hold time in seconds")]
        hold_sec: Option<u64>,
        #[arg(long, help = "Minimum ram in MiB")]
        min_ram_mb: Option<u64>,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },

    /// Generate shell completions.
    Completion {
        #[arg(value_enum, help = "Shell to generate completions for")]
        shell: Shell,
    },
}

/// Generate shell completion script for the target shell and write to stdout.
pub fn generate_script(shell: Shell, stdout: &mut dyn Write) -> ExitCode {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "ramshared", stdout);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }
}
