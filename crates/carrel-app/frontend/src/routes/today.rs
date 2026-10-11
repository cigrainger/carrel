//! Local library list routes.

use crate::api::{ItemFilter, ItemSummary, ItemView};
use crate::components::ItemList;
use crate::feeds::{self, use_feed_controller};
use crate::item_actions::{ItemAction, ItemActions, ItemState, use_item_actions};
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::ensure_list_cursor_visible;
use crate::navigation::{
    ListNavigation, ListSnapshot, list_scroll_y, restore_list_scroll, restored_index,
    use_list_navigation_context,
};
#[cfg(target_arch = "wasm32")]
use crate::{api, requests::Requests};
use carrel_core::keymap::default;
use leptos::prelude::*;
use leptos_router::{
    NavigateOptions,
    components::A,
    hooks::{use_navigate, use_query_map},
};

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
    fn local(title: &str, filter: ItemFilter, path: &str, empty: &str) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            filter,
            return_path: path.into(),
            empty_message: empty.into(),
            fetch_url: None,
        }
    }
}

#[component]
pub fn Today() -> impl IntoView {
    list_route_view(|| {
        let mut spec = ListSpec::local(
            "Today",
            ItemFilter::today(),
            "/today",
            "No posts published in the past 24 hours.",
        );
        spec.subtitle = Some("Published in the past 24 hours · from your subscriptions".into());
        spec
    })
}
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
#[component]
pub fn Unread() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Library",
            ItemFilter::unread(),
            "/unread",
            "No unread articles.",
        )
    })
}
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
#[component]
pub fn Starred() -> impl IntoView {
    list_route_view(|| {
        ListSpec::local(
            "Library",
            ItemFilter::starred(),
            "/starred",
            "No starred articles.",
        )
    })
}

