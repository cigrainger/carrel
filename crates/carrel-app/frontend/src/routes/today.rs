//! Local library list routes.

use std::future::Future;

use carrel_core::keymap::default;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

#[cfg(target_arch = "wasm32")]
use crate::api::{self, ItemStateRequest};
use crate::api::{ItemFilter, ItemSummary, ItemView};
use crate::components::ItemList;
#[cfg(target_arch = "wasm32")]
use crate::feeds::{self, use_feed_controller};
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::{ListNavigation, ListNavigationContext, use_list_navigation_context};

/// Configuration for one local item-list surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ListSpec {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) filter: ItemFilter,
    pub(crate) return_path: String,
    pub(crate) empty_message: String,
    pub(crate) fetch_url: Option<String>,
}

impl ListSpec {
    fn local(title: &str, filter: ItemFilter, return_path: &str, empty_message: &str) -> Self {
        Self {
            title: title.to_string(),
            subtitle: None,
            filter,
            return_path: return_path.to_string(),
            empty_message: empty_message.to_string(),
            fetch_url: None,
        }
    }
}

/// Items discovered recently.
#[component]
pub fn Today() -> impl IntoView {
    list_route_view(|| ListSpec::local("Today", ItemFilter::today(), "/today", "No items today."))
}

/// All saved items.
#[component]
pub fn Library() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Library",
            ItemFilter::library(),
            "/library",
            "Your library is empty.",
        )
    })
}

/// Items that have not been read locally.
#[component]
pub fn Unread() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Unread",
            ItemFilter::unread(),
            "/unread",
            "No unread items.",
        )
    })
}

/// Items currently in progress.
#[component]
pub fn Reading() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Reading",
            ItemFilter::reading(),
            "/reading",
            "Nothing in progress.",
        )
    })
}

/// Starred items.
#[component]
pub fn Starred() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Starred",
            ItemFilter::starred(),
            "/starred",
            "No starred items.",
        )
    })
}

/// Shared route implementation for local item lists.
pub(crate) fn list_route_view(
    spec_builder: impl Fn() -> ListSpec + Send + Sync + 'static,
) -> impl IntoView {
    let spec = Memo::new(move |_| spec_builder());
    let items = RwSignal::new(Vec::<ItemSummary>::new());
    let cursor = RwSignal::new(0_usize);
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    let navigate = use_navigate();
    let list_navigation = use_list_navigation_context();

    use_keymap_layer(default::list_layer());
    use_keymap_layer(default::item_action_layer());

    Effect::new(move |_| clamp_cursor(items, cursor));

    use_action_handler("next-item", move || {
        move_cursor(items, cursor, CursorMove::Next);
    });
    use_action_handler("prev-item", move || {
        move_cursor(items, cursor, CursorMove::Previous);
    });
    use_action_handler("next-unread-item", move || {
        move_cursor_to_unread(items, cursor, CursorMove::Next);
    });
    use_action_handler("prev-unread-item", move || {
        move_cursor_to_unread(items, cursor, CursorMove::Previous);
    });
    use_action_handler("top-of-list", move || {
        cursor.set(0);
    });
    use_action_handler("end-of-list", move || {
        let len = items.with_untracked(Vec::len);
        if len > 0 {
            cursor.set(len - 1);
        }
    });

    let open_navigate = navigate.clone();
    use_action_handler("open-item", move || {
        open_item_at(
            items,
            cursor,
            list_navigation,
            open_navigate.clone(),
            spec,
            cursor.get_untracked(),
        );
    });
    let row_navigate = navigate.clone();
    let open_row = UnsyncCallback::new(move |index| {
        open_item_at(
            items,
            cursor,
            list_navigation,
            row_navigate.clone(),
            spec,
            index,
        );
    });

    use_action_handler("mark-read", move || {
        mark_selected_read(items, cursor, spec, error);
    });
    use_action_handler("mark-unread", move || {
        mark_selected_unread(items, cursor, spec, error);
    });
    use_action_handler("toggle-star", move || {
        toggle_selected_star(items, cursor, spec, error);
    });
    use_action_handler("go-back", move || {});

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        let feed_controller = use_feed_controller();
        Effect::new(move |_| {
            let current = spec.get();
            let _ = feed_controller.refresh_token().get();
            loaded.set(false);
            spawn_local(async move {
                match api::list_items(current.filter).await {
                    Ok(rows) => {
                        items.set(rows);
                        error.set(None);
                    }
                    Err(err) => error.set(Some(err.message().to_string())),
                }
                loaded.set(true);
            });
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        loaded.set(true);
    }

    view! {
        <section class="route-list">
            <header class="route-header route-list-header">
                <div>
                    <h1>{move || spec.get().title}</h1>
                    {move || spec.get().subtitle.map(|subtitle| {
                        view! { <p>{subtitle}</p> }
                    })}
                </div>
                {move || spec.get().fetch_url.map(|url| {
                    view! { <FeedFetchButton url=url/> }
                })}
            </header>
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="quiet-state" aria-hidden="true"></div> }
            >
                {move || match error.get() {
                    Some(message) => view! {
                        <p class="quiet-state">{message}</p>
                    }.into_any(),
                    None if items.get().is_empty() => view! {
                        <p class="quiet-state">{spec.get().empty_message}</p>
                    }.into_any(),
                    None => view! {
                        <ItemList
                            items=items.get()
                            selected_index=cursor.get()
                            aria_label=spec.get().title
                            on_open=open_row
                        />
                    }.into_any(),
                }}
            </Show>
        </section>
    }
}

