//! Item inspection commands.

use clap::Subcommand;
use serde_json::json;
use std::str::FromStr;

use carrel_feeds::html_to_text;
use carrel_store::Store;
use carrel_store::blobs::{BlobId, BlobStore};

use crate::config::Context;
use crate::error::{CliError, Result};
use crate::output;

/// Item subcommands.
#[derive(Debug, Subcommand)]
pub enum ItemCommand {
    /// Show an item and a short preview of cached readable content.
    Show {
        /// Item id to show.
        id: String,

        /// Number of words to include from the readable body.
        #[arg(long, default_value_t = 80)]
        words: usize,
    },
}

/// Run an item subcommand.
pub async fn run(context: &Context, command: &ItemCommand) -> Result<()> {
    context.paths.require_initialized()?;
    let store = Store::open(&context.paths.store)?;
    store.migrate()?;
    let blobs = BlobStore::open(&context.paths.blobs);

    match command {
        ItemCommand::Show { id, words } => show_item(context, &store, &blobs, id, *words).await,
    }
}

async fn show_item(
    context: &Context,
    store: &Store,
    blobs: &BlobStore,
    id: &str,
    words: usize,
) -> Result<()> {
    let item = store
        .get_item_detail(id)?
        .ok_or_else(|| CliError::user(format!("no item found for {id}")))?;
    let preview = match &item.readable {
        Some(readable) => {
            let blob_id = BlobId::from_str(&readable.blob_id)?;
            let bytes = blobs.get(&blob_id).await?;
            let html = String::from_utf8_lossy(&bytes);
            Some(first_words(&html_to_text(&html), words))
        }
        None => None,
    };

    if context.json {
        output::print_json(&json!({
            "id": item.id,
            "title": item.title,
            "primary_url": item.primary_url,
            "summary": item.summary,
            "readable": item.readable.as_ref().map(|readable| json!({
                "blob_id": readable.blob_id,
                "extracted_with": readable.extracted_with,
                "byte_size": readable.byte_size,
            })),
            "preview": preview,
        }))
    } else {
        println!("{}", item.title);
        println!("id: {}", item.id);
        if let Some(url) = item.primary_url {
            println!("url: {url}");
        }
        if let Some(readable) = item.readable {
            println!(
                "content: {} bytes via {}",
                readable.byte_size,
                readable.extracted_with.as_deref().unwrap_or("unknown")
            );
        } else {
            println!("content: not cached");
        }
        if let Some(preview) = preview {
            println!();
            println!("{preview}");
        }
        Ok(())
    }
}

fn first_words(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .take(limit)
        .collect::<Vec<_>>()
        .join(" ")
}
