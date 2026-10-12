//! Feed-scoped item-list route.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use super::today::{ListSpec, list_route_view};
use crate::api::{FeedSummary, ItemFilter};
use crate::feeds::{self, use_feed_controller};

/// Route for a single subscribed feed.
#[component]
pub fn FeedRoute() -> impl IntoView {
    let params = use_params_map();
    let controller = use_feed_controller();
    let feeds = controller.feeds();
    let route_url = Memo::new(move |_| {
        params
            .read()
            .get("url")
            .and_then(|value| feeds::decode_feed_path(&value))
            .unwrap_or_default()
    });
    let selected_feed = Memo::new(move |_| {
        let url = route_url.get();
        feeds
            .get()
            .into_iter()
            .find(|feed| feed.url == url)
            .or_else(|| fallback_feed(url))
    });

    list_route_view(move || match selected_feed.get() {
        Some(feed) => ListSpec {
            title: feed.display_name,
            subtitle: Some(feed.url.clone()),
            filter: ItemFilter::feed(feed.url.clone()),
            return_path: feeds::feed_href(&feed.url),
            empty_message: "No saved items from this feed.".to_string(),
            fetch_url: Some(feed.url),
        },
        None => ListSpec {
            title: "Feed".to_string(),
            subtitle: None,
            filter: ItemFilter::feed(String::new()),
            return_path: "/library".to_string(),
            empty_message: "Feed not found.".to_string(),
            fetch_url: None,
        },
    })
}

fn fallback_feed(url: String) -> Option<FeedSummary> {
    if url.is_empty() {
        None
    } else {
        Some(FeedSummary {
            display_name: url.clone(),
            url,
            title: None,
            description: None,
            last_fetched_label: "unknown".to_string(),
            fetch_interval_seconds: 0,
            consecutive_failures: 0,
            folder: None,
            auto_mark_read: false,
        })
    }
}

/// Combined chronological archive for subscriptions in one folder.
#[component]
pub fn FolderRoute() -> impl IntoView {
    let params = use_params_map();
    let folder = Memo::new(move |_| {
        params
            .read()
            .get("folder")
            .and_then(|value| feeds::decode_feed_path(&value))
            .unwrap_or_default()
    });
    list_route_view(move || {
        let folder = folder.get();
        ListSpec {
            title: folder.clone(),
            subtitle: None,
            filter: ItemFilter::folder(folder.clone()),
            return_path: feeds::folder_href(&folder),
            empty_message: "No saved articles from sources in this folder.".into(),
            fetch_url: None,
        }
    })
}
