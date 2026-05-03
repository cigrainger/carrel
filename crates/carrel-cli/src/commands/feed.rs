//! Feed subscription and fetch commands.

use clap::Subcommand;
use serde_json::json;
use time::OffsetDateTime;

use carrel_feeds::{DEFAULT_USER_AGENT, FetchResult, Fetcher, HttpHeaders, parse_feed};
use carrel_store::Store;
use carrel_store::feeds::{FeedFetchMetadata, FeedRecord, IngestStats};

use crate::config::Context;
use crate::error::{CliError, Result};
use crate::output;

/// Feed subcommands.
#[derive(Debug, Subcommand)]
pub enum FeedCommand {
    /// Subscribe to a feed URL.
    Add {
        /// Feed URL to subscribe to.
        url: String,
    },

    /// List feed subscriptions.
    List,

    /// Remove a feed subscription.
    Remove {
        /// Feed URL to remove.
        url: String,
    },

    /// Fetch one feed, all feeds, or feeds currently due.
    Fetch {
        /// Optional feed URL to fetch.
        url: Option<String>,

        /// Fetch every subscribed feed, ignoring adaptive intervals.
        #[arg(long)]
        all: bool,
    },
}

/// Run a feed subcommand.
pub async fn run(context: &Context, command: &FeedCommand) -> Result<()> {
    context.paths.require_initialized()?;
    let store = Store::open(&context.paths.store)?;
    store.migrate()?;

    match command {
        FeedCommand::Add { url } => add_feed(context, &store, url),
        FeedCommand::List => list_feeds(context, &store),
        FeedCommand::Remove { url } => remove_feed(context, &store, url),
        FeedCommand::Fetch { url, all } => fetch_feeds(context, &store, url.as_deref(), *all).await,
    }
}

fn add_feed(context: &Context, store: &Store, url: &str) -> Result<()> {
    let feed = store.add_feed(url)?;
    if context.json {
        output::print_json(&feed_json(&feed))
    } else {
        println!("Subscribed to {}", feed.url);
        Ok(())
    }
}

fn list_feeds(context: &Context, store: &Store) -> Result<()> {
    let feeds = store.list_feeds()?;
    if context.json {
        output::print_json(&json!(feeds.iter().map(feed_json).collect::<Vec<_>>()))
    } else {
        println!("url  title  interval  failures");
        if feeds.is_empty() {
            println!("(no feeds)");
        }
        for feed in feeds {
            println!(
                "{}  {}  {}s  {}",
                feed.url,
                feed.title.as_deref().unwrap_or("-"),
                feed.fetch_interval_seconds,
                feed.consecutive_failures
            );
        }
        Ok(())
    }
}

fn remove_feed(context: &Context, store: &Store, url: &str) -> Result<()> {
    let removed = store.remove_feed(url)?;
    if context.json {
        output::print_json(&json!({ "removed": removed }))
    } else if removed {
        println!("Removed {url}");
        Ok(())
    } else {
        println!("No subscription found for {url}");
        Ok(())
    }
}

async fn fetch_feeds(context: &Context, store: &Store, url: Option<&str>, all: bool) -> Result<()> {
    let feeds = feeds_to_fetch(store, url, all)?;
    let fetcher = Fetcher::new(DEFAULT_USER_AGENT)?;
    let mut reports = Vec::with_capacity(feeds.len());

    for feed in feeds {
        reports.push(fetch_one(store, &fetcher, &feed).await);
    }

    if context.json {
        output::print_json(&json!(
            reports.iter().map(fetch_report_json).collect::<Vec<_>>()
        ))
    } else {
        if reports.is_empty() {
            println!("No feeds due.");
        }
        for report in reports {
            match report.outcome {
                FetchOutcome::Updated { stats } => println!(
                    "{} updated: {} new, {} updated, {} entry errors",
                    report.url,
                    stats.new_items,
                    stats.updated_items,
                    stats.errors.len()
                ),
                FetchOutcome::NotModified => println!("{} not modified", report.url),
                FetchOutcome::HttpStatus { status } => {
                    println!("{} returned HTTP {}", report.url, status)
                }
                FetchOutcome::Error { message } => println!("{} failed: {}", report.url, message),
            }
        }
        Ok(())
    }
}

fn feeds_to_fetch(store: &Store, url: Option<&str>, all: bool) -> Result<Vec<FeedRecord>> {
    if let Some(url) = url {
        return store
            .get_feed(url)?
            .map(|feed| vec![feed])
            .ok_or_else(|| CliError::user(format!("{url} is not subscribed")));
    }

    if all {
        store.list_feeds().map_err(Into::into)
    } else {
        store.due_feeds(now_micros()).map_err(Into::into)
    }
}

