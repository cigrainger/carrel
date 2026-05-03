//! Store-backed feed subscription and ingest operations.

use std::collections::BTreeMap;

use carrel_core::feed::{
    DEFAULT_FETCH_INTERVAL_SECONDS, MAX_FETCH_INTERVAL_SECONDS, ParsedEntry, ParsedFeed,
    SUSPENDED_FAILURE_THRESHOLD, feed_guid_identifier,
};
use cozo::{DataValue, Num, Validity};
use time::OffsetDateTime;

use crate::ids::{canonicalize_external_identifier, id_for_external};
use crate::{Result, Store, StoreError};

/// Feed subscription row decoded from the store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedRecord {
    /// Canonical feed URL.
    pub url: String,
    /// Feed title, if known.
    pub title: Option<String>,
    /// Feed description, if known.
    pub description: Option<String>,
    /// Last fetch timestamp in Unix microseconds.
    pub last_fetched_micros: Option<i64>,
    /// Last-Modified header retained for conditional GET.
    pub last_modified_header: Option<String>,
    /// ETag header retained for conditional GET.
    pub etag_header: Option<String>,
    /// Current adaptive fetch interval.
    pub fetch_interval_seconds: i64,
    /// Consecutive fetch failure count.
    pub consecutive_failures: i64,
    /// Optional user folder.
    pub folder: Option<String>,
    /// Whether future UI should auto-mark items from this feed as read.
    pub auto_mark_read: bool,
}

/// HTTP metadata persisted after a feed fetch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeedFetchMetadata {
    /// ETag header returned by the server.
    pub etag_header: Option<String>,
    /// Last-Modified header returned by the server.
    pub last_modified_header: Option<String>,
    /// Retry-After delay in seconds.
    pub retry_after_seconds: Option<i64>,
}

/// Feed ingest summary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestStats {
    /// Number of newly discovered items.
    pub new_items: usize,
    /// Number of existing items updated from the feed.
    pub updated_items: usize,
    /// Per-entry ingest errors that did not abort the whole feed ingest.
    pub errors: Vec<EntryError>,
}

/// A non-fatal error while ingesting a feed entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryError {
    /// Entry GUID that failed to ingest.
    pub feed_guid: String,
    /// Developer-facing explanation.
    pub message: String,
}

impl Store {
    /// Subscribe to a feed URL using default local subscription settings.
    pub fn add_feed(&self, url: &str) -> Result<FeedRecord> {
        let url = canonical_feed_url(url)?;
        let record = FeedRecord {
            url,
            title: None,
            description: None,
            last_fetched_micros: None,
            last_modified_header: None,
            etag_header: None,
            fetch_interval_seconds: DEFAULT_FETCH_INTERVAL_SECONDS,
            consecutive_failures: 0,
            folder: None,
            auto_mark_read: false,
        };
        self.put_feed(&record)?;
        Ok(record)
    }

    /// Remove a feed subscription by URL.
    pub fn remove_feed(&self, url: &str) -> Result<bool> {
        let url = canonical_feed_url(url)?;
        let existed = self.get_feed(&url)?.is_some();
        self.query_with_params(
            r#"
            ?[url] := url = $url
            :rm feed {url}
            "#,
            BTreeMap::from([("url".to_string(), DataValue::from(url.as_str()))]),
        )?;
        Ok(existed)
    }

