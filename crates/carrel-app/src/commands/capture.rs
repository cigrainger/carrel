//! Capture an article URL without creating a feed subscription.

use carrel_core::article::ArchivedArticle;
use carrel_feeds::{DEFAULT_USER_AGENT, Fetcher, article::fetch_article, discovery::discovery_url};
use serde::Serialize;
use tauri::State;

use crate::{AppError, Result, state::AppState};

/// Saved article destination and any incomplete image-cache notice.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedArticle {
    /// Stable article ID for opening the existing reader.
    pub id: String,
    /// Display heading, matching Library and the reader.
    pub title: String,
    /// Whether this article was already present.
    pub existed: bool,
    /// Best-effort image failures, expressed without publisher content.
    pub warning: Option<String>,
}

/// Archive an explicit article address through the native ingestion boundary.
#[tauri::command]
pub async fn save_article(state: State<'_, AppState>, url: String) -> Result<SavedArticle> {
    let state = state.inner().clone();
    // Match the feed worker: the HTML parser holds Rc nodes across image fetches.
    // Its single-threaded pipeline lives wholly inside a blocking worker, away
    // from both the UI thread and Tauri's Send command future.
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(capture_error)?;
        runtime.block_on(async move {
            let fetcher = Fetcher::new(DEFAULT_USER_AGENT).map_err(capture_error)?;
            capture_article(&state, &fetcher, &url).await
        })
    })
    .await
    .map_err(capture_error)?
}

pub(crate) async fn capture_article(
    state: &AppState,
    fetcher: &Fetcher,
    input: &str,
) -> Result<SavedArticle> {
    let url = discovery_url(input).map_err(capture_error)?.to_string();
    // A local duplicate must not wait behind a running network feed batch.
    if let Some(article) = cached_article(state, &url)? {
        return Ok(article);
    }
    // Serialize with feed ingestion, and recheck after acquiring the lock so two
    // simultaneous captures do not refetch or overwrite a just-saved archive.
    let _guard = state.feed_fetch_lock.lock().await;
    if let Some(article) = cached_article(state, &url)? {
        return Ok(article);
    }
    let page = fetch_article(fetcher, &url).await.map_err(capture_error)?;
    let rewritten = carrel_feeds::rewrite_images(
        &page.article.content_html,
        &page.base_url,
        fetcher,
        |bytes| state.blobs.put_blocking(bytes).map(|id| id.to_string()),
    )
    .await;
    let warning = (!rewritten.failures.is_empty()).then(|| {
        "Article saved. Some images could not be archived and may need an internet connection."
            .to_string()
    });
    let blob_id = state
        .blobs
        .put(rewritten.html.as_bytes())
        .await?
        .to_string();
    let content = page.article;
    let archive = ArchivedArticle {
        url: page.canonical_url,
        aliases: vec![url, page.final_url],
        title: content.title,
        creators: content.byline.iter().cloned().collect(),
        published_at_micros: page.published_at_micros,
        language: content.language,
        summary: page.summary,
        site_name: content.site_name,
        byline: content.byline,
        blob_id,
        byte_size: i64::try_from(rewritten.html.len()).unwrap_or(i64::MAX),
        extracted_with: content.extractor.as_str().into(),
        word_count: i64::try_from(content.word_count).unwrap_or(i64::MAX),
        estimated_read_minutes: i64::from(content.estimated_read_minutes),
        searchable_text: carrel_feeds::html_to_text(&rewritten.html),
        shape: carrel_feeds::detect_shape(&rewritten.html, content.word_count),
    };
    let mut addresses = archive.aliases.clone();
    addresses.push(archive.url.clone());
    let missing_blob = if let Some(id) = state.store.article_by_urls(&addresses)? {
        state
            .store
            .get_item_detail(&id)?
            .and_then(|item| item.readable)
            .filter(|content| {
                content.blob_id.parse().ok().is_some_and(|id| {
                    matches!(
                        state.blobs.get_blocking(&id),
                        Err(carrel_store::blobs::BlobError::NotFound { .. })
                    )
                })
            })
            .map(|content| content.blob_id)
    } else {
        None
    };
    let saved = state.store.save_archived_article(
        &archive,
        crate::feed_runner::now_micros(),
        missing_blob.as_deref(),
    )?;
    let title = state
        .store
        .get_item_detail(&saved.id)?
        .map_or(saved.title, |item| {
            super::items::item_display_text(
                &item.title,
                item.primary_url.as_deref(),
                item.summary.as_deref(),
            )
            .0
        });
    Ok(SavedArticle {
        id: saved.id,
        title,
        existed: saved.existed,
        warning: warning.filter(|_| saved.archived),
    })
}

