//! Feed management commands for the desktop shell.

use carrel_feeds::{DEFAULT_USER_AGENT, Fetcher};
use carrel_store::feeds::FeedRecord;
use serde::Serialize;
use tauri::State;
use time::{Duration, OffsetDateTime};
use tracing::{debug, warn};
use url::Url;

use crate::Result;
use crate::error::AppError;
use crate::feed_runner::{self, FetchOutcome, FetchReport};
use crate::state::{AppState, FeedStatusCounts, FeedStatusSnapshot};

/// List feed subscriptions.
#[tauri::command]
pub fn list_feeds(state: State<'_, AppState>) -> Result<Vec<FeedSummary>> {
    list_feeds_from_store(&state.store)
}

/// Subscribe to a feed URL.
#[tauri::command]
pub fn add_feed(state: State<'_, AppState>, url: String) -> Result<FeedSummary> {
    let feed = state.store.add_feed(&url)?;
    state.feed_status.set_idle(
        format!("Subscribed to {}", display_name(&feed)),
        FeedStatusCounts::default(),
    );
    Ok(FeedSummary::from_record(&feed, OffsetDateTime::now_utc()))
}

/// Remove a feed subscription. Existing items remain in the local library.
#[tauri::command]
pub fn remove_feed(state: State<'_, AppState>, url: String) -> Result<bool> {
    let removed = state.store.remove_feed(&url)?;
    if removed {
        state.feed_status.set_idle(
            "Feed removed; saved items remain",
            FeedStatusCounts::default(),
        );
    }
    Ok(removed)
}

/// Fetch one subscribed feed now.
#[tauri::command]
pub async fn fetch_feed(
    state: State<'_, AppState>,
    url: String,
) -> Result<Option<FeedFetchSummary>> {
    let app_state = state.inner().clone();
    let Some(feed) = app_state.store.get_feed(&url)? else {
        return Err(AppError::Feed(format!("{url} is not subscribed")));
    };

    Ok(fetch_records(app_state, vec![feed])
        .await?
        .into_iter()
        .next())
}

/// Fetch every subscribed feed now.
#[tauri::command]
pub async fn fetch_all_feeds(state: State<'_, AppState>) -> Result<Vec<FeedFetchSummary>> {
    let app_state = state.inner().clone();
    let feeds = app_state.store.list_feeds()?;
    fetch_records(app_state, feeds).await
}

/// Return current feed fetch status for the status strip.
#[tauri::command]
pub fn feed_status(state: State<'_, AppState>) -> FeedStatusSnapshot {
    state.feed_status.snapshot()
}

pub(crate) fn list_feeds_from_store(store: &carrel_store::Store) -> Result<Vec<FeedSummary>> {
    let now = OffsetDateTime::now_utc();
    store
        .list_feeds()?
        .iter()
        .map(|feed| Ok(FeedSummary::from_record(feed, now)))
        .collect()
}

pub(crate) async fn fetch_due_feeds(app_state: AppState) -> Result<Vec<FeedFetchSummary>> {
    let feeds = app_state.store.due_feeds(feed_runner::now_micros())?;
    if feeds.is_empty() {
        return Ok(Vec::new());
    }

    fetch_records(app_state, feeds).await
}

pub(crate) async fn fetch_records(
    app_state: AppState,
    feeds: Vec<FeedRecord>,
) -> Result<Vec<FeedFetchSummary>> {
    if feeds.is_empty() {
        app_state
            .feed_status
            .set_idle("No feeds subscribed", FeedStatusCounts::default());
        return Ok(Vec::new());
    }

    let feed_fetch_lock = app_state.feed_fetch_lock.clone();
    let _guard = feed_fetch_lock.lock().await;
    tauri::async_runtime::spawn_blocking(move || fetch_records_blocking(app_state, feeds))
        .await
        .map_err(|error| AppError::Feed(format!("feed worker failed: {error}")))?
}

