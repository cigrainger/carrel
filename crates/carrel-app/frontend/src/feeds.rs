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
    fn bump_refresh(&self) {
        self.refresh_token
            .update(|revision| *revision = revision.saturating_add(1));
    }
}

/// Provide feed state for app children.
pub(crate) fn provide_feed_controller() -> FeedController {
    let controller = FeedController {
        feeds: RwSignal::new(Vec::new()),
        status: RwSignal::new(FeedStatusSummary::default()),
        error: RwSignal::new(None),
        refresh_token: RwSignal::new(0),
    };
    provide_context(controller);

    #[cfg(target_arch = "wasm32")]
    {
        reload(controller);
        let interval = set_interval_with_handle(
            move || {
                poll_status(controller);
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
    let feed = api::add_feed(url).await.map_err(message)?;
    let _ = api::fetch_feed(feed.url).await.map_err(message)?;
    refresh_after_mutation(controller).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn add_and_fetch(_controller: FeedController, _url: String) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn remove(controller: FeedController, url: String) -> Result<(), String> {
    let _ = api::remove_feed(url).await.map_err(message)?;
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
                controller.error.set(None);
                if changed {
                    refresh(controller, finished).await;
                }
            }
            Err(error) => controller.error.set(Some(error.message().to_string())),
        }
    });
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

fn percent_encode(value: &str) -> String {
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
}