pub(crate) fn list_route_view(
    spec_builder: impl Fn() -> ListSpec + Send + Sync + 'static,
) -> impl IntoView {
    let base_spec = Memo::new(move |_| spec_builder());
    let query_map = use_query_map();
    let query = Memo::new(move |_| {
        query_map.with(|map| {
            map.get_str("q")
                .filter(|query| !query.trim().is_empty())
                .map(str::to_owned)
        })
    });
    let spec = Memo::new(move |_| {
        let mut spec = base_spec.get();
        spec.filter.search = query.get();
        spec.return_path = search_path(&spec.return_path, spec.filter.search.as_deref());
        spec
    });
    let draft = RwSignal::new(query.get_untracked().unwrap_or_default());
    Effect::new(move |_| draft.set(query.get().unwrap_or_default()));
    let search_input = NodeRef::<leptos::html::Input>::new();
    let navigate_search = use_navigate();
    let commit_search = UnsyncCallback::new(move |value: String| {
        navigate_search(
            &search_path(&base_spec.get_untracked().return_path, Some(&value)),
            NavigateOptions {
                replace: true,
                scroll: false,
                ..NavigateOptions::default()
            },
        );
    });
    #[cfg(target_arch = "wasm32")]
    let search_timer = StoredValue::new(None::<leptos::leptos_dom::helpers::TimeoutHandle>);
    #[cfg(target_arch = "wasm32")]
    on_cleanup(move || {
        if let Some(timer) = search_timer.get_value() {
            timer.clear();
        }
    });
    let submit_search = UnsyncCallback::new(move |value: String| {
        #[cfg(target_arch = "wasm32")]
        if let Some(timer) = search_timer.get_value() {
            timer.clear();
        }
        commit_search.run(value);
    });
    let change_search = UnsyncCallback::new(move |value: String| {
        draft.set(value.clone());
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(timer) = search_timer.get_value() {
                timer.clear();
            }
            search_timer.set_value(
                set_timeout_with_handle(
                    move || commit_search.run(value),
                    std::time::Duration::from_millis(200),
                )
                .ok(),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        commit_search.run(value);
    });
    let clear_search = UnsyncCallback::new(move |()| {
        draft.set(String::new());
        submit_search.run(String::new());
    });
    use_action_handler("search-list", move || {
        if let Some(input) = search_input.get() {
            let _ = input.focus();
        }
    });
    use_action_handler("clear-search", move || clear_search.run(()));
    let controller = use_feed_controller();
    let search_status = controller.search_status;
    let list_navigation = use_list_navigation_context();
    let snapshot = list_navigation.snapshot(&spec.get_untracked().return_path);
    let items = RwSignal::new(
        snapshot
            .as_ref()
            .map(|snapshot| snapshot.items.clone())
            .unwrap_or_default(),
    );
    let cursor = RwSignal::new(
        snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.selected_index),
    );
    let loaded = RwSignal::new(snapshot.is_some());
    let next_cursor = RwSignal::new(
        snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.item_cursor.clone()),
    );
    let complete = RwSignal::new(
        snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.items_complete),
    );
    let paging = RwSignal::new(false);
    let older = RwSignal::new(0_u64);
    let error = RwSignal::new(None::<String>);
    let retry_older = RwSignal::new(false);
    let reload = RwSignal::new(0_u64);
    let actions = use_item_actions();
    let visible = Memo::new(move |_| {
        let action_state = actions.state.get();
        let view = spec.get().filter.view;
        items
            .get()
            .into_iter()
            .filter_map(|mut item| {
                let state = action_state.project(
                    &item.id,
                    ItemState {
                        read: item.read_state.clone(),
                        starred: item.starred,
                    },
                );
                item.read_state = state.read;
                item.starred = state.starred;
                matches_view(&item, &view).then_some(item)
            })
            .collect::<Vec<_>>()
    });
    let restore = RwSignal::new(Some(snapshot.unwrap_or_default()));
    let previous_ids = RwSignal::new(Vec::<String>::new());
    Effect::new(move |_| {
        let rows = visible.get();
        if !loaded.get() {
            return;
        }
        if let Some(snapshot) = restore.get_untracked() {
            let context = list_navigation
                .get()
                .filter(|context| context.return_path == spec.get_untracked().return_path);
            let selected = context
                .as_ref()
                .map(|context| context.current_id.as_str())
                .or(snapshot.selected_id.as_deref());
            let original_ids = context
                .as_ref()
                .map(|context| context.item_ids.clone())
                .unwrap_or_else(|| snapshot.items.iter().map(|item| item.id.clone()).collect());
            cursor.set(restored_index(
                &rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
                selected,
                snapshot.selected_index,
                &original_ids,
            ));
            restore.set(None);
            restore_list_scroll(
                spec.get_untracked().return_path,
                snapshot.scroll_y,
                context.is_some() && !search_has_focus(),
            );
        } else {
            let original = previous_ids.get_untracked();
            cursor.update(|index| {
                *index = restored_index(
                    &rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
                    original.get(*index).map(String::as_str),
                    *index,
                    &original,
                );
            });
        }
        previous_ids.set(rows.iter().map(|row| row.id.clone()).collect());
    });
    use_keymap_layer(default::list_layer());
    use_keymap_layer(default::item_action_layer());
    for (name, forward, unread) in [
        ("next-item", true, false),
        ("prev-item", false, false),
        ("next-unread-item", true, true),
        ("prev-unread-item", false, true),
    ] {
        use_action_handler(name, move || {
            let rows = visible.get_untracked();
            let current = cursor.get_untracked();
            let index = if unread {
                if forward {
                    rows.iter()
                        .enumerate()
                        .skip(current + 1)
                        .find(|(_, row)| row.read_state == "unread")
                        .map(|(index, _)| index)
                } else {
                    rows.iter()
                        .enumerate()
                        .take(current)
                        .rev()
                        .find(|(_, row)| row.read_state == "unread")
                        .map(|(index, _)| index)
                }
            } else {
                Some(if forward {
                    (current + 1).min(rows.len().saturating_sub(1))
                } else {
                    current.saturating_sub(1)
                })
            };
            if let Some(index) = index {
                cursor.set(index);
                ensure_list_cursor_visible(spec.get_untracked().return_path);
            }
        });
    }
    use_action_handler("top-of-list", move || {
        cursor.set(0);
        ensure_list_cursor_visible(spec.get_untracked().return_path);
    });
    use_action_handler("end-of-list", move || {
        cursor.set(visible.get_untracked().len().saturating_sub(1));
        ensure_list_cursor_visible(spec.get_untracked().return_path);
    });
    let navigate = use_navigate();
    let open = UnsyncCallback::new(move |index: usize| {
        let rows = visible.get_untracked();
        let Some(item) = rows.get(index) else {
            return;
        };
        let current = spec.get_untracked();
        list_navigation.remember(
            current.return_path.clone(),
            ListSnapshot {
                items: items.get_untracked(),
                selected_id: Some(item.id.clone()),
                selected_index: index,
                scroll_y: list_scroll_y(),
                item_cursor: next_cursor.get_untracked(),
                items_complete: complete.get_untracked(),
                ..Default::default()
            },
        );
        list_navigation.set(ListNavigation {
            item_ids: rows.iter().map(|item| item.id.clone()).collect(),
            current_id: item.id.clone(),
            return_path: current.return_path,
            return_label: current.title,
        });
        cursor.set(index);
        navigate(&format!("/item/{}", item.id), NavigateOptions::default());
    });
    use_action_handler("open-item", move || open.run(cursor.get_untracked()));
    for (name, action) in [
        ("mark-read", ItemAction::Read(true)),
        ("mark-unread", ItemAction::Read(false)),
        ("toggle-star", ItemAction::Star),
    ] {
        use_action_handler(name, move || {
            request_at(visible, cursor.get_untracked(), actions, action)
        });
    }
    use_action_handler("go-back", move || {
        if let Some(input) = search_input.get()
            && search_has_focus()
        {
            let _ = input.blur();
            ensure_list_cursor_visible(spec.get_untracked().return_path);
        }
    });
    let read = UnsyncCallback::new(move |index| {
        if let Some(item) = visible.get_untracked().get(index) {
            request_at(
                visible,
                index,
                actions,
                ItemAction::Read(item.read_state != "read"),
            );
        }
    });
    let star =
        UnsyncCallback::new(move |index| request_at(visible, index, actions, ItemAction::Star));

    #[cfg(target_arch = "wasm32")]
    {
        let requests = Requests::default();
        let cleanup = requests.clone();
        on_cleanup(move || cleanup.invalidate());
        let previous_path = RwSignal::new(spec.get_untracked().return_path);
        let previous_older = RwSignal::new(0_u64);
        Effect::new(move |_| {
            let current = spec.get();
            let older_request = older.get();
            let append = older_request != previous_older.get_untracked()
                && previous_path.get_untracked() == current.return_path;
            previous_older.set(older_request);
            if previous_path.get_untracked() != current.return_path {
                let snapshot = list_navigation.snapshot(&current.return_path);
                items.set(
                    snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.items.clone())
                        .unwrap_or_default(),
                );
                loaded.set(snapshot.is_some());
                next_cursor.set(
                    snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.item_cursor.clone()),
                );
                complete.set(
                    snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.items_complete),
                );
                error.set(None);
                cursor.set(0);
                restore.set(Some(snapshot.unwrap_or_default()));
                previous_path.set(current.return_path.clone());
                restore_list_scroll(current.return_path.clone(), 0, false);
            }
            let _ = controller.refresh_token().get();
            let _ = reload.get();
            let generation = requests.begin();
            let guard = requests.clone();
            let action_revision = actions.state.with_untracked(|state| state.revision());
            let boundary = next_cursor.get_untracked();
            let to_end = complete.get_untracked();
            let mut filter = current.filter.clone();
            if append {
                filter.after = boundary.clone();
            }
            paging.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = fetch_archive(
                    filter,
                    if append { None } else { boundary },
                    !append && to_end,
                    &guard,
                    generation,
                )
                .await;
                if !guard.accepts(generation) {
                    return;
                }
                match result {
                    Ok(Some(page)) => {
                        next_cursor.set(page.next_cursor.clone());
                        complete.set(page.next_cursor.is_none());
                        let mut rows = if append {
                            items.get_untracked()
                        } else {
                            Vec::new()
                        };
                        for row in page.items {
                            if let Some(existing) =
                                rows.iter_mut().find(|existing| existing.id == row.id)
                            {
                                *existing = row;
                            } else {
                                rows.push(row);
                            }
                        }
                        actions.state.update(|state| {
                            state.rebase(
                                action_revision,
                                &rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
                            )
                        });
                        // Retain pending rows so a failed filtered action can put them back.
                        let pending = actions.state.get_untracked();
                        for (index, row) in items.get_untracked().into_iter().enumerate() {
                            if pending.is_pending(&row.id)
                                && !rows.iter().any(|fetched| fetched.id == row.id)
                            {
                                rows.insert(index.min(rows.len()), row);
                            }
                        }
                        let focus = crate::navigation::list_has_focus();
                        let scroll_y = list_scroll_y();
                        items.set(rows);
                        if focus && !append {
                            restore_list_scroll(current.return_path, scroll_y, true);
                        }
                        error.set(None);
                        retry_older.set(false);
                    }
                    Err(err) => {
                        retry_older.set(append);
                        error.set(Some(format!("Could not load articles: {}", err.message())))
                    }
                    Ok(None) => return,
                }
                paging.set(false);
                loaded.set(true);
            });
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    loaded.set(true);

    view! {
        <section class="route-list" data-list-path=move || spec.get().return_path>
            <header class="route-header route-list-header">
                <div><h1>{move || spec.get().title}</h1>{move || spec.get().subtitle.map(|text| view! { <p>{text}</p> })}</div>
                {move || spec.get().fetch_url.map(|url| view! { <FeedFetchButton url=url/> })}
            </header>
            <Show when=move || matches!(spec.get().filter.view, ItemView::Library | ItemView::Unread | ItemView::Starred)>
                <nav class="library-filters" aria-label="Library filter">
                    <A href=move || search_path("/library", Some(&draft.get())) exact=true>"All"</A><A href=move || search_path("/unread", Some(&draft.get())) exact=true>"Unread"</A><A href=move || search_path("/starred", Some(&draft.get())) exact=true>"Starred"</A>
                </nav>
            </Show>
            <form class="list-search" role="search" on:submit=move |event| { event.prevent_default(); submit_search.run(draft.get_untracked()); }>
                <label for="article-search">{move || search_label(&spec.get())}</label>
                <div class="list-search-control">
                    <input id="article-search" type="search" node_ref=search_input autocomplete="off" spellcheck="false" maxlength="512"
                        aria-describedby="article-search-help" placeholder=move || search_label(&spec.get()) prop:value=move || draft.get()
                        on:input=move |event| change_search.run(event_target_value(&event))/>
                    <Show when=move || !draft.get().is_empty()><button type="button" on:click=move |_| clear_search.run(())>"Clear"</button></Show>
                </div>
                <p id="article-search-help" class="search-help">"Each word matches a word beginning in the title or cached text."</p>
                <Show when=move || query.get().is_some()>
                    <p class="search-status" role="status">{move || match search_status.get() {
                        crate::api::SearchStatus::Preparing => "Preparing cached text. Titles are searchable now.",
                        crate::api::SearchStatus::Partial => "Some cached text is unavailable. Titles remain searchable.",
                        crate::api::SearchStatus::Failed => "Could not prepare cached text. Titles remain searchable.",
                        crate::api::SearchStatus::Ready => "",
                    }}
                        <Show when=move || matches!(search_status.get(), crate::api::SearchStatus::Partial | crate::api::SearchStatus::Failed)>
                            <button type="button" on:click=move |_| feeds::retry_search(controller)>"Retry cached text"</button>
                        </Show>
                    </p>
                </Show>
            </form>
            {move || error.get().map(|message| view! { <div class="quiet-state" role="status"><p>{message}</p><button type="button" on:click=move |_| {
                if retry_older.get_untracked() { older.update(|token| *token += 1); } else { reload.update(|token| *token += 1); }
            }>"Try again"</button></div> })}
            <Show when=move || loaded.get()>
                <Show when=move || !visible.get().is_empty() fallback=move || view! { <Show when=move || error.get().is_none()><div class="quiet-state">
                    <Show when=move || query.get().is_some() fallback=move || view! {
                        <p>{spec.get().empty_message}
                            <Show when=move || matches!(spec.get().filter.view, ItemView::Today | ItemView::Reading)>
                                " "<A href="/library" attr:class="quiet-link">"Browse Library"</A>
                            </Show>
                        </p>
                        <EmptyListAddFeed/>
                    }>
                        <p>"No articles match in this view."</p><button type="button" on:click=move |_| clear_search.run(())>"Clear search"</button>
                    </Show>
                </div></Show> }>
                    <ItemList items=visible selected_index=cursor aria_label=spec.get_untracked().title on_open=open on_read=read on_star=star on_select=UnsyncCallback::new(move |index| cursor.set(index))/>
                </Show>
                <Show when=move || next_cursor.get().is_some()>
                    <div class="archive-footer"><button type="button" disabled=move || paging.get() on:click=move |_| older.update(|token| *token += 1)>"Load older"</button></div>
                </Show>
            </Show>
        </section>
    }
}

