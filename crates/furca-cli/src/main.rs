//! furca: the headless CLI door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "furca",
    version,
    about = "A fast git client that stays out of your way."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the repository's HEAD summary as JSON.
    Status {
        /// Path inside the repository to inspect. Defaults to the current directory.
        path: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Status { path } => {
            let path = path.unwrap_or_else(|| PathBuf::from("."));
            match run_status(&path) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("furca: {message}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn run_status(path: &std::path::Path) -> Result<(), String> {
    let repo = furca_core::Repository::open(path).map_err(|e| e.to_string())?;
    let head = repo.head().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&head).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}
