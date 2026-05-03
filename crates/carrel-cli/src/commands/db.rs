//! `carrel db ...` implementation.

use std::str::FromStr;

use carrel_feeds::{detect_shape, html_to_text};
use carrel_store::Store;
use carrel_store::blobs::{BlobId, BlobStore};
use clap::{Args, Subcommand};
use serde_json::json;

use crate::config::Context;
use crate::error::Result;
use crate::output::{print_json, print_rows};

/// Database subcommands.
#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// Run a raw CozoScript query against the store.
    Query(QueryCommand),

    /// Apply pending schema migrations.
    Migrate,

    /// Recompute derived facts from cached local data.
    Recompute {
        /// Recompute command to run.
        #[command(subcommand)]
        command: RecomputeCommand,
    },
}

/// Derived-fact recomputation commands.
#[derive(Debug, Subcommand)]
pub enum RecomputeCommand {
    /// Recompute shape facts for items with cached readable content.
    Shapes {
        /// Recompute all cached readable items. This is the default in v1.
        #[arg(long)]
        all: bool,
    },
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
        DbCommand::Recompute { command } => recompute(context, &store, command),
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

fn recompute(context: &Context, store: &Store, command: &RecomputeCommand) -> Result<()> {
    match command {
        RecomputeCommand::Shapes { all: _ } => recompute_shapes(context, store),
    }
}

fn recompute_shapes(context: &Context, store: &Store) -> Result<()> {
    store.migrate()?;
    let blobs = BlobStore::open(&context.paths.blobs);
    let items = store.list_items_with_readable_content()?;
    let mut errors = Vec::new();
    let mut recomputed = 0usize;

    for item in &items {
        match recompute_one_shape(store, &blobs, &item.item_id, &item.blob_id) {
            Ok(()) => recomputed += 1,
            Err(error) => errors.push(json!({
                "item_id": item.item_id,
                "blob_id": item.blob_id,
                "message": error.to_string(),
            })),
        }
    }

    if context.json {
        print_json(&json!({
            "scanned": items.len(),
            "recomputed": recomputed,
            "errors": errors,
        }))
    } else {
        println!(
            "Recomputed shapes for {recomputed}/{} cached readable items.",
            items.len()
        );
        if !errors.is_empty() {
            println!("{} errors", errors.len());
        }
        Ok(())
    }
}

fn recompute_one_shape(
    store: &Store,
    blobs: &BlobStore,
    item_id: &str,
    blob_id: &str,
) -> Result<()> {
    let blob_id = BlobId::from_str(blob_id)?;
    let bytes = blobs.get_blocking(&blob_id)?;
    let html = String::from_utf8_lossy(&bytes);
    let word_count = html_to_text(&html).split_whitespace().count();
    let shape = detect_shape(&html, word_count);
    store.put_item_shape(item_id, &shape)?;
    Ok(())
}
