//! Feed management state shared by the app shell.

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

use leptos::prelude::*;

#[cfg(target_arch = "wasm32")]
use crate::api::{self, ApiError};
use crate::api::{FeedStatusSummary, FeedSummary};

/// Feed subscriptions and fetch status shared through Leptos context.
#[derive(Clone, Copy)]
pub(crate) struct FeedController {
    feeds: RwSignal<Vec<FeedSummary>>,
    status: RwSignal<FeedStatusSummary>,
    error: RwSignal<Option<String>>,
    pub(crate) add_open: RwSignal<bool>,
    pub(crate) file_busy: RwSignal<bool>,
    pub(crate) search_status: RwSignal<crate::api::SearchStatus>,
    pub(crate) sync_revision: RwSignal<u64>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    writes: StoredValue<crate::writes::PendingWrites>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    refresh_token: RwSignal<u64>,
}

impl FeedController {
    pub(crate) fn feeds(&self) -> RwSignal<Vec<FeedSummary>> {
        self.feeds
    }

    pub(crate) fn status(&self) -> RwSignal<FeedStatusSummary> {
        self.status
    }

    pub(crate) fn error(&self) -> RwSignal<Option<String>> {
        self.error
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn refresh_token(&self) -> RwSignal<u64> {
        self.refresh_token
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn bump_refresh(&self) {
        self.refresh_token
            .try_update(|revision| *revision = revision.saturating_add(1));
    }
}

/// Provide feed state for app children.
pub(crate) fn provide_feed_controller() -> FeedController {
    let controller = FeedController {
        feeds: RwSignal::new(Vec::new()),
        status: RwSignal::new(FeedStatusSummary::default()),
        error: RwSignal::new(None),
        add_open: RwSignal::new(false),
        file_busy: RwSignal::new(false),
        search_status: RwSignal::new(crate::api::SearchStatus::default()),
        sync_revision: RwSignal::new(0),
        writes: StoredValue::new(crate::writes::use_pending_writes()),
        refresh_token: RwSignal::new(0),
    };
    provide_context(controller);

    #[cfg(target_arch = "wasm32")]
    {
        reload(controller);
        poll_search_status(controller);
        let interval = set_interval_with_handle(
            move || {
                poll_status(controller);
                if controller.search_status.get_untracked() == crate::api::SearchStatus::Preparing {
                    poll_search_status(controller);
                }
            },
            Duration::from_secs(5),
        );
        if let Ok(interval) = interval {
            on_cleanup(move || interval.clear());
        }
    }

    controller
}

/// Read the feed controller.
pub(crate) fn use_feed_controller() -> FeedController {
    use_context::<FeedController>().expect("FeedController should be mounted")
}

/// Compose native file commands and refresh the subscription presentation.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn subscription_file(
    controller: FeedController,
    import: bool,
) -> Result<(), String> {
    let guard = controller.writes.get_value().begin();
    let result = if import {
        api::import_subscriptions().await
    } else {
        api::export_subscriptions().await
    };
    drop(guard);
    if result.map_err(message)? {
        refresh(controller, import).await;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn subscription_file(
    _controller: FeedController,
    _import: bool,
) -> Result<(), String> {
    Ok(())
}

/// Route path for a feed URL.
pub(crate) async fn refresh_from_sync(controller: FeedController) {
    #[cfg(target_arch = "wasm32")]
    refresh(controller, true).await;
    #[cfg(not(target_arch = "wasm32"))]
    let _ = controller;
}

/// Route path for a feed URL.
pub(crate) fn feed_href(url: &str) -> String {
    format!("/feeds/{}", percent_encode(url))
}

/// Decode a feed URL path segment.
pub(crate) fn decode_feed_path(value: &str) -> Option<String> {
    percent_decode(value)
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn add_and_fetch(controller: FeedController, url: String) -> Result<(), String> {
    let guard = controller.writes.get_value().begin();
    let feed = api::add_feed(url).await.map_err(message)?;
    publish_subscription(controller, feed.clone());
    drop(guard);
    wasm_bindgen_futures::spawn_local(async move {
        let result = api::fetch_feed(feed.url).await;
        refresh_after_mutation(controller).await.ok();
        if let Err(error) = result {
            controller.error.try_set(Some(format!(
                "Subscription saved. First fetch failed: {}",
                error.message()
            )));
        }
    });
    Ok(())
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn publish_subscription(controller: FeedController, feed: FeedSummary) {
    controller.feeds.try_update(|feeds| {
        if let Some(existing) = feeds.iter_mut().find(|existing| existing.url == feed.url) {
            *existing = feed;
        } else {
            feeds.push(feed);
        }
        // Match the store before closing Add, so the first fetch cannot move a row.
        feeds.sort_unstable_by(|left, right| left.url.cmp(&right.url));
    });
    controller.bump_refresh();
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn add_and_fetch(_controller: FeedController, _url: String) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn remove(controller: FeedController, url: String) -> Result<(), String> {
    let guard = controller.writes.get_value().begin();
    let _ = api::remove_feed(url).await.map_err(message)?;
    drop(guard);
    refresh_after_mutation(controller).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn remove(_controller: FeedController, _url: String) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn fetch_all(controller: FeedController) -> Result<(), String> {
    let _ = api::fetch_all_feeds().await.map_err(message)?;
    refresh_after_mutation(controller).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn fetch_all(_controller: FeedController) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn fetch_one(controller: FeedController, url: String) -> Result<(), String> {
    let _ = api::fetch_feed(url).await.map_err(message)?;
    refresh_after_mutation(controller).await
}

#[cfg(target_arch = "wasm32")]
fn reload(controller: FeedController) {
    use wasm_bindgen_futures::spawn_local;

    spawn_local(async move {
        refresh(controller, false).await;
    });
}

#[cfg(target_arch = "wasm32")]
fn poll_status(controller: FeedController) {
    use wasm_bindgen_futures::spawn_local;

    spawn_local(async move {
        let previous_revision = controller.status.get_untracked().revision;
        match api::feed_status().await {
            Ok(status) => {
                let changed = status.revision != previous_revision;
                let finished = !status.is_fetching;
                controller.status.set(status);
                if changed {
                    refresh(controller, finished).await;
                }
            }
            Err(error) => controller.error.set(Some(error.message().to_string())),
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn poll_search_status(controller: FeedController) {
    wasm_bindgen_futures::spawn_local(async move {
        let status = api::search_status()
            .await
            .unwrap_or(crate::api::SearchStatus::Failed);
        let previous = controller.search_status.get_untracked();
        controller.search_status.set(status);
        if previous == crate::api::SearchStatus::Preparing && status != previous {
            controller.bump_refresh();
        }
    });
}

/// Retry the local text rebuild and resume status checks.
pub(crate) fn retry_search(controller: FeedController) {
    #[cfg(target_arch = "wasm32")]
    {
        controller
            .search_status
            .set(crate::api::SearchStatus::Preparing);
        wasm_bindgen_futures::spawn_local(async move {
            if api::rebuild_search().await.is_err() {
                controller
                    .search_status
                    .set(crate::api::SearchStatus::Failed);
            } else {
                poll_search_status(controller);
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = controller;
}

#[cfg(target_arch = "wasm32")]
async fn refresh_after_mutation(controller: FeedController) -> Result<(), String> {
    refresh(controller, true).await;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn refresh(controller: FeedController, bump: bool) {
    match api::list_feeds().await {
        Ok(feeds) => {
            controller.feeds.set(feeds);
            controller.error.set(None);
        }
        Err(error) => controller.error.set(Some(error.message().to_string())),
    }

    match api::feed_status().await {
        Ok(status) => controller.status.set(status),
        Err(error) => controller.error.set(Some(error.message().to_string())),
    }

    if bump {
        controller.bump_refresh();
    }
}

#[cfg(target_arch = "wasm32")]
fn message(error: ApiError) -> String {
    error.message().to_string()
}

pub(crate) fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if is_unreserved(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_char(byte >> 4));
            encoded.push(hex_char(byte & 0x0f));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn hex_char(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'A' + value - 10),
        _ => unreachable!("nibble should fit in hex"),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_href_round_trips_feed_urls() {
        let url = "https://example.com/rss/feed.xml?x=1&name=Carrel";
        let href = feed_href(url);

        assert_eq!(
            href,
            "/feeds/https%3A%2F%2Fexample.com%2Frss%2Ffeed.xml%3Fx%3D1%26name%3DCarrel"
        );
        assert_eq!(
            decode_feed_path(href.trim_start_matches("/feeds/")),
            Some(url.to_string())
        );
    }

    #[test]
    fn decode_feed_path_rejects_bad_escapes() {
        assert_eq!(decode_feed_path("https%3A%2"), None);
        assert_eq!(decode_feed_path("https%3X"), None);
    }

    #[test]
    fn newly_added_subscriptions_keep_the_store_order_before_and_after_fetch() {
        Owner::new().with(|| {
            let controller = provide_feed_controller();
            let json = FeedSummary {
                url: "https://json.blog/feed.xml".into(),
                title: None,
                description: None,
                display_name: "json.blog".into(),
                last_fetched_label: "Never".into(),
                fetch_interval_seconds: 3600,
                consecutive_failures: 0,
                folder: None,
                auto_mark_read: false,
            };
            let mut elixir = FeedSummary {
                url: "https://elixir-lang.org/blog".into(),
                display_name: "elixir-lang.org".into(),
                ..json.clone()
            };
            publish_subscription(controller, json.clone());
            publish_subscription(controller, elixir.clone());
            let before_fetch = controller.feeds.get_untracked();
            // The native store returns subscriptions in canonical URL order.
            assert_eq!(before_fetch, vec![elixir.clone(), json.clone()]);
            controller.feeds.set(vec![elixir.clone(), json.clone()]);
            assert_eq!(controller.feeds.get_untracked(), before_fetch);

            elixir.title = Some("The Elixir Blog".into());
            elixir.display_name = "The Elixir Blog".into();
            elixir.consecutive_failures = 1;
            publish_subscription(controller, elixir.clone());
            assert_eq!(controller.feeds.get_untracked(), vec![elixir, json]);
        });
    }

    #[test]
    fn a_saved_subscription_is_visible_before_fetch_and_survives_fetch_failure() {
        Owner::new().with(|| {
            let controller = provide_feed_controller();
            let feed = FeedSummary {
                url: "https://example.com/feed".into(),
                title: None,
                description: None,
                display_name: "Example".into(),
                last_fetched_label: "Never".into(),
                fetch_interval_seconds: 3600,
                consecutive_failures: 0,
                folder: None,
                auto_mark_read: false,
            };
            publish_subscription(controller, feed.clone());
            assert_eq!(controller.feeds.get_untracked(), vec![feed.clone()]);
            assert_eq!(controller.refresh_token.get_untracked(), 1);
            controller.error.set(Some("First fetch failed".into()));
            assert_eq!(controller.feeds.get_untracked(), vec![feed.clone()]);
            publish_subscription(controller, feed.clone());
            assert_eq!(controller.feeds.get_untracked(), vec![feed]);
        });
    }
}