    /// Return all subscribed feeds sorted by URL.
    pub fn list_feeds(&self) -> Result<Vec<FeedRecord>> {
        let rows = self.query(
            r#"
            ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] :=
                *feed{url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}
            :sort url
            "#,
        )?;
        rows.rows
            .iter()
            .map(|row| decode_feed_row(row))
            .collect::<Result<Vec<_>>>()
    }

    /// Return a feed subscription by URL, if present.
    pub fn get_feed(&self, url: &str) -> Result<Option<FeedRecord>> {
        let url = canonical_feed_url(url)?;
        let rows = self.query_with_params(
            r#"
            ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] :=
                *feed{url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read},
                url = $url
            :limit 1
            "#,
            BTreeMap::from([("url".to_string(), DataValue::from(url.as_str()))]),
        )?;
        rows.rows
            .first()
            .map(|row| decode_feed_row(row))
            .transpose()
    }

    /// Return feeds due for a normal scheduled fetch.
    pub fn due_feeds(&self, now_micros: i64) -> Result<Vec<FeedRecord>> {
        Ok(self
            .list_feeds()?
            .into_iter()
            .filter(|feed| feed.consecutive_failures < SUSPENDED_FAILURE_THRESHOLD)
            .filter(|feed| match feed.last_fetched_micros {
                Some(last_fetched) => {
                    last_fetched + feed.fetch_interval_seconds.saturating_mul(1_000_000)
                        <= now_micros
                }
                None => true,
            })
            .collect())
    }

    /// Record a 304 Not Modified fetch.
    pub fn record_feed_not_modified(
        &self,
        url: &str,
        metadata: &FeedFetchMetadata,
    ) -> Result<FeedRecord> {
        let mut feed = self.ensure_feed(url)?;
        feed.last_fetched_micros = Some(now_micros());
        if let Some(etag) = &metadata.etag_header {
            feed.etag_header = Some(etag.clone());
        }
        if let Some(last_modified) = &metadata.last_modified_header {
            feed.last_modified_header = Some(last_modified.clone());
        }
        feed.fetch_interval_seconds =
            (feed.fetch_interval_seconds * 2).min(MAX_FETCH_INTERVAL_SECONDS);
        feed.consecutive_failures = 0;
        self.put_feed(&feed)?;
        Ok(feed)
    }

    /// Record a non-success fetch status.
    pub fn record_feed_fetch_failure(
        &self,
        url: &str,
        metadata: &FeedFetchMetadata,
    ) -> Result<FeedRecord> {
        let mut feed = self.ensure_feed(url)?;
        feed.last_fetched_micros = Some(now_micros());
        feed.consecutive_failures = feed.consecutive_failures.saturating_add(1);
        feed.fetch_interval_seconds = failure_interval_seconds(&feed, metadata.retry_after_seconds);
        self.put_feed(&feed)?;
        Ok(feed)
    }

    /// Ingest parsed feed entries and update the subscription metadata.
    pub fn ingest_feed(
        &self,
        feed_url: &str,
        parsed: &ParsedFeed,
        metadata: &FeedFetchMetadata,
    ) -> Result<IngestStats> {
        let feed_url = canonical_feed_url(feed_url)?;
        let mut stats = IngestStats::default();

        for entry in &parsed.entries {
            match self.ingest_entry(&feed_url, parsed, entry) {
                Ok(true) => stats.new_items += 1,
                Ok(false) => stats.updated_items += 1,
                Err(error) => stats.errors.push(EntryError {
                    feed_guid: entry.feed_guid.clone(),
                    message: error.to_string(),
                }),
            }
        }

        let mut feed = self.ensure_feed(&feed_url)?;
        feed.title = parsed.title.clone().or(feed.title);
        feed.description = parsed.description.clone().or(feed.description);
        feed.last_fetched_micros = Some(now_micros());
        if let Some(etag) = &metadata.etag_header {
            feed.etag_header = Some(etag.clone());
        }
        if let Some(last_modified) = &metadata.last_modified_header {
            feed.last_modified_header = Some(last_modified.clone());
        }
        feed.consecutive_failures = 0;
        feed.fetch_interval_seconds = if stats.new_items == 0 && stats.updated_items == 0 {
            (feed.fetch_interval_seconds * 2).min(MAX_FETCH_INTERVAL_SECONDS)
        } else {
            DEFAULT_FETCH_INTERVAL_SECONDS
        };
        self.put_feed(&feed)?;

        Ok(stats)
    }

    fn ingest_entry(
        &self,
        feed_url: &str,
        parsed: &ParsedFeed,
        entry: &ParsedEntry,
    ) -> Result<bool> {
        let scoped_guid = feed_guid_identifier(feed_url, &entry.feed_guid);
        let canonical_url = entry.url.as_deref().map(canonicalize_external_identifier);
        let existing_item_id = self
            .find_item_by_identifier("url", canonical_url.as_deref())?
            .or(self.find_item_by_identifier("feed_guid", Some(&scoped_guid))?);
        let item_id = existing_item_id
            .unwrap_or_else(|| id_for_external(&entry.canonical_identifier(feed_url)));
        let is_new = !self.item_exists(&item_id)?;

        self.put_item(feed_url, parsed, entry, &item_id, canonical_url.as_deref())?;
        if let Some(url) = canonical_url.as_deref() {
            self.put_identifier(&item_id, "url", url, true)?;
        }
        self.put_identifier(&item_id, "feed_guid", &scoped_guid, canonical_url.is_none())?;

        Ok(is_new)
    }

    fn put_feed(&self, feed: &FeedRecord) -> Result<()> {
        self.query_with_params(
            r#"
            ?[url, title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read] :=
                url = $url,
                title = $title,
                description = $description,
                last_fetched = $last_fetched,
                last_modified_header = $last_modified_header,
                etag_header = $etag_header,
                fetch_interval_seconds = $fetch_interval_seconds,
                consecutive_failures = $consecutive_failures,
                folder = $folder,
                auto_mark_read = $auto_mark_read
            :put feed {url => title, description, last_fetched, last_modified_header, etag_header, fetch_interval_seconds, consecutive_failures, folder, auto_mark_read}
            "#,
            BTreeMap::from([
                ("url".to_string(), DataValue::from(feed.url.as_str())),
                ("title".to_string(), option_string(&feed.title)),
                ("description".to_string(), option_string(&feed.description)),
                (
                    "last_fetched".to_string(),
                    option_validity(feed.last_fetched_micros),
                ),
                (
                    "last_modified_header".to_string(),
                    option_string(&feed.last_modified_header),
                ),
                ("etag_header".to_string(), option_string(&feed.etag_header)),
                (
                    "fetch_interval_seconds".to_string(),
                    DataValue::Num(Num::Int(feed.fetch_interval_seconds)),
                ),
                (
                    "consecutive_failures".to_string(),
                    DataValue::Num(Num::Int(feed.consecutive_failures)),
                ),
                ("folder".to_string(), option_string(&feed.folder)),
                (
                    "auto_mark_read".to_string(),
                    DataValue::Bool(feed.auto_mark_read),
                ),
            ]),
        )?;
        Ok(())
    }

    fn ensure_feed(&self, url: &str) -> Result<FeedRecord> {
        match self.get_feed(url)? {
            Some(feed) => Ok(feed),
            None => self.add_feed(url),
        }
    }

    fn put_item(
        &self,
        feed_url: &str,
        parsed: &ParsedFeed,
        entry: &ParsedEntry,
        item_id: &str,
        canonical_url: Option<&str>,
    ) -> Result<()> {
        let title = entry
            .title
            .clone()
            .or_else(|| canonical_url.map(ToString::to_string))
            .unwrap_or_else(|| entry.feed_guid.clone());
        let byline = if entry.authors.is_empty() {
            None
        } else {
            Some(entry.authors.join(", "))
        };

        self.query_with_params(
            r#"
            {
                ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] :=
                    id = $id,
                    kind = 'article',
                    title = $title,
                    creators = $creators,
                    primary_url = $primary_url,
                    published_at = $published_at,
                    language = $language,
                    summary = $summary,
                    discovered_at = $discovered_at
                :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at}
            }
            {
                ?[item_id, feed_url, word_count, estimated_read_minutes, site_name, byline] :=
                    item_id = $id,
                    feed_url = $feed_url,
                    word_count = null,
                    estimated_read_minutes = null,
                    site_name = $site_name,
                    byline = $byline
                :put item_article {item_id => feed_url, word_count, estimated_read_minutes, site_name, byline}
            }
            "#,
            BTreeMap::from([
                ("id".to_string(), DataValue::from(item_id)),
                ("title".to_string(), DataValue::from(title.as_str())),
                (
                    "creators".to_string(),
                    DataValue::List(
                        entry
                            .authors
                            .iter()
                            .map(|author| DataValue::from(author.as_str()))
                            .collect(),
                    ),
                ),
                ("primary_url".to_string(), option_str(canonical_url)),
                (
                    "published_at".to_string(),
                    option_validity(entry.published_at_micros),
                ),
                (
                    "language".to_string(),
                    option_string(&entry.language.clone().or_else(|| parsed.language.clone())),
                ),
                ("summary".to_string(), option_string(&entry.summary_html)),
                ("discovered_at".to_string(), validity_now()),
                ("feed_url".to_string(), DataValue::from(feed_url)),
                ("site_name".to_string(), option_string(&parsed.title)),
                ("byline".to_string(), option_string(&byline)),
            ]),
        )?;
        Ok(())
    }

    fn put_identifier(
        &self,
        item_id: &str,
        scheme: &str,
        value: &str,
        is_canonical: bool,
    ) -> Result<()> {
        self.query_with_params(
            r#"
            ?[item_id, scheme, value, is_canonical, discovered_at] :=
                item_id = $item_id,
                scheme = $scheme,
                value = $value,
                is_canonical = $is_canonical,
                discovered_at = $discovered_at
            :put item_identifier {item_id, scheme, value => is_canonical, discovered_at}
            "#,
            BTreeMap::from([
                ("item_id".to_string(), DataValue::from(item_id)),
                ("scheme".to_string(), DataValue::from(scheme)),
                ("value".to_string(), DataValue::from(value)),
                ("is_canonical".to_string(), DataValue::Bool(is_canonical)),
                ("discovered_at".to_string(), validity_now()),
            ]),
        )?;
        Ok(())
    }

    fn find_item_by_identifier(&self, scheme: &str, value: Option<&str>) -> Result<Option<String>> {
        let Some(value) = value else {
            return Ok(None);
        };
        let rows = self.query_with_params(
            r#"
            ?[item_id] :=
                *item_identifier{item_id, scheme, value},
                scheme = $scheme,
                value = $value
            :limit 1
            "#,
            BTreeMap::from([
                ("scheme".to_string(), DataValue::from(scheme)),
                ("value".to_string(), DataValue::from(value)),
            ]),
        )?;
        rows.rows
            .first()
            .and_then(|row| row.first())
            .map(value_as_string)
            .transpose()
    }

    fn item_exists(&self, item_id: &str) -> Result<bool> {
        let rows = self.query_with_params(
            r#"
            ?[id] := *item{id}, id = $id
            :limit 1
            "#,
            BTreeMap::from([("id".to_string(), DataValue::from(item_id))]),
        )?;
        Ok(!rows.rows.is_empty())
    }
}