fn cached_article(state: &AppState, url: &str) -> Result<Option<SavedArticle>> {
    if let Some(id) = state.store.article_by_urls(&[url.into()])?
        && let Some(item) = state.store.get_item_detail(&id)?
        && let Some(content) = &item.readable
        && state
            .blobs
            .get_blocking(&content.blob_id.parse().map_err(capture_error)?)
            .is_ok()
    {
        Ok(Some(SavedArticle {
            id,
            title: super::items::item_display_text(
                &item.title,
                item.primary_url.as_deref(),
                item.summary.as_deref(),
            )
            .0,
            existed: true,
            warning: None,
        }))
    } else {
        Ok(None)
    }
}

fn capture_error(error: impl std::fmt::Display) -> AppError {
    AppError::Capture(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use carrel_core::library::ItemFilter;
    use carrel_feeds::FetcherConfig;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn state() -> (AppState, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let paths = crate::config::InstallPaths {
            blobs: root.join("blobs"),
            keys: root.join("keys"),
            keymap_config: root.join("keymap.toml"),
            store: root.join("store"),
            root,
        };
        let store = carrel_store::Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        (
            AppState::new(
                store,
                carrel_store::blobs::BlobStore::open(&paths.blobs),
                paths,
            ),
            directory,
        )
    }
    fn fetcher() -> Fetcher {
        Fetcher::with_config(
            DEFAULT_USER_AGENT,
            FetcherConfig {
                min_interval_per_host: Duration::ZERO,
                request_timeout: Duration::from_secs(2),
                ..Default::default()
            },
        )
        .unwrap()
    }
    async fn robots(server: &MockServer, body: &str) {
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(server)
            .await;
    }
    fn page(head: &str, image: &str) -> String {
        format!("<html><head><title>A thoughtful article</title>{head}</head><body><article><h1>A thoughtful article</h1><p>{}</p>{image}</article></body></html>", "Reading architectural counterforms carefully helps us understand useful tools and their construction. ".repeat(30))
    }

    #[tokio::test]
    async fn redirect_capture_archives_images_and_repeated_input_works_offline() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nAllow: /").await;
        Mock::given(path("/old"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/blog/page"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/blog/page")).respond_with(ResponseTemplate::new(200).set_body_string(page("<link rel='canonical' href='/article'><meta property='article:published_time' content='2026-09-09T08:48:00Z'>", "<img src='diagram.png' alt='Diagram' width='200' height='100'>"))).expect(1).mount(&server).await;
        Mock::given(path("/blog/diagram.png"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(b"\x89PNG\r\n\x1a\nimage".to_vec()),
            )
            .mount(&server)
            .await;
        let (state, _directory) = state();
        let saved = capture_article(
            &state,
            &fetcher(),
            &format!("{}/old?utm_source=test#section", server.uri()),
        )
        .await
        .unwrap();
        assert!(!saved.existed);
        assert!(saved.warning.is_none());
        let item = state.store.get_item_detail(&saved.id).unwrap().unwrap();
        assert_eq!(
            item.primary_url.as_deref(),
            Some(format!("{}/article", server.uri()).as_str())
        );
        let html = state
            .blobs
            .get(&item.readable.unwrap().blob_id.parse().unwrap())
            .await
            .unwrap();
        let html = String::from_utf8(html.to_vec()).unwrap();
        assert!(html.contains("blob://"));
        assert!(!html.contains("src=\"diagram.png\""));
        assert!(!html.contains("<script"));
        let mut search = ItemFilter::library();
        search.search = Some("architectural counter".into());
        assert_eq!(
            state.store.list_item_page(&search, 0).unwrap().items.len(),
            1
        );
        assert!(state.store.list_feeds().unwrap().is_empty());
        // Clear every HTTP fixture; repeated saves use the existing local cache.
        server.reset().await;
        let _feed_batch = state.feed_fetch_lock.lock().await;
        let again = tokio::time::timeout(
            Duration::from_secs(1),
            capture_article(
                &state,
                &fetcher(),
                &format!("{}/old?utm_source=another", server.uri()),
            ),
        )
        .await
        .expect("local capture must not wait for network feed ingestion")
        .unwrap();
        assert!(again.existed);
        assert_eq!(again.id, saved.id);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn untitled_capture_uses_the_same_opening_heading_in_results_library_and_reader() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nAllow: /").await;
        let html = page("", "")
            .replace("<title>A thoughtful article</title>", "")
            .replace("<h1>A thoughtful article</h1>", "");
        Mock::given(path("/untitled"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html))
            .mount(&server)
            .await;
        let (state, _directory) = state();
        let url = format!("{}/untitled", server.uri());
        let saved = capture_article(&state, &fetcher(), &url).await.unwrap();
        assert!(
            saved
                .title
                .starts_with("Reading architectural counterforms"),
            "{}",
            saved.title
        );
        let reader =
            super::super::items::get_item_from_parts(&state.store, &state.blobs, &saved.id)
                .unwrap()
                .unwrap();
        let library =
            super::super::items::list_item_page_from_store(&state.store, ItemFilter::library())
                .unwrap();
        assert_eq!(saved.title, reader.title);
        assert_eq!(saved.title, library.items[0].title);
        server.reset().await;
        let again = capture_article(&state, &fetcher(), &url).await.unwrap();
        assert_eq!(saved.title, again.title);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_image_is_explicit_but_readable_text_remains_available() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nAllow: /").await;
        Mock::given(path("/article"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page(
                "",
                "<img src='/missing.png' alt='Publisher illustration' width='200' height='100'>",
            )))
            .mount(&server)
            .await;
        let (state, _directory) = state();
        let saved = capture_article(&state, &fetcher(), &format!("{}/article", server.uri()))
            .await
            .unwrap();
        assert!(saved.warning.unwrap().contains("Some images"));
        assert!(
            state
                .store
                .get_item_detail(&saved.id)
                .unwrap()
                .unwrap()
                .readable
                .is_some()
        );
    }

    #[tokio::test]
    async fn failures_leave_no_partial_item_and_can_be_retried() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nAllow: /").await;
        let (state, _directory) = state();
        let url = format!("{}/article", server.uri());
        assert!(capture_article(&state, &fetcher(), &url).await.is_err());
        assert!(
            state
                .store
                .list_item_page(&ItemFilter::library(), 0)
                .unwrap()
                .items
                .is_empty()
        );
        Mock::given(path("/article"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page("", "")))
            .mount(&server)
            .await;
        assert!(capture_article(&state, &fetcher(), &url).await.is_ok());
        assert_eq!(
            state
                .store
                .list_item_page(&ItemFilter::library(), 0)
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn robots_denial_and_invalid_addresses_do_not_fetch_or_save_an_article() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nDisallow: /").await;
        let (state, _directory) = state();
        assert!(
            capture_article(&state, &fetcher(), &format!("{}/article", server.uri()))
                .await
                .is_err()
        );
        for url in [
            "",
            "file:///tmp/secret",
            "javascript:alert(1)",
            "https://user:password@example.org/article",
        ] {
            assert!(capture_article(&state, &fetcher(), url).await.is_err());
        }
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.url.path() == "/robots.txt")
        );
        assert!(
            state
                .store
                .list_item_page(&ItemFilter::library(), 0)
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_lost_readable_blob_can_be_recaptured_without_changing_identity() {
        let server = MockServer::start().await;
        robots(&server, "User-agent: *\nAllow: /").await;
        let (state, _directory) = state();
        let url = format!("{}/article", server.uri());
        let archive = carrel_core::article::ArchivedArticle {
            url: url.clone(),
            aliases: vec![],
            title: Some("Retained title".into()),
            creators: vec![],
            published_at_micros: None,
            language: None,
            summary: None,
            site_name: None,
            byline: None,
            blob_id: "a".repeat(64),
            byte_size: 3,
            extracted_with: "manual".into(),
            word_count: 1,
            estimated_read_minutes: 1,
            searchable_text: "Lost content".into(),
            shape: Default::default(),
        };
        let first = state
            .store
            .save_archived_article(&archive, 10, None)
            .unwrap();
        Mock::given(path("/article"))
            .respond_with(ResponseTemplate::new(200).set_body_string(page("", "")))
            .mount(&server)
            .await;
        let saved = capture_article(&state, &fetcher(), &url).await.unwrap();
        assert_eq!(saved.id, first.id);
        assert_eq!(saved.title, "Retained title");
        let item = state.store.get_item_detail(&saved.id).unwrap().unwrap();
        assert!(
            state
                .blobs
                .get(&item.readable.unwrap().blob_id.parse().unwrap())
                .await
                .is_ok()
        );
    }
}
