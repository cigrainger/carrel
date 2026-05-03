//! Feed-domain data structures shared by fetch, store, and CLI layers.

/// Default fetch interval for a newly subscribed feed: one hour.
pub const DEFAULT_FETCH_INTERVAL_SECONDS: i64 = 60 * 60;

/// Maximum adaptive fetch interval for normal subscribed feeds: one day.
pub const MAX_FETCH_INTERVAL_SECONDS: i64 = 24 * 60 * 60;

/// Consecutive failures after which a feed is skipped by normal due-feed fetches.
pub const SUSPENDED_FAILURE_THRESHOLD: i64 = 30;

/// A parsed syndication feed in Carrel's normalized shape.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ParsedFeed {
    /// Feed title, if present.
    pub title: Option<String>,
    /// Feed description or subtitle, if present.
    pub description: Option<String>,
    /// Feed-level language, if present.
    pub language: Option<String>,
    /// Normalized entries contained in the feed.
    pub entries: Vec<ParsedEntry>,
}

/// A parsed feed entry in Carrel's normalized article shape.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ParsedEntry {
    /// Stable per-feed entry identifier.
    pub feed_guid: String,
    /// Entry title, if present.
    pub title: Option<String>,
    /// Entry authors, normalized to display names.
    pub authors: Vec<String>,
    /// Canonical or best-known article URL, if present.
    pub url: Option<String>,
    /// Publication timestamp in Unix microseconds, if present.
    pub published_at_micros: Option<i64>,
    /// Entry language, if present.
    pub language: Option<String>,
    /// Summary/dek HTML or text, if present.
    pub summary_html: Option<String>,
    /// Embedded full content HTML or text, if present.
    pub content_html: Option<String>,
}

impl ParsedEntry {
    /// Return the best identifier for deriving a stable item id.
    pub fn canonical_identifier(&self, feed_url: &str) -> String {
        self.url
            .clone()
            .unwrap_or_else(|| feed_guid_identifier(feed_url, &self.feed_guid))
    }
}

/// Scope a feed GUID by feed URL to avoid collisions between feeds.
pub fn feed_guid_identifier(feed_url: &str, feed_guid: &str) -> String {
    format!("feed_guid:{feed_url}:{feed_guid}")
}

/// Readable article content extracted for a feed entry before store ingest.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ExtractedEntryContent {
    /// Entry GUID this content belongs to, before feed URL scoping.
    pub feed_guid: String,
    /// Extracted or embedded article title, if the extractor found one.
    pub title: Option<String>,
    /// Extracted byline text, if present.
    pub byline: Option<String>,
    /// Blob id containing sanitized readable HTML.
    pub blob_id: String,
    /// Number of bytes in the stored readable HTML blob.
    pub byte_size: i64,
    /// Extractor label persisted in `item_content.extracted_with`.
    pub extracted_with: String,
    /// Word count of the sanitized readable body.
    pub word_count: i64,
    /// Estimated reading time in whole minutes.
    pub estimated_read_minutes: i64,
    /// Extracted or inherited language tag, if known.
    pub language: Option<String>,
    /// Source site name, if known.
    pub site_name: Option<String>,
}