fn search_path(path: &str, query: Option<&str>) -> String {
    match query.filter(|query| !query.trim().is_empty()) {
        Some(query) => format!("{path}?q={}", feeds::percent_encode(query)),
        None => path.into(),
    }
}

fn search_label(spec: &ListSpec) -> String {
    match spec.filter.view {
        ItemView::Unread => "Search unread articles".into(),
        ItemView::Starred => "Search starred articles".into(),
        ItemView::Feed { .. } => "Search this feed".into(),
        _ => format!("Search {}", spec.title.to_lowercase()),
    }
}

fn search_has_focus() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element())
            .is_some_and(|element| element.id() == "article-search")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Refresh through the loaded boundary, preserving the older end of the list.
#[cfg(target_arch = "wasm32")]
async fn fetch_archive(
    mut filter: ItemFilter,
    boundary: Option<crate::api::ItemCursor>,
    to_end: bool,
    requests: &Requests,
    generation: u64,
) -> Result<Option<crate::api::ItemPage<ItemSummary>>, crate::api::ApiError> {
    let mut items = Vec::new();
    loop {
        let page = api::list_items(filter.clone()).await?;
        if !requests.accepts(generation) {
            return Ok(None);
        }
        for row in page.items {
            if !items
                .iter()
                .any(|existing: &ItemSummary| existing.id == row.id)
            {
                items.push(row);
            }
        }
        let continue_refresh = page.next_cursor.as_ref().is_some_and(|next| {
            to_end
                || boundary
                    .as_ref()
                    .is_some_and(|boundary| !boundary.reached_by(next))
        });
        if !continue_refresh {
            return Ok(Some(crate::api::ItemPage {
                items,
                next_cursor: page.next_cursor,
            }));
        }
        filter.after = page.next_cursor;
    }
}

