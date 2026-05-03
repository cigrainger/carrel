//! carrel-cli: headless development interface for Carrel.
//!
//! The CLI is intentionally small for now. Brief 4 gives it real store-backed
//! commands once the schema exists.

#![deny(unsafe_code)]

use clap::{Parser, Subcommand};

/// Command-line entry point for Carrel development tasks.
#[derive(Debug, Parser)]
#[command(version, about = "Headless development interface for Carrel")]
struct Cli {
    /// Command to run.
    #[command(subcommand)]
    command: Option<Command>,
}

/// Available CLI commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print the CLI version.
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Version) {
        Command::Version => println!("carrel-cli {}", env!("CARGO_PKG_VERSION")),
    }
}
