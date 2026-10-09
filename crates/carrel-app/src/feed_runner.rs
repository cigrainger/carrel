//! App-side feed fetch orchestration.

use carrel_core::feed::{ExtractedEntryContent, ParsedEntry, ParsedFeed};
use carrel_feeds::{
    FetchResult, Fetcher, HttpHeaders, detect_shape, extract_embedded_html, extract_from_url,
    parse_feed, rewrite_images,
};
use carrel_store::blobs::BlobStore;
use carrel_store::feeds::{EntryError, FeedFetchMetadata, FeedRecord, IngestStats};
use time::OffsetDateTime;

/// Fetch and ingest one subscribed feed.
pub(crate) async fn fetch_one(
    store: &carrel_store::Store,
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

/// Report for one feed fetch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FetchReport {
    pub(crate) url: String,
    pub(crate) outcome: FetchOutcome,
    pub(crate) content_errors: Vec<EntryError>,
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

/// Outcome for one feed fetch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FetchOutcome {
    /// Feed was updated and ingested.
    Updated {
        /// Store ingest stats.
        stats: IngestStats,
    },
    /// Conditional request returned 304.
    NotModified,
    /// The remote returned a non-success HTTP status.
    HttpStatus {
        /// HTTP status code.
        status: u16,
    },
    /// Fetch, parse, or store failure.
    Error {
        /// Developer-facing message.
        message: String,
    },
}

pub(crate) fn now_micros() -> i64 {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    i64::try_from(nanos / 1_000).unwrap_or(i64::MAX)
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
            searchable_text: carrel_feeds::html_to_text(&article.content_html),
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use carrel_feeds::{DEFAULT_USER_AGENT, FetcherConfig};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn fetch_one_ingests_feed_and_extracted_content() {
        let server = MockServer::start().await;
        allow_robots(&server).await;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"app-feed\"")
                    .set_body_string(format!(
                        r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
  <channel>
    <title>App Feed</title>
    <description>Fixture app feed</description>
    <item>
      <guid>app-1</guid>
      <title>App Item</title>
      <link>{}/article</link>
      <description>Summary</description>
    </item>
  </channel>
</rss>"#,
                        server.uri()
                    )),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/article"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<!doctype html>
<html>
  <body>
    <article>
      <h1>App Article</h1>
      <p>This readable article body is stored through the app runner.</p>
    </article>
  </body>
</html>"#,
            ))
            .mount(&server)
            .await;

        let tempdir = tempfile::tempdir().unwrap();
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let feed = store
            .add_feed(&format!("{}/feed.xml", server.uri()))
            .unwrap();
        let fetcher = test_fetcher();

        let report = fetch_one(&store, &blobs, &fetcher, &feed).await;

        match report.outcome {
            FetchOutcome::Updated { stats } => {
                assert_eq!(stats.new_items, 1);
                assert_eq!(stats.updated_items, 0);
                assert!(stats.errors.is_empty());
            }
            other => panic!("expected updated report, got {other:?}"),
        }
        assert!(report.content_errors.is_empty());

        let rows = store
            .query(
                r#"
                ?[id] := *item{id}
                :limit 1
                "#,
            )
            .unwrap();
        let cozo::DataValue::Str(item_id) = &rows.rows[0][0] else {
            panic!("expected stored item id");
        };
        let detail = store.get_item_detail(item_id).unwrap().unwrap();
        let readable = detail.readable.unwrap();
        let blob_id = readable.blob_id.parse().unwrap();
        let html = String::from_utf8(blobs.get(&blob_id).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("readable article body"));
        assert!(store.pending_search_content().unwrap().is_empty());
        let matches = store
            .list_item_page(
                &carrel_core::library::ItemFilter {
                    search: Some("readable body".into()),
                    ..carrel_core::library::ItemFilter::library()
                },
                0,
            )
            .unwrap();
        assert_eq!(matches.items[0].id, item_id.as_str());
    }

    #[tokio::test]
    async fn fetch_one_records_not_modified() {
        let server = MockServer::start().await;
        allow_robots(&server).await;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(ResponseTemplate::new(304).insert_header("etag", "\"fresh\""))
            .mount(&server)
            .await;

        let tempdir = tempfile::tempdir().unwrap();
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let blobs = BlobStore::open(tempdir.path());
        let feed = store
            .add_feed(&format!("{}/feed.xml", server.uri()))
            .unwrap();
        let fetcher = test_fetcher();

        let report = fetch_one(&store, &blobs, &fetcher, &feed).await;

        assert_eq!(report.outcome, FetchOutcome::NotModified);
        let feed = store.get_feed(&feed.url).unwrap().unwrap();
        assert_eq!(feed.etag_header.as_deref(), Some("\"fresh\""));
        assert_eq!(feed.consecutive_failures, 0);
    }

    async fn allow_robots(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
            .mount(server)
            .await;
    }

    fn test_fetcher() -> Fetcher {
        Fetcher::with_config(
            DEFAULT_USER_AGENT,
            FetcherConfig {
                max_concurrent_per_host: 2,
                min_interval_per_host: Duration::from_millis(0),
                robots_cache_ttl: Duration::from_secs(60),
                request_timeout: Duration::from_secs(5),
            },
        )
        .unwrap()
    }
}