#[component]
fn FeedFetchButton(url: String) -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    {
        let controller = use_feed_controller();
        let status = controller.status();
        let busy = RwSignal::new(false);

        view! {
            <button
                type="button"
                disabled=move || status.get().is_fetching || busy.get()
                on:click=move |_| {
                    busy.set(true);
                    let controller = controller;
                    let url = url.clone();
                    spawn_feed_task(async move {
                        if let Err(message) = feeds::fetch_one(controller, url).await {
                            controller.error().set(Some(message));
                        }
                        busy.set(false);
                    });
                }
            >
                "Fetch"
            </button>
        }
        .into_any()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = url;
        view! {
            <button type="button" disabled=true>"Fetch"</button>
        }
        .into_any()
    }
}

fn open_item_at(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    list_navigation: ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions),
    spec: Memo<ListSpec>,
    index: usize,
) {
    let return_path = spec.get_untracked().return_path;
    let list_context = items.with_untracked(|rows| {
        let item = rows.get(index)?;
        Some(ListNavigation {
            item_ids: rows.iter().map(|row| row.id.clone()).collect(),
            current_id: item.id.clone(),
            return_path,
        })
    });
    let Some(list_context) = list_context else {
        return;
    };

    cursor.set(index);
    let item_id = list_context.current_id.clone();
    list_navigation.set(list_context);
    navigate(&format!("/item/{item_id}"), NavigateOptions::default());
}

fn selected_item_id(items: RwSignal<Vec<ItemSummary>>, cursor: RwSignal<usize>) -> Option<String> {
    items.with_untracked(|rows| rows.get(cursor.get_untracked()).map(|row| row.id.clone()))
}

fn mark_selected_read(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    spec: Memo<ListSpec>,
    error: RwSignal<Option<String>>,
) {
    let Some(item_id) = selected_item_id(items, cursor) else {
        return;
    };
    let view = spec.get_untracked().filter.view;
    mutate_current_item(items, cursor, |row| {
        row.read_state = "read".to_string();
        !matches!(&view, ItemView::Unread | ItemView::Reading)
    });

    spawn_item_task(async move {
        if let Err(err) = api_mark_item_read(item_id).await {
            error.set(Some(err));
        }
    });
}

fn mark_selected_unread(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    spec: Memo<ListSpec>,
    error: RwSignal<Option<String>>,
) {
    let Some(item_id) = selected_item_id(items, cursor) else {
        return;
    };
    let view = spec.get_untracked().filter.view;
    mutate_current_item(items, cursor, |row| {
        row.read_state = "unread".to_string();
        !matches!(&view, ItemView::Reading)
    });

    spawn_item_task(async move {
        if let Err(err) = api_mark_item_unread(item_id).await {
            error.set(Some(err));
        }
    });
}