async fn fetch_one(store: &Store, fetcher: &Fetcher, feed: &FeedRecord) -> FetchReport {
    let result = fetcher
        .fetch(
            &feed.url,
            feed.etag_header.as_deref(),
            feed.last_modified_header.as_deref(),
        )
        .await;

    match result {
        Ok(FetchResult::Updated { body, headers }) => {
            let metadata = feed_metadata(&headers);
            match parse_feed(&body, &feed.url) {
                Ok(parsed) => match store.ingest_feed(&feed.url, &parsed, &metadata) {
                    Ok(stats) => FetchReport::updated(feed.url.clone(), stats),
                    Err(error) => {
                        let _ = store.record_feed_fetch_failure(&feed.url, &metadata);
                        FetchReport::error(feed.url.clone(), error.to_string())
                    }
                },
                Err(error) => {
                    let _ = store.record_feed_fetch_failure(&feed.url, &metadata);
                    FetchReport::error(feed.url.clone(), error.to_string())
                }
            }
        }
        Ok(FetchResult::NotModified { headers }) => {
            let metadata = feed_metadata(&headers);
            match store.record_feed_not_modified(&feed.url, &metadata) {
                Ok(_) => FetchReport::not_modified(feed.url.clone()),
                Err(error) => FetchReport::error(feed.url.clone(), error.to_string()),
            }
        }
        Ok(FetchResult::GoneOrError { status, headers }) => {
            let metadata = feed_metadata(&headers);
            let _ = store.record_feed_fetch_failure(&feed.url, &metadata);
            FetchReport::http_status(feed.url.clone(), status)
        }
        Err(error) => {
            let _ = store.record_feed_fetch_failure(&feed.url, &FeedFetchMetadata::default());
            FetchReport::error(feed.url.clone(), error.to_string())
        }
    }
}

#[derive(Clone, Debug)]
struct FetchReport {
    url: String,
    outcome: FetchOutcome,
}

impl FetchReport {
    fn updated(url: String, stats: IngestStats) -> Self {
        Self {
            url,
            outcome: FetchOutcome::Updated { stats },
        }
    }

    fn not_modified(url: String) -> Self {
        Self {
            url,
            outcome: FetchOutcome::NotModified,
        }
    }

    fn http_status(url: String, status: u16) -> Self {
        Self {
            url,
            outcome: FetchOutcome::HttpStatus { status },
        }
    }

    fn error(url: String, message: String) -> Self {
        Self {
            url,
            outcome: FetchOutcome::Error { message },
        }
    }
}

#[derive(Clone, Debug)]
enum FetchOutcome {
    Updated { stats: IngestStats },
    NotModified,
    HttpStatus { status: u16 },
    Error { message: String },
}

fn feed_metadata(headers: &HttpHeaders) -> FeedFetchMetadata {
    FeedFetchMetadata {
        etag_header: headers.etag.clone(),
        last_modified_header: headers.last_modified.clone(),
        retry_after_seconds: headers.retry_after_seconds,
    }
}

fn feed_json(feed: &FeedRecord) -> serde_json::Value {
    json!({
        "url": feed.url,
        "title": feed.title,
        "description": feed.description,
        "last_fetched_micros": feed.last_fetched_micros,
        "last_modified_header": feed.last_modified_header,
        "etag_header": feed.etag_header,
        "fetch_interval_seconds": feed.fetch_interval_seconds,
        "consecutive_failures": feed.consecutive_failures,
        "folder": feed.folder,
        "auto_mark_read": feed.auto_mark_read,
    })
}

fn fetch_report_json(report: &FetchReport) -> serde_json::Value {
    match &report.outcome {
        FetchOutcome::Updated { stats } => json!({
            "url": report.url,
            "outcome": "updated",
            "new_items": stats.new_items,
            "updated_items": stats.updated_items,
            "entry_errors": stats.errors.iter().map(|error| {
                json!({ "feed_guid": error.feed_guid, "message": error.message })
            }).collect::<Vec<_>>(),
        }),
        FetchOutcome::NotModified => json!({
            "url": report.url,
            "outcome": "not_modified",
        }),
        FetchOutcome::HttpStatus { status } => json!({
            "url": report.url,
            "outcome": "http_status",
            "status": status,
        }),
        FetchOutcome::Error { message } => json!({
            "url": report.url,
            "outcome": "error",
            "message": message,
        }),
    }
}

fn now_micros() -> i64 {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    i64::try_from(nanos / 1_000).unwrap_or(i64::MAX)
}
