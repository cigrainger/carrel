//! carrel-cli: headless development interface for Carrel.

#![deny(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::filter::LevelFilter;

mod commands;
mod config;
mod error;
mod output;

use crate::commands::db::DbCommand;
use crate::commands::feed::FeedCommand;
use crate::commands::init::InitCommand;
use crate::error::Result;

/// Command-line entry point for Carrel development tasks.
#[derive(Debug, Parser)]
#[command(version, about = "Headless development interface for Carrel")]
struct Cli {
    /// Override the Carrel data directory.
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    /// Emit machine-readable JSON where a command supports it.
    #[arg(long, global = true)]
    json: bool,

    /// Enable info-level diagnostic logging.
    #[arg(long, global = true, conflicts_with = "debug")]
    verbose: bool,

    /// Enable debug-level diagnostic logging.
    #[arg(long, global = true)]
    debug: bool,

    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

/// Available CLI commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a fresh Carrel data directory.
    Init(InitCommand),

    /// Print diagnostic information about the current install.
    Info,

    /// Inspect or maintain the local Cozo store.
    Db {
        /// Database command to run.
        #[command(subcommand)]
        command: DbCommand,
    },

    /// Manage and fetch feed subscriptions.
    Feed {
        /// Feed command to run.
        #[command(subcommand)]
        command: FeedCommand,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(&cli);

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let context = config::Context::resolve(cli.data_dir, cli.json)?;

    match cli.command {
        Command::Init(command) => commands::init::run(&context, &command),
        Command::Info => commands::info::run(&context),
        Command::Db { command } => commands::db::run(&context, &command),
        Command::Feed { command } => commands::feed::run(&context, &command).await,
    }
}

fn init_tracing(cli: &Cli) {
    let level = if cli.debug {
        LevelFilter::DEBUG
    } else if cli.verbose {
        LevelFilter::INFO
    } else {
        LevelFilter::WARN
    };

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .without_time()
        .try_init();
}