fn canonical_feed_url(url: &str) -> Result<String> {
    let parsed = url::Url::parse(url).map_err(|source| StoreError::InvalidUrl {
        url: url.to_string(),
        source,
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(StoreError::InvalidUrl {
                url: url.to_string(),
                source: url::ParseError::RelativeUrlWithoutBase,
            });
        }
    }
    Ok(canonicalize_external_identifier(url))
}

fn decode_feed_row(row: &[DataValue]) -> Result<FeedRecord> {
    Ok(FeedRecord {
        url: value_as_string(required(row, 0, "feed.url")?)?,
        title: optional_string(required(row, 1, "feed.title")?)?,
        description: optional_string(required(row, 2, "feed.description")?)?,
        last_fetched_micros: optional_validity_micros(required(row, 3, "feed.last_fetched")?)?,
        last_modified_header: optional_string(required(row, 4, "feed.last_modified_header")?)?,
        etag_header: optional_string(required(row, 5, "feed.etag_header")?)?,
        fetch_interval_seconds: value_as_i64(required(row, 6, "feed.fetch_interval_seconds")?)?,
        consecutive_failures: value_as_i64(required(row, 7, "feed.consecutive_failures")?)?,
        folder: optional_string(required(row, 8, "feed.folder")?)?,
        auto_mark_read: value_as_bool(required(row, 9, "feed.auto_mark_read")?)?,
    })
}

