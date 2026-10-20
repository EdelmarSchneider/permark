// Release on Windows: no console window. NEVER remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use permark_core::open::{parse_args, Cli, USAGE};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let cwd = std::env::current_dir().ok();
    // Command-line modes exit before Tauri boots (no window, no single-instance socket).
    match parse_args(&args, cwd.as_deref()) {
        Cli::Open(request) => match permark_lib::run(request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("PerMark could not start: {error}");
                ExitCode::FAILURE
            }
        },
        #[cfg(feature = "plugin-agent")]
        Cli::Mcp { structured } => permark_lib::agent::mcp(structured),
        #[cfg(feature = "plugin-agent")]
        Cli::Context => permark_lib::agent::context(),
        #[cfg(not(feature = "plugin-agent"))]
        Cli::Mcp { .. } | Cli::Context => {
            eprintln!("PerMark: this build has no agent connection.");
            ExitCode::from(2)
        }
        Cli::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Cli::Version => {
            println!("PerMark {}", permark_core::app_info().version);
            ExitCode::SUCCESS
        }
        Cli::Usage(message) => {
            eprintln!("permark: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