fn fetch_records_blocking(
    app_state: AppState,
    feeds: Vec<FeedRecord>,
) -> Result<Vec<FeedFetchSummary>> {
    let status_label = if feeds.len() == 1 {
        format!("Fetching {}", display_name(&feeds[0]))
    } else {
        "Fetching feeds".to_string()
    };
    app_state.feed_status.set_fetching(status_label);

    let fetcher = Fetcher::new(DEFAULT_USER_AGENT).map_err(|error| {
        let message = error.to_string();
        app_state.feed_status.set_idle(
            format!("Feed fetch failed: {message}"),
            feed_failure_counts(),
        );
        AppError::Feed(message)
    })?;
    let mut reports = Vec::with_capacity(feeds.len());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::Feed(format!("could not start feed worker: {error}")))?;

    for feed in feeds {
        debug!(feed_url = %feed.url, "fetching feed");
        reports.push(runtime.block_on(feed_runner::fetch_one(
            &app_state.store,
            &app_state.blobs,
            &fetcher,
            &feed,
        )));
    }

    let counts = status_counts(&reports);
    let message = status_message(&reports, counts);
    app_state.feed_status.set_idle(message, counts);

    Ok(reports.iter().map(FeedFetchSummary::from_report).collect())
}

/// Feed subscription DTO returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSummary {
    /// Canonical feed URL.
    pub url: String,
    /// Feed title, if known.
    pub title: Option<String>,
    /// Feed description, if known.
    pub description: Option<String>,
    /// Display label for navigation.
    pub display_name: String,
    /// Human-readable last fetched label.
    pub last_fetched_label: String,
    /// Current adaptive fetch interval.
    pub fetch_interval_seconds: i64,
    /// Consecutive fetch failure count.
    pub consecutive_failures: i64,
    /// Optional user folder.
    pub folder: Option<String>,
    /// Whether feed items are auto-marked read.
    pub auto_mark_read: bool,
}

impl FeedSummary {
    fn from_record(feed: &FeedRecord, now: OffsetDateTime) -> Self {
        Self {
            url: feed.url.clone(),
            title: feed.title.clone(),
            description: feed.description.clone(),
            display_name: display_name(feed),
            last_fetched_label: last_fetched_label(feed.last_fetched_micros, now),
            fetch_interval_seconds: feed.fetch_interval_seconds,
            consecutive_failures: feed.consecutive_failures,
            folder: feed.folder.clone(),
            auto_mark_read: feed.auto_mark_read,
        }
    }
}

/// Summary of one feed fetch returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedFetchSummary {
    /// Canonical feed URL.
    pub url: String,
    /// Stable outcome label.
    pub outcome: String,
    /// Short status-strip style message.
    pub message: String,
    /// Newly discovered items.
    pub new_items: usize,
    /// Existing items updated.
    pub updated_items: usize,
    /// Entry ingest errors.
    pub entry_errors: usize,
    /// Content extraction/cache errors.
    pub content_errors: usize,
    /// Whole-feed error count.
    pub feed_errors: usize,
}

impl FeedFetchSummary {
    fn from_report(report: &FetchReport) -> Self {
        match &report.outcome {
            FetchOutcome::Updated { stats } => Self {
                url: report.url.clone(),
                outcome: "updated".to_string(),
                message: format!(
                    "{} updated: {} new, {} updated",
                    report.url, stats.new_items, stats.updated_items
                ),
                new_items: stats.new_items,
                updated_items: stats.updated_items,
                entry_errors: stats.errors.len(),
                content_errors: report.content_errors.len(),
                feed_errors: 0,
            },
            FetchOutcome::NotModified => Self {
                url: report.url.clone(),
                outcome: "not_modified".to_string(),
                message: format!("{} not modified", report.url),
                new_items: 0,
                updated_items: 0,
                entry_errors: 0,
                content_errors: 0,
                feed_errors: 0,
            },
            FetchOutcome::HttpStatus { status } => Self {
                url: report.url.clone(),
                outcome: "http_status".to_string(),
                message: format!("{} returned HTTP {status}", report.url),
                new_items: 0,
                updated_items: 0,
                entry_errors: 0,
                content_errors: 0,
                feed_errors: 1,
            },
            FetchOutcome::Error { message } => Self {
                url: report.url.clone(),
                outcome: "error".to_string(),
                message: format!("{} failed: {message}", report.url),
                new_items: 0,
                updated_items: 0,
                entry_errors: 0,
                content_errors: 0,
                feed_errors: 1,
            },
        }
    }
}

