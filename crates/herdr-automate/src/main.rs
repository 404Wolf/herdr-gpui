//! `herdr-automate`: runs approved automation scripts on a host. Phase 1
//! ships the checks an agent needs to write one; nothing runs yet.

mod cli;

use std::{io::Write, process::ExitCode};

fn main() -> ExitCode {
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "herdr-automate: {error}");
            return ExitCode::from(2);
        }
    };
    let mut out = std::io::stdout().lock();
    match cli::run(&command, &mut out) {
        Ok(code) => code,
        // A closed pipe (`check ... | head`) is not the script's failure.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
