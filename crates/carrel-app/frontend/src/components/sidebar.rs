//! Primary desktop navigation.

use std::future::Future;

use leptos::html;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::api::{self, FeedCandidate, FeedSummary};
use crate::components::{Icon, IconName, Modal};
use crate::feeds::{self, use_feed_controller};

/// Field's destination band contracts into a compact toolbar while reading.
#[component]
pub(crate) fn FieldNavigation() -> impl IntoView {
    let location = use_location();
    let navigation = crate::navigation::use_list_navigation_context();
    let sources = crate::navigation::use_reading_sidebar();
    let controller = use_feed_controller();
    let fetch_busy = RwSignal::new(false);
    let destination = Memo::new(move |_| {
        let path = location.pathname.get();
        let origin = if path.starts_with("/item/") {
            navigation
                .get()
                .map(|list| list.return_path)
                .unwrap_or_else(|| "/library".into())
        } else {
            path
        };
        match origin.as_str() {
            "/" | "/today" => "today",
            "/reading" => "reading",
            "/highlights" => "highlights",
            _ => "library",
        }
    });
    view! {
        <header class="field-navigation" data-tauri-drag-region="deep">
            <div class="field-brand"><span class="field-mark" aria-hidden="true"></span><span>"carrel"</span></div>
            <nav class="field-destinations" aria-label="Primary">
                <A href="/today" attr:class=move || if destination.get() == "today" { "is-active" } else { "" }><Icon name=IconName::Today/><span>"Today"</span></A>
                <A href="/reading" attr:class=move || if destination.get() == "reading" { "is-active" } else { "" }><Icon name=IconName::Reading/><span>"Reading"</span></A>
                <A href="/library" attr:class=move || if destination.get() == "library" { "is-active" } else { "" }><Icon name=IconName::Library/><span>"Library"</span></A>
                <A href="/highlights" attr:class=move || if destination.get() == "highlights" { "is-active" } else { "" }><Icon name=IconName::Highlights/><span>"Highlights"</span></A>
            </nav>
            <div class="field-tools">
                <button type="button" class:active=move || sources.0.get() aria-label="Show or hide sources" title="Show or hide sources (⌘B in reader)" aria-controls="reading-sidebar" aria-expanded=move || sources.0.get().to_string() on:click=move |_| sources.toggle()>{move || view! { <Icon name=if sources.0.get() { IconName::SidebarClose } else { IconName::SidebarOpen }/> }}</button>
                <button type="button" aria-label="Refresh feeds" title="Refresh feeds" disabled=move || fetch_busy.get() || controller.status().get().is_fetching on:click=move |_| {
                    fetch_busy.set(true);
                    spawn_feed_task(async move {
                        if let Err(message) = feeds::fetch_all(controller).await { controller.error().try_set(Some(message)); }
                        fetch_busy.try_set(false);
                    });
                }><Icon name=IconName::Refresh/></button>
                <button type="button" aria-label="Add source" title="Add source" on:click=move |_| controller.add_open.set(true)><Icon name=IconName::Add/></button>
            </div>
        </header>
    }
}

/// Source drawer, with subscription dialogs mounted outside its hidden surface.
#[component]
pub fn Sidebar() -> impl IntoView {
    let feed_controller = use_feed_controller();
    let feeds = feed_controller.feeds();
    let controller_error = feed_controller.error();
    let add_open = feed_controller.add_open;
    let remove_target = RwSignal::new(None::<FeedSummary>);
    let remove_busy = RwSignal::new(false);

    view! {
        <aside class="sidebar" id="reading-sidebar" aria-label="Sources">
            <div class="sidebar-section">
                <div class="sidebar-section-header">
                    <h2>"Sources"</h2>
                    <div class="sidebar-section-actions">
                        <button
                            type="button"
                            aria-label="Add source"
                            title="Add source"
                            on:click=move |_| add_open.set(true)
                        >
                            <Icon name=IconName::Add/>
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
                                            <Icon name=IconName::Close/>
                                        </button>
                                    </li>
                                }
                            }
                        </For>
                    </ul>
                </Show>
            </div>
            <div class="sidebar-utilities">
                <crate::components::devices::Devices/>
                <SubscriptionFiles/>
            </div>
        </aside>
            <Show when=move || add_open.get()>
                <AddSource/>
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
    }
}

