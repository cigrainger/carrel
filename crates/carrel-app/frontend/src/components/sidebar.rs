//! Primary desktop navigation.

use std::future::Future;

use leptos::html;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

use super::source_organization::{self, DropTarget, Organization, OrganizationDialog};
use crate::api::{self, FeedCandidate};
use crate::components::{Icon, IconName, Modal};
use crate::feeds::{self, use_feed_controller};

#[derive(Clone, Copy)]
pub(crate) struct ContextBarTarget(NodeRef<html::Div>);

pub(crate) fn provide_context_bar() {
    provide_context(ContextBarTarget(NodeRef::new()));
}

/// Controls retain route ownership while rendering into the shared window bar.
#[component]
pub(crate) fn ContextControls(children: ChildrenFn) -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    {
        use leptos::portal::Portal;
        use wasm_bindgen::JsCast;
        let target = expect_context::<ContextBarTarget>().0;
        let children = StoredValue::new(children);
        view! {
            <Show when=move || target.get().is_some()>
                {move || {
                    target.get().map(|element| {
                        let children = children.get_value();
                        let element: web_sys::Element = element.unchecked_into();
                        view! { <Portal mount=element>{children()}</Portal> }
                    })
                }}
            </Show>
        }
        .into_any()
    }
    #[cfg(not(target_arch = "wasm32"))]
    children()
}

/// Compact window toolbar shared by lists and the reader.
#[component]
pub(crate) fn FieldNavigation() -> impl IntoView {
    let sources = crate::navigation::use_reading_sidebar();
    let controller = use_feed_controller();
    let fetch_busy = RwSignal::new(false);
    let capture = crate::components::save_article::use_capture_dialog();
    let save_button = NodeRef::<leptos::html::Button>::new();
    let target = expect_context::<ContextBarTarget>().0;
    let location = use_location();
    let reading = Memo::new(move |_| location.pathname.get().starts_with("/item/"));
    let title = Memo::new(move |_| {
        let path = location.pathname.get();
        match path.as_str() {
            "/" | "/today" => "Today".into(),
            "/reading" => "Reading".into(),
            "/library" => "Library".into(),
            "/unread" => "Library · Unread".into(),
            "/starred" => "Library · Starred".into(),
            "/friends" => "Shared with you".into(),
            "/highlights" => "Highlights".into(),
            _ if path.starts_with("/folders/") => {
                feeds::decode_feed_path(path.trim_start_matches("/folders/")).unwrap_or_default()
            }
            _ => controller.feeds().with(|feeds| {
                feeds
                    .iter()
                    .find(|feed| feeds::feed_href(&feed.url) == path)
                    .map(|feed| feed.display_name.clone())
                    .unwrap_or_default()
            }),
        }
    });
    view! {
        <header class="field-navigation" data-tauri-drag-region="deep">
            <div class="context-leading field-tools">
                <button type="button" class:active=move || sources.0.get() aria-label="Show or hide sidebar" title="Show or hide sidebar (⌘B)" aria-controls="reading-sidebar" aria-expanded=move || sources.0.get().to_string() on:click=move |_| sources.toggle()>{move || view! { <Icon name=if sources.0.get() { IconName::SidebarClose } else { IconName::SidebarOpen }/> }}</button>
            </div>
            <div class="context-content">
                <Show when=move || !reading.get()><span class="context-title">{move || title.get()}</span></Show>
                <div class="context-controls" node_ref=target></div>
            </div>
            <div class="context-tools field-tools">
                <Show when=move || !reading.get()>
                <button type="button" aria-label="Refresh feeds" title="Refresh feeds (⌘R)" disabled=move || fetch_busy.get() || controller.manual_fetch_busy.get() || controller.status().get().is_fetching on:click=move |_| {
                    fetch_busy.set(true);
                    spawn_feed_task(async move {
                        if let Err(message) = feeds::fetch_all(controller).await { controller.error().try_set(Some(message)); }
                        fetch_busy.try_set(false);
                    });
                }><Icon name=IconName::Refresh/></button>
                <button node_ref=save_button type="button" aria-label="Save article" title="Save article (⌘N)" on:click=move |_| capture.open_from(save_button)><Icon name=IconName::Add/></button>
                </Show>
                <Show when=move || reading.get()><crate::components::AppearanceMenu/></Show>
                <ContextMenu/>
            </div>
        </header>
    }
}