#[component]
fn EmptyListAddFeed() -> impl IntoView {
    let controller = crate::feeds::use_feed_controller();
    view! { <Show when=move || controller.feeds().get().is_empty()>
        <button type="button" on:click=move |_| controller.add_open.set(true)>"Add a feed"</button>
        <crate::components::SubscriptionFiles export=false/>
    </Show> }
}

fn matches_view(item: &ItemSummary, view: &ItemView) -> bool {
    match view {
        ItemView::Unread => item.read_state == "unread",
        ItemView::Reading => item.read_state == "reading",
        ItemView::Starred => item.starred,
        _ => true,
    }
}

fn request_at(
    items: Memo<Vec<ItemSummary>>,
    index: usize,
    actions: ItemActions,
    action: ItemAction,
) {
    if let Some(item) = items.get_untracked().get(index) {
        actions.request(
            item.id.clone(),
            ItemState {
                read: item.read_state.clone(),
                starred: item.starred,
            },
            action,
        );
    }
}

#[component]
fn FeedFetchButton(url: String) -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    {
        let controller = use_feed_controller();
        let status = controller.status();
        let busy = RwSignal::new(false);
        view! { <button type="button" disabled=move || status.get().is_fetching || busy.get() on:click=move |_| {
            busy.set(true); let url = url.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(message) = feeds::fetch_one(controller, url).await { controller.error().try_set(Some(message)); }
                busy.try_set(false);
            });
        }>"Fetch"</button> }.into_any()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = url;
        view! { <button type="button" disabled=true>"Fetch"</button> }.into_any()
    }
}
