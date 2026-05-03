//! `carrel db ...` implementation.

use carrel_store::Store;
use clap::{Args, Subcommand};

use crate::config::Context;
use crate::error::Result;
use crate::output::print_rows;

/// Database subcommands.
#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Run a raw CozoScript query against the store.
    Query(QueryCommand),

    /// Apply pending schema migrations.
    Migrate,
}

/// Raw database query arguments.
#[derive(Debug, Args)]
pub struct QueryCommand {
    /// CozoScript query to run.
    pub datalog: String,
}

/// Run a database command.
pub fn run(context: &Context, command: &DbCommand) -> Result<()> {
    context.paths.require_initialized()?;
    let store = Store::open(&context.paths.store)?;

    match command {
        DbCommand::Query(command) => query(context, &store, command),
        DbCommand::Migrate => migrate(&store),
    }
}

fn query(context: &Context, store: &Store, command: &QueryCommand) -> Result<()> {
    let rows = store.query(&command.datalog)?;
    print_rows(&rows, context.json)
}

fn migrate(store: &Store) -> Result<()> {
    let before = store.current_schema_version()?;
    store.migrate()?;
    let after = store.current_schema_version()?;

    if before == after {
        println!("Already at v{after}.");
    } else {
        println!("Migrated v{before} -> v{after}.");
    }

    Ok(())
}