#[component]
fn ContextMenu() -> impl IntoView {
    let (can_copy, copy_link) = crate::keymap::action("copy-carrel-link");
    let menu = NodeRef::<html::Details>::new();
    let trigger = NodeRef::<html::Summary>::new();
    let open = RwSignal::new(false);
    let preferences = crate::preferences::use_preferences();
    let feeds = use_feed_controller();
    let welcome = crate::welcome::use_welcome();
    let capture = crate::components::save_article::use_capture_dialog();
    let close = UnsyncCallback::new(move |restore: bool| {
        if let Some(menu) = menu.get_untracked() {
            let _ = menu.remove_attribute("open");
        }
        open.set(false);
        if restore && let Some(trigger) = trigger.get_untracked() {
            let _ = trigger.focus();
        }
    });
    view! {
        <details class="context-menu" node_ref=menu on:toggle=move |_| open.set(menu.get_untracked().is_some_and(|menu| menu.has_attribute("open")))>
            <summary node_ref=trigger aria-label="App commands" title="App commands" aria-expanded=move || open.get().to_string()><Icon name=IconName::More/></summary>
            <Show when=move || open.get()><super::status_strip::PopoverDismissal name="app-menu" menu=menu on_close=close/></Show>
            <div class="context-menu-panel">
                <button type="button" on:click=move |_| { close.run(true); capture.0.set(true); }>"Save article"<kbd>"⌘N"</kbd></button>
                <button type="button" on:click=move |_| { close.run(true); feeds.add_open.set(true); }>"Add source"<kbd>"⇧⌘N"</kbd></button>
                <button type="button" disabled=move || feeds.manual_fetch_busy.get() || feeds.status().get().is_fetching on:click=move |_| {
                    close.run(true); spawn_feed_task(async move { if let Err(message) = feeds::fetch_all(feeds).await { feeds.error().try_set(Some(message)); } });
                }>"Refresh feeds"<kbd>"⌘R"</kbd></button>
                <hr/>
                <Show when=move || can_copy.get()><button type="button" on:click=move |_| { close.run(true); copy_link.run(()); }>"Copy Carrel link"</button></Show>
                <button type="button" on:click=move |_| { close.run(true); crate::keymap::open_palette(); }><span><Icon name=IconName::Search/>"Commands"</span><kbd>"⌘K"</kbd></button>
                <button type="button" on:click=move |_| { close.run(true); crate::keymap::open_help(); }>"Keyboard reference"<kbd>"?"</kbd></button>
                <button type="button" on:click=move |_| { close.run(true); welcome.show(); }>"Getting started"</button>
                <button type="button" on:click=move |_| { close.run(true); preferences.settings_open.set(true); }>"Settings"<kbd>"⌘,"</kbd></button>
            </div>
        </details>
    }
}

