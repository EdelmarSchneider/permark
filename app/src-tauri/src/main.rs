// Release on Windows: no console window. NEVER remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match permark_lib::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PerMark could not start: {error}");
            ExitCode::FAILURE
        }
    }
}
