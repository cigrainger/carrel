//! Primary desktop navigation.

use std::future::Future;

use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::api::FeedSummary;
use crate::feeds::{self, use_feed_controller};

/// Sidebar navigation root.
#[component]
pub fn Sidebar() -> impl IntoView {
    let feed_controller = use_feed_controller();
    let feeds = feed_controller.feeds();
    let status = feed_controller.status();
    let controller_error = feed_controller.error();
    let add_open = RwSignal::new(false);
    let add_busy = RwSignal::new(false);
    let add_error = RwSignal::new(None::<String>);
    let add_input = NodeRef::<html::Input>::new();
    let remove_target = RwSignal::new(None::<FeedSummary>);
    let remove_busy = RwSignal::new(false);
    let fetch_busy = RwSignal::new(false);

    view! {
        <aside class="sidebar" aria-label="Primary">
            <div class="sidebar-brand">"Carrel"</div>
            <nav class="sidebar-nav">
                <A href="/today">"Today"</A>
                <A href="/friends">"Friends"</A>
                <A href="/library">"Library"</A>
                <A href="/highlights">"Highlights"</A>
                <A href="/lists">"Lists"</A>
            </nav>
            <div class="sidebar-section">
                <div class="sidebar-section-header">
                    <h2>"Feeds"</h2>
                    <div class="sidebar-section-actions">
                        <button
                            type="button"
                            aria-label="Fetch feeds"
                            disabled=move || status.get().is_fetching || fetch_busy.get()
                            on:click=move |_| {
                                fetch_busy.set(true);
                                let controller = feed_controller;
                                spawn_feed_task(async move {
                                    if let Err(message) = feeds::fetch_all(controller).await {
                                        controller.error().set(Some(message));
                                    }
                                    fetch_busy.set(false);
                                });
                            }
                        >
                            "Fetch"
                        </button>
                        <button
                            type="button"
                            aria-label="Add feed"
                            on:click=move |_| {
                                add_error.set(None);
                                add_open.set(true);
                            }
                        >
                            "+"
                        </button>
                    </div>
                </div>
                {move || {
                    controller_error
                        .get()
                        .map(|message| view! { <p class="feed-error">{message}</p> }.into_any())
                }}
                <Show
                    when=move || !feeds.get().is_empty()
                    fallback=move || view! { <p>"No feeds yet"</p> }
                >
                    <ul class="feed-list">
                        <For
                            each=move || feeds.get()
                            key=|feed| feed.url.clone()
                            let:feed
                        >
                            {
                                let remove_feed = feed.clone();
                                view! {
                                    <li class="feed-nav-row">
                                        <A href=feeds::feed_href(&feed.url)>{feed.display_name}</A>
                                        <button
                                            type="button"
                                            aria-label=format!("Remove {}", remove_feed.display_name)
                                            title="Remove feed"
                                            on:click=move |_| remove_target.set(Some(remove_feed.clone()))
                                        >
                                            "x"
                                        </button>
                                    </li>
                                }
                            }
                        </For>
                    </ul>
                </Show>
            </div>
            <Show when=move || add_open.get()>
                <div class="feed-dialog-backdrop" role="presentation">
                    <form
                        class="feed-dialog"
                        aria-label="Add feed"
                        on:submit=move |event| {
                            event.prevent_default();
                            let url = add_input
                                .get_untracked()
                                .map(|input| input.value())
                                .unwrap_or_default()
                                .trim()
                                .to_string();
                            if url.is_empty() {
                                add_error.set(Some("Enter a feed URL".to_string()));
                                return;
                            }

                            add_busy.set(true);
                            add_error.set(None);
                            let controller = feed_controller;
                            spawn_feed_task(async move {
                                match feeds::add_and_fetch(controller, url).await {
                                    Ok(()) => {
                                        if let Some(input) = add_input.get_untracked() {
                                            input.set_value("");
                                        }
                                        add_open.set(false);
                                    }
                                    Err(message) => add_error.set(Some(message)),
                                }
                                add_busy.set(false);
                            });
                        }
                    >
                        <h3>"Add Feed"</h3>
                        <input
                            node_ref=add_input
                            type="url"
                            name="feed-url"
                            placeholder="https://example.com/feed.xml"
                            autocomplete="url"
                            autofocus=true
                        />
                        {move || add_error.get().map(|message| {
                            view! { <p class="feed-error">{message}</p> }
                        })}
                        <div class="feed-dialog-actions">
                            <button
                                type="button"
                                on:click=move |_| {
                                    add_error.set(None);
                                    add_open.set(false);
                                }
                            >
                                "Cancel"
                            </button>
                            <button type="submit" disabled=move || add_busy.get()>
                                "Add"
                            </button>
                        </div>
                    </form>
                </div>
            </Show>
            <Show when=move || remove_target.get().is_some()>
                {move || remove_target.get().map(|feed| {
                    view! {
                        <div class="feed-dialog-backdrop" role="presentation">
                            <div class="feed-dialog" role="dialog" aria-label="Remove feed">
                                <h3>"Remove Feed"</h3>
                                <p>{format!("Stop fetching {}. Saved items stay in Carrel.", feed.display_name)}</p>
                                <div class="feed-dialog-actions">
                                    <button
                                        type="button"
                                        on:click=move |_| remove_target.set(None)
                                    >
                                        "Cancel"
                                    </button>
                                    <button
                                        type="button"
                                        disabled=move || remove_busy.get()
                                        on:click=move |_| {
                                            remove_busy.set(true);
                                            let controller = feed_controller;
                                            let url = feed.url.clone();
                                            spawn_feed_task(async move {
                                                if let Err(message) = feeds::remove(controller, url).await {
                                                    controller.error().set(Some(message));
                                                } else {
                                                    remove_target.set(None);
                                                }
                                                remove_busy.set(false);
                                            });
                                        }
                                    >
                                        "Remove"
                                    </button>
                                </div>
                            </div>
                        </div>
                    }
                })}
            </Show>
        </aside>
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_feed_task(_task: impl Future<Output = ()> + 'static) {}