fn required<'a>(
    row: &'a [DataValue],
    index: usize,
    context: &'static str,
) -> Result<&'a DataValue> {
    row.get(index).ok_or_else(|| StoreError::UnexpectedValue {
        context,
        value: "missing column".to_string(),
    })
}

fn option_string(value: &Option<String>) -> DataValue {
    value
        .as_deref()
        .map(DataValue::from)
        .unwrap_or(DataValue::Null)
}

fn option_str(value: Option<&str>) -> DataValue {
    value.map(DataValue::from).unwrap_or(DataValue::Null)
}

fn option_validity(value: Option<i64>) -> DataValue {
    value
        .map(|micros| DataValue::Validity(Validity::from((micros, true))))
        .unwrap_or(DataValue::Null)
}

fn optional_string(value: &DataValue) -> Result<Option<String>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Str(value) => Ok(Some(value.to_string())),
        other => Err(unexpected("optional string", other)),
    }
}

fn optional_validity_micros(value: &DataValue) -> Result<Option<i64>> {
    match value {
        DataValue::Null => Ok(None),
        DataValue::Validity(value) => Ok(Some(value.timestamp.0.0)),
        other => Err(unexpected("optional validity", other)),
    }
}

fn value_as_string(value: &DataValue) -> Result<String> {
    match value {
        DataValue::Str(value) => Ok(value.to_string()),
        other => Err(unexpected("string", other)),
    }
}

