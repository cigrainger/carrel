//! Primary desktop navigation.

use std::future::Future;

use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::api::FeedSummary;
use crate::components::Modal;
use crate::feeds::{self, use_feed_controller};

/// Sidebar navigation root.
#[component]
pub fn Sidebar() -> impl IntoView {
    let feed_controller = use_feed_controller();
    let feeds = feed_controller.feeds();
    let status = feed_controller.status();
    let controller_error = feed_controller.error();
    let add_open = feed_controller.add_open;
    let location = use_location();
    let add_busy = RwSignal::new(false);
    let add_error = RwSignal::new(None::<String>);
    let add_input = NodeRef::<html::Input>::new();
    let remove_target = RwSignal::new(None::<FeedSummary>);
    let remove_busy = RwSignal::new(false);
    let fetch_busy = RwSignal::new(false);

    view! {
        <aside class="sidebar" id="reading-sidebar" aria-label="Primary">
            <div class="sidebar-brand">"carrel"<span>"."</span></div>
            <nav class="sidebar-nav">
                <A href="/today" attr:class=move || if location.pathname.get() == "/" { "is-active" } else { "" }>"Today"</A>
                <A href="/reading">"Reading"</A>
                <A href="/library" attr:class=move || if matches!(location.pathname.get().as_str(), "/unread" | "/starred") { "is-active" } else { "" }>"Library"</A>
                <A href="/highlights">"Highlights"</A>
            </nav>
            <div class="sidebar-section">
                <div class="sidebar-section-header">
                    <h2>"Subscriptions"</h2>
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
                            each={move || feeds.get().into_iter().map(|feed| feed.url).collect::<Vec<_>>()}
                            key=|url| url.clone()
                            let:url
                        >
                            {
                                let href = feeds::feed_href(&url);
                                let current = Memo::new(move |_| feeds.with(|feeds| feeds.iter().find(|feed| feed.url == url).cloned()));
                                view! {
                                    <li class="feed-nav-row">
                                        <A href=href>{move || current.with(|feed| feed.as_ref().map(|feed| feed.display_name.clone()).unwrap_or_default())}</A>
                                        <button
                                            type="button"
                                            aria-label=move || current.with(|feed| format!("Remove {}", feed.as_ref().map(|feed| feed.display_name.as_str()).unwrap_or_default()))
                                            title="Remove feed"
                                            on:click=move |_| remove_target.set(current.get_untracked())
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
            <SubscriptionFiles/>
            <crate::components::devices::Devices/>
            <Show when=move || add_open.get()>
                <Modal label="Add feed" class="feed-dialog" on_cancel=UnsyncCallback::new(move |()| {
                    add_error.set(None);
                    add_open.set(false);
                })>
                    <form
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
                            aria-label="Feed URL"
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
                </Modal>
            </Show>
            <Show when=move || remove_target.get().is_some()>
                {move || remove_target.get().map(|feed| {
                    view! {
                        <Modal label="Remove feed" class="feed-dialog" on_cancel=UnsyncCallback::new(move |()| remove_target.set(None))>
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
                        </Modal>
                    }
                })}
            </Show>
        </aside>
    }
}

/// Quiet native OPML actions, also available in the empty library.
#[component]
pub(crate) fn SubscriptionFiles(#[prop(default = true)] export: bool) -> impl IntoView {
    let controller = use_feed_controller();
    let run = move |import| {
        controller.file_busy.set(true);
        controller.error().set(None);
        spawn_feed_task(async move {
            if let Err(message) = feeds::subscription_file(controller, import).await {
                controller.error().try_set(Some(message));
            }
            controller.file_busy.try_set(false);
        });
    };
    view! { <div class="subscription-files">
        <button type="button" disabled=move || controller.file_busy.get() on:click=move |_| run(true)>"Import OPML"</button>
        <Show when=move || export><button type="button" disabled=move || controller.file_busy.get() on:click=move |_| run(false)>"Export OPML"</button></Show>
    </div> }
}

#[cfg(target_arch = "wasm32")]
fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_feed_task(_task: impl Future<Output = ()> + 'static) {}