#[component]
fn PrimaryNavigation() -> impl IntoView {
    let path = use_navigation_path();
    let sidebar = crate::navigation::use_reading_sidebar();
    let destination = Memo::new(move |_| match path.get().as_str() {
        "/" | "/today" => "today",
        "/reading" => "reading",
        "/highlights" => "highlights",
        _ => "library",
    });
    view! {
        <nav class="field-destinations" aria-label="Primary">
            <a href="/today" aria-current=move || (destination.get() == "today").then_some("page") class=move || if destination.get() == "today" { "is-active" } else { "" } on:click=move |_| close_narrow_sidebar(sidebar)><Icon name=IconName::Today/><span>"Today"</span></a>
            <a href="/reading" aria-current=move || (destination.get() == "reading").then_some("page") class=move || if destination.get() == "reading" { "is-active" } else { "" } on:click=move |_| close_narrow_sidebar(sidebar)><Icon name=IconName::Reading/><span>"Reading"</span></a>
            <a href="/library" aria-current=move || (destination.get() == "library").then_some("page") class=move || if destination.get() == "library" { "is-active" } else { "" } on:click=move |_| close_narrow_sidebar(sidebar)><Icon name=IconName::Library/><span>"Library"</span></a>
            <a href="/highlights" aria-current=move || (destination.get() == "highlights").then_some("page") class=move || if destination.get() == "highlights" { "is-active" } else { "" } on:click=move |_| close_narrow_sidebar(sidebar)><Icon name=IconName::Highlights/><span>"Highlights"</span></a>
        </nav>
    }
}

// The selected navigation entry follows the originating list, including search.
fn use_navigation_path() -> Memo<String> {
    let location = use_location();
    let navigation = crate::navigation::use_list_navigation_context();
    Memo::new(move |_| {
        let path = location.pathname.get();
        let origin = if path.starts_with("/item/") {
            navigation
                .get()
                .map(|list| list.return_path)
                .unwrap_or_else(|| "/library".into())
        } else {
            path
        };
        origin.split('?').next().unwrap_or_default().to_string()
    })
}

fn close_narrow_sidebar(_sidebar: crate::navigation::ReadingSidebar) {
    #[cfg(target_arch = "wasm32")]
    if web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .is_some_and(|width| width <= 720.0)
    {
        _sidebar.0.set(false);
    }
}