fn value_as_i64(value: &DataValue) -> Result<i64> {
    match value {
        DataValue::Num(Num::Int(value)) => Ok(*value),
        other => Err(unexpected("integer", other)),
    }
}

fn value_as_bool(value: &DataValue) -> Result<bool> {
    match value {
        DataValue::Bool(value) => Ok(*value),
        other => Err(unexpected("bool", other)),
    }
}

fn unexpected(context: &'static str, value: &DataValue) -> StoreError {
    StoreError::UnexpectedValue {
        context,
        value: format!("{value:?}"),
    }
}

fn validity_now() -> DataValue {
    DataValue::Validity(Validity::from((now_micros(), true)))
}

fn now_micros() -> i64 {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    i64::try_from(nanos / 1_000).unwrap_or(i64::MAX)
}

fn failure_interval_seconds(feed: &FeedRecord, retry_after_seconds: Option<i64>) -> i64 {
    if let Some(retry_after) = retry_after_seconds {
        return retry_after
            .max(feed.fetch_interval_seconds)
            .min(MAX_FETCH_INTERVAL_SECONDS);
    }

    if feed.consecutive_failures < 10 {
        return feed.fetch_interval_seconds;
    }

    let doubled = feed.fetch_interval_seconds.saturating_mul(2);
    let jitter = deterministic_jitter_seconds(&feed.url, feed.consecutive_failures, doubled);
    doubled
        .saturating_add(jitter)
        .min(MAX_FETCH_INTERVAL_SECONDS)
}

fn deterministic_jitter_seconds(url: &str, failures: i64, interval: i64) -> i64 {
    let window = (interval / 10).max(1);
    let mut hasher = blake3::Hasher::new();
    hasher.update(url.as_bytes());
    hasher.update(&failures.to_be_bytes());
    let first = i64::from(hasher.finalize().as_bytes()[0]);
    first % window
}

#[cfg(test)]
mod tests {
    use carrel_core::feed::{ParsedEntry, ParsedFeed};

    use super::{FeedFetchMetadata, Store, canonicalize_external_identifier};

    #[test]
    fn feed_subscriptions_are_add_list_and_remove() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();

        let feed = store
            .add_feed("https://Example.com/feed.xml?utm_source=x")
            .unwrap();
        assert_eq!(feed.url, "https://example.com/feed.xml");

        let feeds = store.list_feeds().unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].url, "https://example.com/feed.xml");

        assert!(store.remove_feed("https://example.com/feed.xml").unwrap());
        assert!(store.list_feeds().unwrap().is_empty());
    }

    #[test]
    fn ingest_is_idempotent_on_url_and_feed_guid() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        store.add_feed("https://example.com/feed.xml").unwrap();

        let parsed = ParsedFeed {
            title: Some("Example".to_string()),
            description: Some("A feed".to_string()),
            language: Some("en".to_string()),
            entries: vec![ParsedEntry {
                feed_guid: "post-1".to_string(),
                title: Some("Post One".to_string()),
                authors: vec!["Ada".to_string()],
                url: Some("https://example.com/post-1?utm_campaign=nope".to_string()),
                published_at_micros: Some(1_700_000_000_000_000),
                language: None,
                summary_html: Some("Summary".to_string()),
                content_html: Some("<p>Full</p>".to_string()),
            }],
        };
        let metadata = FeedFetchMetadata {
            etag_header: Some("\"abc\"".to_string()),
            last_modified_header: Some("Sun, 03 May 2026 00:00:00 GMT".to_string()),
            retry_after_seconds: None,
        };

        let first = store
            .ingest_feed("https://example.com/feed.xml", &parsed, &metadata)
            .unwrap();
        let second = store
            .ingest_feed("https://example.com/feed.xml", &parsed, &metadata)
            .unwrap();

        assert_eq!(first.new_items, 1);
        assert_eq!(second.new_items, 0);
        assert_eq!(second.updated_items, 1);

        let canonical_url =
            canonicalize_external_identifier("https://example.com/post-1?utm_campaign=nope");
        let rows = store
            .query_with_params(
                "?[item_id] := *item_identifier{item_id, scheme: 'url', value: $value}",
                std::collections::BTreeMap::from([(
                    "value".to_string(),
                    cozo::DataValue::from(canonical_url.as_str()),
                )]),
            )
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
    }
}
