//! Background workers owned by the desktop shell.

use std::{str::FromStr, sync::atomic::Ordering, time::Duration};

use tracing::warn;

use crate::commands::feeds;
use crate::state::SearchStatus;
use crate::state::{AppState, FeedStatusCounts};

const FEED_LOOP_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the conservative feed fetch loop.
pub(crate) fn spawn_feed_loop(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = feeds::fetch_due_feeds(state.clone()).await {
                warn!(%error, "background feed fetch failed");
                state.feed_status.set_idle(
                    format!("Background feed fetch failed: {error}"),
                    FeedStatusCounts {
                        feed_errors: 1,
                        ..FeedStatusCounts::default()
                    },
                );
            }

            tokio::time::sleep(FEED_LOOP_INTERVAL).await;
        }
    });
}

/// Rebuild older cached text without blocking the UI or the feed worker.
pub(crate) fn spawn_search_rebuild(state: AppState) {
    if state.search_rebuilding.swap(true, Ordering::AcqRel) {
        return;
    }
    *state.search_status.lock().expect("search status lock") = SearchStatus::Preparing;
    tauri::async_runtime::spawn(async move {
        let worker = state.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            rebuild_search_text(&worker.store, &worker.blobs)
        })
        .await;
        let status = match result {
            Ok(Ok(status)) => status,
            _ => {
                warn!("cached-text search rebuild failed");
                SearchStatus::Failed
            }
        };
        *state.search_status.lock().expect("search status lock") = status;
        state.search_rebuilding.store(false, Ordering::Release);
    });
}

fn rebuild_search_text(
    store: &carrel_store::Store,
    blobs: &carrel_store::blobs::BlobStore,
) -> crate::Result<SearchStatus> {
    let mut status = SearchStatus::Ready;
    // Take one snapshot: unavailable blobs are retried on request or next launch,
    // rather than repeatedly retrying the same missing file in a busy loop.
    for source in store.pending_search_content()? {
        let html = carrel_store::blobs::BlobId::from_str(&source.blob_id)
            .ok()
            .and_then(|id| {
                blobs
                    .get_blocking(&id)
                    .ok()
                    .filter(|bytes| carrel_store::blobs::BlobId::from_bytes(bytes) == id)
            })
            .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok());
        let Some(html) = html else {
            warn!(item_id = %source.item_id, "cached text unavailable for search");
            status = SearchStatus::Partial;
            continue;
        };
        let text = carrel_feeds::html_to_text(&html);
        // The store checks the current blob in the same transaction as the write.
        // A newer feed extraction can safely win while this task is running.
        store.put_search_text(&source.item_id, &source.blob_id, &text)?;
    }
    Ok(status)
}

#[cfg(test)]
mod search_tests {
    use std::collections::BTreeMap;

    use carrel_core::library::ItemFilter;
    use carrel_store::{Store, blobs::BlobStore};
    use cozo::DataValue;

    use super::*;

    fn reference(store: &Store, id: &str, blob: &str) {
        store.query_with_params(r#"
            { ?[id, kind, title, creators, primary_url, published_at, language, summary, discovered_at] <-
                [[$id, 'article', 'Older title', [], null, null, null, null, validity(1)]]
              :put item {id => kind, title, creators, primary_url, published_at, language, summary, discovered_at} }
            { ?[item_id, format, blob_id, fetched_at, extracted_with, byte_size] <-
                [[$id, 'html_readable', $blob, validity(1), 'test', 5]]
              :put item_content {item_id, format => blob_id, fetched_at, extracted_with, byte_size} }
        "#, BTreeMap::from([("id".into(), DataValue::from(id)), ("blob".into(), DataValue::from(blob))])).unwrap();
    }

    #[test]
    fn older_blobs_rebuild_locally_and_missing_content_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let store_path = dir.path().join("store");
        let blobs = BlobStore::open(dir.path().join("blobs"));
        let missing_html = b"<p>Recovered offline text</p>";
        {
            let store = Store::open(&store_path).unwrap();
            store.migrate().unwrap();
            let id = blobs
                .put_blocking(b"<p>First paragraph</p><p>CAF&Eacute; body</p>")
                .unwrap();
            reference(&store, "cached", &id.to_string());
            reference(
                &store,
                "missing",
                &carrel_store::blobs::BlobId::from_bytes(missing_html).to_string(),
            );
            assert_eq!(
                rebuild_search_text(&store, &blobs).unwrap(),
                SearchStatus::Partial
            );
            let filter = ItemFilter {
                search: Some("paragraph café".into()),
                ..ItemFilter::library()
            };
            assert_eq!(
                store.list_item_page(&filter, 0).unwrap().items[0].id,
                "cached"
            );
            assert_eq!(store.pending_search_content().unwrap().len(), 1);
        }
        let store = Store::open(&store_path).unwrap();
        store.migrate().unwrap();
        blobs.put_blocking(missing_html).unwrap();
        assert_eq!(
            rebuild_search_text(&store, &blobs).unwrap(),
            SearchStatus::Ready
        );
        assert!(store.pending_search_content().unwrap().is_empty());
        assert_eq!(
            store
                .list_item_page(
                    &ItemFilter {
                        search: Some("recovered".into()),
                        ..ItemFilter::library()
                    },
                    0
                )
                .unwrap()
                .items[0]
                .id,
            "missing"
        );
        assert_eq!(
            rebuild_search_text(&store, &blobs).unwrap(),
            SearchStatus::Ready
        );
    }

    #[test]
    fn malformed_and_non_utf8_blobs_do_not_block_title_search() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let blobs = BlobStore::open(dir.path());
        reference(&store, "invalid", "not a hash");
        let binary = blobs.put_blocking(&[0xff, 0xfe]).unwrap();
        reference(&store, "binary", &binary.to_string());
        assert_eq!(
            rebuild_search_text(&store, &blobs).unwrap(),
            SearchStatus::Partial
        );
        assert_eq!(
            store
                .list_item_page(
                    &ItemFilter {
                        search: Some("older".into()),
                        ..ItemFilter::library()
                    },
                    0
                )
                .unwrap()
                .items
                .len(),
            2
        );
    }
}
