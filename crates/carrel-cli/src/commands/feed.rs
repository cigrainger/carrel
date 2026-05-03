//! Feed subscription and fetch commands.

use clap::Subcommand;
use serde_json::json;
use time::OffsetDateTime;

use carrel_core::feed::{ExtractedEntryContent, ParsedEntry, ParsedFeed};
use carrel_feeds::{DEFAULT_USER_AGENT, FetchResult, Fetcher, HttpHeaders, parse_feed};
use carrel_feeds::{detect_shape, extract_embedded_html, extract_from_url, rewrite_images};
use carrel_store::Store;
use carrel_store::blobs::BlobStore;
use carrel_store::feeds::{EntryError, FeedFetchMetadata, FeedRecord, IngestStats};

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
    let blobs = BlobStore::open(&context.paths.blobs);
    let mut reports = Vec::with_capacity(feeds.len());

    for feed in feeds {
        reports.push(fetch_one(store, &blobs, &fetcher, &feed).await);
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
                    "{} updated: {} new, {} updated, {} entry errors, {} content errors",
                    report.url,
                    stats.new_items,
                    stats.updated_items,
                    stats.errors.len(),
                    report.content_errors.len()
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

async fn fetch_one(
    store: &Store,
    blobs: &BlobStore,
    fetcher: &Fetcher,
    feed: &FeedRecord,
) -> FetchReport {
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
                Ok(parsed) => {
                    let content = extract_feed_content(blobs, fetcher, &feed.url, &parsed).await;
                    match store.ingest_feed_with_content(
                        &feed.url,
                        &parsed,
                        &metadata,
                        &content.contents,
                    ) {
                        Ok(stats) => {
                            FetchReport::updated(feed.url.clone(), stats, content.content_errors)
                        }
                        Err(error) => {
                            let _ = store.record_feed_fetch_failure(&feed.url, &metadata);
                            FetchReport::error(feed.url.clone(), error.to_string())
                        }
                    }
                }
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
    content_errors: Vec<EntryError>,
}

impl FetchReport {
    fn updated(url: String, stats: IngestStats, content_errors: Vec<EntryError>) -> Self {
        Self {
            url,
            outcome: FetchOutcome::Updated { stats },
            content_errors,
        }
    }

    fn not_modified(url: String) -> Self {
        Self {
            url,
            outcome: FetchOutcome::NotModified,
            content_errors: Vec::new(),
        }
    }

    fn http_status(url: String, status: u16) -> Self {
        Self {
            url,
            outcome: FetchOutcome::HttpStatus { status },
            content_errors: Vec::new(),
        }
    }

    fn error(url: String, message: String) -> Self {
        Self {
            url,
            outcome: FetchOutcome::Error { message },
            content_errors: Vec::new(),
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

#[derive(Default)]
struct ExtractedFeedContent {
    contents: Vec<ExtractedEntryContent>,
    content_errors: Vec<EntryError>,
}

async fn extract_feed_content(
    blobs: &BlobStore,
    fetcher: &Fetcher,
    feed_url: &str,
    parsed: &ParsedFeed,
) -> ExtractedFeedContent {
    let mut extracted = ExtractedFeedContent::default();

    for entry in &parsed.entries {
        match extract_entry_content(blobs, fetcher, feed_url, parsed, entry).await {
            Ok(result) => {
                if let Some(content) = result.content {
                    extracted.contents.push(content);
                }
                extracted
                    .content_errors
                    .extend(result.warnings.into_iter().map(|message| EntryError {
                        feed_guid: entry.feed_guid.clone(),
                        message,
                    }));
            }
            Err(message) => extracted.content_errors.push(EntryError {
                feed_guid: entry.feed_guid.clone(),
                message,
            }),
        }
    }

    extracted
}

async fn extract_entry_content(
    blobs: &BlobStore,
    fetcher: &Fetcher,
    feed_url: &str,
    parsed: &ParsedFeed,
    entry: &ParsedEntry,
) -> std::result::Result<EntryContentExtraction, String> {
    let base_url = entry.url.as_deref().unwrap_or(feed_url);
    let mut article = if let Some(html) = entry.content_html.as_deref() {
        extract_embedded_html(html, base_url).map_err(|error| error.to_string())?
    } else if let Some(url) = entry.url.as_deref() {
        extract_from_url(fetcher, url)
            .await
            .map_err(|error| error.to_string())?
    } else {
        return Ok(EntryContentExtraction {
            content: None,
            warnings: Vec::new(),
        });
    };

    let rewritten = rewrite_images(&article.content_html, base_url, fetcher, |bytes| {
        blobs.put_blocking(bytes).map(|id| id.to_string())
    })
    .await;
    let warnings = rewritten
        .failures
        .iter()
        .map(|failure| format!("image {} was not cached: {}", failure.url, failure.message))
        .collect();
    article.content_html = rewritten.html;
    let shape = detect_shape(&article.content_html, article.word_count);

    let blob_id = blobs
        .put(article.content_html.as_bytes())
        .await
        .map_err(|error| error.to_string())?
        .to_string();

    Ok(EntryContentExtraction {
        content: Some(ExtractedEntryContent {
            feed_guid: entry.feed_guid.clone(),
            title: article.title,
            byline: article.byline,
            blob_id,
            byte_size: i64::try_from(article.content_html.len()).unwrap_or(i64::MAX),
            extracted_with: article.extractor.as_str().to_string(),
            word_count: i64::try_from(article.word_count).unwrap_or(i64::MAX),
            estimated_read_minutes: i64::from(article.estimated_read_minutes),
            language: article.language.or_else(|| entry.language.clone()),
            site_name: article.site_name.or_else(|| parsed.title.clone()),
            shape,
        }),
        warnings,
    })
}

struct EntryContentExtraction {
    content: Option<ExtractedEntryContent>,
    warnings: Vec<String>,
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
            "content_errors": report.content_errors.iter().map(|error| {
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