#[component]
fn AddSource() -> impl IntoView {
    let controller = use_feed_controller();
    let sources = crate::navigation::use_reading_sidebar();
    let input = NodeRef::<html::Input>::new();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let candidates = RwSignal::new(Vec::<FeedCandidate>::new());
    let selected = RwSignal::new(String::new());
    let requests = StoredValue::new(crate::requests::Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let cancel = UnsyncCallback::new(move |()| {
        requests.get_value().invalidate();
        controller.add_open.set(false);
    });
    view! {
        <Modal label="Add a source" class="feed-dialog" on_cancel=cancel>
            <form on:submit=move |event| {
                event.prevent_default();
                if busy.get_untracked() { return; }
                let url = input.get_untracked().map(|input| input.value()).unwrap_or_default().trim().to_string();
                if url.is_empty() { error.set(Some("Enter a website or feed address".into())); return; }
                let chosen = selected.get_untracked();
                let guard = requests.get_value();
                let request = guard.begin();
                busy.set(true);
                error.set(None);
                spawn_feed_task(async move {
                    let result = if chosen.is_empty() {
                        api::discover_feeds(url).await.map_err(|error| error.message().to_string())
                    } else { Ok(vec![FeedCandidate { url: chosen, title: String::new() }]) };
                    // The dialog may have closed or its address may have changed during HTTP.
                    if !guard.accepts(request) { return; }
                    match result {
                        Ok(found) if found.len() == 1 => {
                            let saved = feeds::add_and_fetch(controller, found[0].url.clone()).await;
                            if !guard.accepts(request) {
                                if let Err(message) = saved { controller.error().try_set(Some(message)); }
                                return;
                            }
                            match saved {
                                Ok(()) => { sources.0.try_set(true); controller.add_open.try_set(false); }
                                Err(message) => { error.try_set(Some(message)); }
                            }
                        }
                        Ok(found) if !found.is_empty() => {
                            selected.try_set(found[0].url.clone());
                            candidates.try_set(found);
                        }
                        Ok(_) => { error.try_set(Some("No feed was advertised at this address".into())); }
                        Err(message) => { error.try_set(Some(message)); }
                    }
                    busy.try_set(false);
                });
            }>
                <h3>"Add a source"</h3>
                <label for="feed-url">"Website or feed address"</label>
                <input node_ref=input id="feed-url" name="feed-url" type="text" inputmode="url" autocomplete="url" placeholder="example.org/blog" autofocus=true on:input=move |_| {
                    requests.get_value().invalidate();
                    candidates.set(Vec::new()); selected.set(String::new()); error.set(None); busy.set(false);
                }/>
                <Show when=move || !candidates.get().is_empty()>
                    <fieldset class="feed-chooser"><legend>"Choose a feed"</legend>
                        <For each=move || candidates.get() key=|feed| feed.url.clone() let:feed>
                            {
                                let value = feed.url.clone();
                                let check = feed.url.clone();
                                view! { <label><input type="radio" name="discovered-feed" value=value.clone() checked=move || selected.get() == check on:change=move |_| selected.set(value.clone())/><span><strong>{feed.title}</strong><small>{feed.url}</small></span></label> }
                            }
                        </For>
                    </fieldset>
                </Show>
                {move || error.get().map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
                <div class="feed-dialog-actions">
                    <button type="button" on:click=move |_| cancel.run(())>"Cancel"</button>
                    <button type="submit" disabled=move || busy.get()>{move || if busy.get() { if selected.get().is_empty() { "Finding feeds…" } else { "Saving…" } } else if candidates.get().is_empty() { "Add source" } else { "Subscribe" }}</button>
                </div>
            </form>
        </Modal>
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
        <button type="button" disabled=move || controller.file_busy.get() on:click=move |_| run(true)><Icon name=IconName::Import/><span>"Import OPML"</span></button>
        <Show when=move || export><button type="button" disabled=move || controller.file_busy.get() on:click=move |_| run(false)><Icon name=IconName::Export/><span>"Export OPML"</span></button></Show>
    </div> }
}

#[cfg(target_arch = "wasm32")]
fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_feed_task(_task: impl Future<Output = ()> + 'static) {}
