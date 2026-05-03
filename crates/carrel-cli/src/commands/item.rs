//! Item inspection commands.

use clap::Subcommand;
use serde_json::json;
use std::str::FromStr;

use carrel_feeds::html_to_text;
use carrel_store::Store;
use carrel_store::blobs::{BlobId, BlobStore};
use cozo::Validity;

use crate::config::Context;
use crate::error::{CliError, Result};
use crate::output::{self, format_validity};

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

    /// Show stored shape facts for an item.
    Shape {
        /// Item id to inspect.
        id: String,
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
        ItemCommand::Shape { id } => show_shape(context, &store, id),
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

fn show_shape(context: &Context, store: &Store, id: &str) -> Result<()> {
    let shape = store
        .get_item_shape(id)?
        .ok_or_else(|| CliError::user(format!("no shape facts found for {id}")))?;

    if context.json {
        output::print_json(&json!({
            "item_id": shape.item_id,
            "has_video_embed": shape.shape.has_video_embed,
            "has_audio_embed": shape.shape.has_audio_embed,
            "is_link_roundup": shape.shape.is_link_roundup,
            "is_long_form": shape.shape.is_long_form,
            "is_short": shape.shape.is_short,
            "has_code": shape.shape.has_code,
            "has_math": shape.shape.has_math,
            "detected_at": detected_at(shape.detected_at_micros),
        }))
    } else {
        println!("has_video_embed:    {}", shape.shape.has_video_embed);
        println!("has_audio_embed:    {}", shape.shape.has_audio_embed);
        println!("is_link_roundup:    {}", shape.shape.is_link_roundup);
        println!("is_long_form:       {}", shape.shape.is_long_form);
        println!("is_short:           {}", shape.shape.is_short);
        println!("has_code:           {}", shape.shape.has_code);
        println!("has_math:           {}", shape.shape.has_math);
        println!(
            "detected_at:        {}",
            detected_at(shape.detected_at_micros)
        );
        Ok(())
    }
}

fn detected_at(micros: i64) -> String {
    format_validity(&Validity::from((micros, true)))
}
