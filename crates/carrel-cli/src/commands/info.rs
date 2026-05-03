//! `carrel info` implementation.

use carrel_store::Store;
use cozo::{DataValue, Num};
use serde_json::json;

use crate::config::Context;
use crate::error::{CliError, Result};
use crate::output::{format_validity, print_json};

/// Run `carrel info`.
pub fn run(context: &Context) -> Result<()> {
    context.paths.require_initialized()?;
    let store = Store::open(&context.paths.store)?;
    let info = collect_info(context, &store)?;

    if context.json {
        print_json(&json!({
            "data_dir": info.data_dir,
            "schema_version": info.schema_version,
            "master_pubkey": info.master_pubkey,
            "device_pubkey": info.device_pubkey,
            "device_authorized_at": info.device_authorized_at,
            "items": info.items,
            "feeds": info.feeds,
            "highlights": info.highlights,
            "peers": info.peers,
        }))?;
    } else {
        println!("Data dir:      {}", info.data_dir);
        println!("Schema:        v{}", info.schema_version);
        println!(
            "Master pubkey: {}",
            info.master_pubkey.unwrap_or_else(|| "unknown".to_string())
        );
        match (info.device_pubkey, info.device_authorized_at) {
            (Some(device), Some(authorized_at)) => {
                println!("Device pubkey: {} (authorized {})", device, authorized_at);
            }
            (Some(device), None) => println!("Device pubkey: {}", device),
            (None, _) => println!("Device pubkey: unknown"),
        }
        println!("Items:         {}", info.items);
        println!("Feeds:         {}", info.feeds);
        println!("Highlights:    {}", info.highlights);
        println!("Peers:         {}", info.peers);
    }

    Ok(())
}

#[derive(Debug)]
struct Info {
    data_dir: String,
    schema_version: u32,
    master_pubkey: Option<String>,
    device_pubkey: Option<String>,
    device_authorized_at: Option<String>,
    items: i64,
    feeds: i64,
    highlights: i64,
    peers: i64,
}

fn collect_info(context: &Context, store: &Store) -> Result<Info> {
    let schema_version = store.current_schema_version()?;
    let (master_pubkey, device_pubkey, device_authorized_at) = identity_info(store)?;

    Ok(Info {
        data_dir: context.data_dir.display().to_string(),
        schema_version,
        master_pubkey,
        device_pubkey,
        device_authorized_at,
        items: count(store, "item", "id")?,
        feeds: count(store, "feed", "url")?,
        highlights: count(store, "highlight", "id")?,
        peers: count(store, "peer", "pubkey")?,
    })
}

fn identity_info(store: &Store) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let rows = store.query(
        r#"
        ?[master_pubkey, device_pubkey, authorized_at] :=
            *device_authorization{master_pubkey, device_pubkey, authorized_at}
        :limit 1
        "#,
    )?;

    let Some(row) = rows.rows.first() else {
        return Ok((None, None, None));
    };

    let master = bytes_hex(row.first(), "device_authorization.master_pubkey")?;
    let device = bytes_hex(row.get(1), "device_authorization.device_pubkey")?;
    let authorized_at = match row.get(2) {
        Some(DataValue::Validity(validity)) => Some(format_validity(validity)),
        Some(DataValue::Null) | None => None,
        Some(other) => {
            return Err(CliError::internal(format!(
                "unexpected device_authorization.authorized_at: {other:?}"
            )));
        }
    };

    Ok((Some(master), Some(device), authorized_at))
}

fn count(store: &Store, relation: &str, key: &str) -> Result<i64> {
    let rows = store.query(&format!("?[count({key})] := *{relation}{{{key}}}"))?;
    let value = rows
        .rows
        .first()
        .and_then(|row| row.first())
        .ok_or_else(|| {
            CliError::internal(format!("count query for {relation} returned no rows"))
        })?;

    match value {
        DataValue::Num(Num::Int(count)) => Ok(*count),
        other => Err(CliError::internal(format!(
            "count query for {relation} returned {other:?}"
        ))),
    }
}

fn bytes_hex(value: Option<&DataValue>, context: &'static str) -> Result<String> {
    match value {
        Some(DataValue::Bytes(bytes)) => Ok(hex::encode(bytes)),
        Some(other) => Err(CliError::internal(format!(
            "unexpected {context}: {other:?}"
        ))),
        None => Err(CliError::internal(format!("missing {context}"))),
    }
}