fn toggle_selected_star(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    spec: Memo<ListSpec>,
    error: RwSignal<Option<String>>,
) {
    let Some(item_id) = selected_item_id(items, cursor) else {
        return;
    };
    let view = spec.get_untracked().filter.view;
    let mut optimistic_starred = false;
    mutate_current_item(items, cursor, |row| {
        row.starred = !row.starred;
        optimistic_starred = row.starred;
        row.starred || !matches!(&view, ItemView::Starred)
    });

    spawn_item_task(async move {
        match api_toggle_item_star(item_id.clone()).await {
            Ok(starred) => reconcile_star(items, item_id, starred),
            Err(err) => {
                reconcile_star(items, item_id, !optimistic_starred);
                error.set(Some(err));
            }
        }
    });
}

fn mutate_current_item(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    mut update: impl FnMut(&mut ItemSummary) -> bool,
) {
    let index = cursor.get_untracked();
    items.update(|rows| {
        let Some(row) = rows.get_mut(index) else {
            return;
        };
        if !update(row) {
            rows.remove(index);
        }
    });
    clamp_cursor(items, cursor);
}

fn reconcile_star(items: RwSignal<Vec<ItemSummary>>, item_id: String, starred: bool) {
    items.update(|rows| {
        if let Some(row) = rows.iter_mut().find(|row| row.id == item_id) {
            row.starred = starred;
        }
    });
}

fn clamp_cursor(items: RwSignal<Vec<ItemSummary>>, cursor: RwSignal<usize>) {
    let len = items.with_untracked(Vec::len);
    cursor.update(|index| {
        if len == 0 {
            *index = 0;
        } else if *index >= len {
            *index = len - 1;
        }
    });
}

#[derive(Clone, Copy)]
enum CursorMove {
    Next,
    Previous,
}

fn move_cursor(items: RwSignal<Vec<ItemSummary>>, cursor: RwSignal<usize>, direction: CursorMove) {
    let len = items.with_untracked(Vec::len);
    if len == 0 {
        cursor.set(0);
        return;
    }

    cursor.update(|index| match direction {
        CursorMove::Next => *index = (*index + 1).min(len - 1),
        CursorMove::Previous => *index = index.saturating_sub(1),
    });
}

fn move_cursor_to_unread(
    items: RwSignal<Vec<ItemSummary>>,
    cursor: RwSignal<usize>,
    direction: CursorMove,
) {
    let next_index = items.with_untracked(|rows| {
        if rows.is_empty() {
            return None;
        }

        let current = cursor.get_untracked().min(rows.len() - 1);
        match direction {
            CursorMove::Next => rows
                .iter()
                .enumerate()
                .skip(current + 1)
                .find(|(_, item)| item.read_state == "unread")
                .map(|(index, _)| index),
            CursorMove::Previous => rows
                .iter()
                .enumerate()
                .take(current)
                .rev()
                .find(|(_, item)| item.read_state == "unread")
                .map(|(index, _)| index),
        }
    });

    if let Some(next_index) = next_index {
        cursor.set(next_index);
    }
}

#[cfg(target_arch = "wasm32")]
async fn api_mark_item_read(item_id: String) -> Result<(), String> {
    api::mark_item_read(ItemStateRequest { item_id })
        .await
        .map_err(|err| err.message().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn api_mark_item_read(_item_id: String) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn api_mark_item_unread(item_id: String) -> Result<(), String> {
    api::mark_item_unread(ItemStateRequest { item_id })
        .await
        .map_err(|err| err.message().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn api_mark_item_unread(_item_id: String) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn api_toggle_item_star(item_id: String) -> Result<bool, String> {
    api::toggle_item_star(ItemStateRequest { item_id })
        .await
        .map_err(|err| err.message().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn api_toggle_item_star(_item_id: String) -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_arch = "wasm32")]
fn spawn_item_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_item_task(_task: impl Future<Output = ()> + 'static) {}

#[cfg(target_arch = "wasm32")]
fn spawn_feed_task(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
