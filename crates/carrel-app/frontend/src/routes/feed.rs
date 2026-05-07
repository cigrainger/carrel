//! Feed route shell.

use std::future::Future;

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::api::FeedSummary;
use crate::feeds::{self, use_feed_controller};

/// Route for a single subscribed feed.
#[component]
pub fn FeedRoute() -> impl IntoView {
    let params = use_params_map();
    let controller = use_feed_controller();
    let feeds = controller.feeds();
    let status = controller.status();
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
    let fetch_busy = RwSignal::new(false);

    view! {
        <section class="route-panel feed-route">
            {move || match selected_feed.get() {
                Some(feed) => {
                    let feed_for_fetch = feed.clone();
                    view! {
                        <header class="route-header feed-route-header">
                            <div>
                                <h1>{feed.display_name}</h1>
                                <p>{feed.url.clone()}</p>
                            </div>
                            <button
                                type="button"
                                disabled=move || status.get().is_fetching || fetch_busy.get()
                                on:click=move |_| {
                                    fetch_busy.set(true);
                                    let controller = controller;
                                    let url = feed_for_fetch.url.clone();
                                    spawn_feed_task(async move {
                                        if let Err(message) = feeds::fetch_one(controller, url).await {
                                            controller.error().set(Some(message));
                                        }
                                        fetch_busy.set(false);
                                    });
                                }
                            >
                                "Fetch"
                            </button>
                        </header>
                        {feed.description.map(|description| {
                            view! { <p class="quiet-state">{description}</p> }
                        })}
                        <dl class="feed-details">
                            <div>
                                <dt>"Last fetched"</dt>
                                <dd>{feed.last_fetched_label}</dd>
                            </div>
                            <div>
                                <dt>"Fetch interval"</dt>
                                <dd>{format!("{}s", feed.fetch_interval_seconds)}</dd>
                            </div>
                            <div>
                                <dt>"Failures"</dt>
                                <dd>{feed.consecutive_failures}</dd>
                            </div>
                        </dl>
                    }.into_any()
                }
                None => view! {
                    <h1>"Feed"</h1>
                    <p class="quiet-state">"Feed not found."</p>
                }.into_any(),
            }}
        </section>
    }
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

#[cfg(target_arch = "wasm32")]
fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_feed_task(_task: impl Future<Output = ()> + 'static) {}