fn display_name(feed: &FeedRecord) -> String {
    if let Some(title) = feed
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    {
        return title.to_string();
    }

    Url::parse(&feed.url)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .unwrap_or_else(|| feed.url.clone())
}

fn last_fetched_label(last_fetched_micros: Option<i64>, now: OffsetDateTime) -> String {
    let Some(last_fetched_micros) = last_fetched_micros else {
        return "never".to_string();
    };
    let last_fetched =
        OffsetDateTime::from_unix_timestamp(last_fetched_micros / 1_000_000).unwrap_or(now);
    let age = now - last_fetched;

    if age <= Duration::ZERO {
        "now".to_string()
    } else if age < Duration::hours(1) {
        format!("{}m ago", age.whole_minutes().max(1))
    } else if age < Duration::days(1) {
        format!("{}h ago", age.whole_hours())
    } else if age < Duration::days(7) {
        format!("{}d ago", age.whole_days())
    } else {
        let date = last_fetched.date();
        format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            u8::from(date.month()),
            date.day()
        )
    }
}

fn feed_failure_counts() -> FeedStatusCounts {
    FeedStatusCounts {
        feed_errors: 1,
        ..FeedStatusCounts::default()
    }
}

fn status_counts(reports: &[FetchReport]) -> FeedStatusCounts {
    let mut counts = FeedStatusCounts::default();
    for report in reports {
        match &report.outcome {
            FetchOutcome::Updated { stats } => {
                counts.new_items += stats.new_items;
                counts.updated_items += stats.updated_items;
                counts.entry_errors += stats.errors.len();
                counts.content_errors += report.content_errors.len();
            }
            FetchOutcome::NotModified => {}
            FetchOutcome::HttpStatus { .. } | FetchOutcome::Error { .. } => {
                counts.feed_errors += 1;
            }
        }
    }
    counts
}

fn status_message(reports: &[FetchReport], counts: FeedStatusCounts) -> String {
    if reports.is_empty() {
        return "No feeds due".to_string();
    }

    let issue_count = counts.feed_errors + counts.entry_errors + counts.content_errors;
    if issue_count > 0 {
        warn!(
            feed_errors = counts.feed_errors,
            entry_errors = counts.entry_errors,
            content_errors = counts.content_errors,
            "feed fetch completed with issues"
        );
        return format!("Feed fetch finished with {issue_count} issues");
    }

    if counts.new_items > 0 || counts.updated_items > 0 {
        return format!(
            "Feeds updated: {} new, {} updated",
            counts.new_items, counts.updated_items
        );
    }

    "Feeds checked".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_summary_uses_title_or_host() {
        let titled = FeedRecord {
            url: "https://example.com/feed.xml".to_string(),
            title: Some("Example Feed".to_string()),
            description: None,
            last_fetched_micros: None,
            last_modified_header: None,
            etag_header: None,
            fetch_interval_seconds: 900,
            consecutive_failures: 0,
            folder: None,
            auto_mark_read: false,
        };
        let untitled = FeedRecord {
            title: None,
            ..titled.clone()
        };

        assert_eq!(display_name(&titled), "Example Feed");
        assert_eq!(display_name(&untitled), "example.com");
    }

    #[test]
    fn status_message_stays_quiet_for_empty_fetches() {
        let reports = vec![FetchReport {
            url: "https://example.com/feed.xml".to_string(),
            outcome: FetchOutcome::NotModified,
            content_errors: Vec::new(),
        }];

        assert_eq!(
            status_message(&reports, status_counts(&reports)),
            "Feeds checked"
        );
    }
}
