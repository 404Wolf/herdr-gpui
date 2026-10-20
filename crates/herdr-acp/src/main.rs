//! `herdr-acp`: runs an ACP agent in a terminal pane, draws the conversation
//! there, and lets Herdr UI attach to it.

mod agents;
mod app;
mod attach;
mod cli;
mod error;
mod herdr;
mod process;
mod project;
mod rpc;
mod session;
mod tui;

use std::process::ExitCode;

pub(crate) use error::{Error, Result};

fn main() -> ExitCode {
    match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Parsed::Help) => {
            print!("{}", cli::HELP);
            ExitCode::SUCCESS
        }
        Ok(cli::Parsed::Version) => {
            println!("herdr-acp {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Parsed::Run(options)) => match app::run(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("herdr-acp: {error:#}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("herdr-acp: {error}\n\nRun `herdr-acp --help` for usage.");
            ExitCode::from(2)
        }
    }
}