/// One navigation sidebar, with subscription dialogs outside its hidden surface.
#[component]
pub fn Sidebar() -> impl IntoView {
    let active_path = use_navigation_path();
    let feed_controller = use_feed_controller();
    let organization = source_organization::provide();
    let feeds = organization.feeds;
    let controller_error = feed_controller.error();
    let add_open = feed_controller.add_open;
    let remove_target = feed_controller.remove_target;
    let remove_busy = RwSignal::new(false);
    let settings_button = NodeRef::<html::Button>::new();
    let preferences = crate::preferences::use_preferences();

    view! {
        <aside class="sidebar" id="reading-sidebar" aria-label="Navigation and sources">
            <div class="field-brand"><span class="field-mark" aria-hidden="true"></span><span>"carrel"</span></div>
            <PrimaryNavigation/>
            <div class="sidebar-section">
                <div class="sidebar-section-header">
                    <h2>"Sources"</h2>
                    <div class="sidebar-section-actions">
                        <button type="button" aria-label="New folder" title="New folder" disabled=move || feeds.get().is_empty() on:click=move |_| organization.new_folder()><Icon name=IconName::NewFolder/></button>
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
                    <div class="source-groups">
                    <DropTarget folder=None>
                        <Show when=move || organization.dragging() || feeds.with(|feeds| feeds.iter().any(|feed| feed.folder.is_some()))><p class="source-group-label">"Unfiled"</p></Show>
                        <SourceRows folder=None active_path=active_path/>
                    </DropTarget>
                    <For each=move || feeds.with(|feeds| feeds.iter().filter_map(|feed| feed.folder.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>()) key=|folder| folder.clone() let:folder>
                        <SourceFolder folder=folder active_path=active_path/>
                    </For>
                    </div>
                </Show>
            </div>
            <div class="sidebar-utilities">
                <button node_ref=settings_button type="button" title="Settings (⌘,)" on:click=move |_| {
                    if let Some(button) = settings_button.get_untracked() { let _ = button.focus(); }
                    preferences.settings_open.set(true);
                }><Icon name=IconName::Settings/><span>"Settings"</span></button>
            </div>
        </aside>
            <OrganizationDialog/>
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
fn SourceFolder(folder: String, active_path: Memo<String>) -> impl IntoView {
    let expanded = RwSignal::new(true);
    let href = feeds::folder_href(&folder);
    let selected_href = href.clone();
    let label = folder.clone();
    let heading = folder.clone();
    let disclosure = format!("Show or hide {folder} sources");
    let sidebar = crate::navigation::use_reading_sidebar();
    view! {
        <div class="source-group">
            <DropTarget folder=Some(folder.clone())><div class="source-group-heading">
                <button type="button" aria-label=disclosure aria-expanded=move || expanded.get().to_string() on:click=move |_| expanded.update(|open| *open = !*open)><span aria-hidden="true">{move || if expanded.get() { "▾" } else { "▸" }}</span></button>
                <a href=href title=label aria-current=move || active_path.with(|path| (path == &selected_href).then_some("page")) on:click=move |_| close_narrow_sidebar(sidebar)>{heading}</a>
            </div></DropTarget>
            <div hidden=move || !expanded.get()><SourceRows folder=Some(folder) active_path=active_path/></div>
        </div>
    }
}

#[component]
fn SourceRows(folder: Option<String>, active_path: Memo<String>) -> impl IntoView {
    let organization = expect_context::<Organization>();
    let feeds = organization.feeds;
    let sidebar = crate::navigation::use_reading_sidebar();
    view! {
        <ul class="feed-list">
            <For each=move || feeds.with(|feeds| feeds.iter().filter(|feed| feed.folder == folder).map(|feed| feed.url.clone()).collect::<Vec<_>>()) key=|url| url.clone() let:url>
                {
                    let href = feeds::feed_href(&url);
                    let selected_href = href.clone();
                    let drag_url = url.clone();
                    let busy_url = url.clone();
                    let current = Memo::new(move |_| feeds.with(|feeds| feeds.iter().find(|feed| feed.url == url).cloned()));
                    view! {
                        <li class="feed-nav-row">
                            <a href=href draggable=move || (!organization.busy(&busy_url)).to_string() on:dragstart=move |event| organization.start_drag(event, drag_url.clone()) on:dragend=move |_| organization.end_drag() aria-current=move || active_path.with(|path| (path == &selected_href).then_some("page")) title=move || current.with(|feed| feed.as_ref().map(|feed| feed.display_name.clone()).unwrap_or_default()) on:click=move |_| close_narrow_sidebar(sidebar)>{move || current.with(|feed| feed.as_ref().map(|feed| feed.display_name.clone()).unwrap_or_default())}</a>
                            <button type="button" aria-label=move || current.with(|feed| format!("Organize {}", feed.as_ref().map(|feed| feed.display_name.as_str()).unwrap_or_default())) title="Organize source" disabled=move || current.with(|feed| feed.as_ref().is_none_or(|feed| organization.busy(&feed.url))) on:click=move |_| { if let Some(feed) = current.get_untracked() { organization.organize(feed); } }><Icon name=IconName::More/></button>
                        </li>
                    }
                }
            </For>
        </ul>
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
pub(crate) fn SubscriptionFiles(
    #[prop(default = true)] export: bool,
    #[prop(default = false)] compact: bool,
) -> impl IntoView {
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
        <button type="button" aria-label="Import OPML" title="Import OPML" disabled=move || controller.file_busy.get() on:click=move |_| run(true)><Icon name=IconName::Import/><span>{if compact { "Import" } else { "Import OPML" }}</span></button>
        <Show when=move || export><button type="button" aria-label="Export OPML" title="Export OPML" disabled=move || controller.file_busy.get() on:click=move |_| run(false)><Icon name=IconName::Export/><span>{if compact { "Export" } else { "Export OPML" }}</span></button></Show>
    </div> }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn_feed_task(_task: impl Future<Output = ()> + 'static) {}
