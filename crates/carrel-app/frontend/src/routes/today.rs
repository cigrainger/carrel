//! Local library list routes.

use crate::api::{ItemFilter, ItemSummary, ItemView};
use crate::components::ItemList;
use crate::item_actions::{ItemAction, ItemActions, ItemState, use_item_actions};
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::ensure_list_cursor_visible;
use crate::navigation::{
    ListNavigation, ListSnapshot, list_scroll_y, restore_list_scroll, restored_index,
    use_list_navigation_context,
};
#[cfg(target_arch = "wasm32")]
use crate::{
    api,
    feeds::{self, use_feed_controller},
    requests::Requests,
};
use carrel_core::keymap::default;
use leptos::prelude::*;
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};

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
            "Nothing new in the past 24 hours.",
        );
        spec.subtitle = Some("From your subscriptions · past 24 hours".into());
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
            "Nothing in progress. Open an article to begin.",
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
    let spec = Memo::new(move |_| spec_builder());
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
    let error = RwSignal::new(None::<String>);
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
                context.is_some(),
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
    use_action_handler("go-back", move || {});
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
        let controller = use_feed_controller();
        let requests = Requests::default();
        let cleanup = requests.clone();
        on_cleanup(move || cleanup.invalidate());
        let previous_path = RwSignal::new(spec.get_untracked().return_path);
        Effect::new(move |_| {
            let current = spec.get();
            if previous_path.get_untracked() != current.return_path {
                let snapshot = list_navigation.snapshot(&current.return_path);
                items.set(
                    snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.items.clone())
                        .unwrap_or_default(),
                );
                loaded.set(snapshot.is_some());
                error.set(None);
                cursor.set(0);
                restore.set(Some(snapshot.unwrap_or_default()));
                previous_path.set(current.return_path.clone());
            }
            let _ = controller.refresh_token().get();
            let _ = reload.get();
            let generation = requests.begin();
            let guard = requests.clone();
            let action_revision = actions.state.with_untracked(|state| state.revision());
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::list_items(current.filter).await;
                if !guard.accepts(generation) {
                    return;
                }
                match result {
                    Ok(mut rows) => {
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
                        if focus {
                            restore_list_scroll(current.return_path, scroll_y, true);
                        }
                        error.set(None);
                    }
                    Err(err) => {
                        error.set(Some(format!("Could not load articles: {}", err.message())))
                    }
                }
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
                    <A href="/library" exact=true>"All"</A><A href="/unread" exact=true>"Unread"</A><A href="/starred" exact=true>"Starred"</A>
                </nav>
            </Show>
            {move || error.get().map(|message| view! { <div class="quiet-state" role="status"><p>{message}</p><button type="button" on:click=move |_| reload.update(|token| *token += 1)>"Try again"</button></div> })}
            <Show when=move || loaded.get()>
                <Show when=move || !visible.get().is_empty() fallback=move || view! { <Show when=move || error.get().is_none()><div class="quiet-state"><p>{spec.get().empty_message}</p><EmptyListAddFeed/></div></Show> }>
                    <ItemList items=visible selected_index=cursor aria_label=spec.get_untracked().title on_open=open on_read=read on_star=star on_select=UnsyncCallback::new(move |index| cursor.set(index))/>
                </Show>
            </Show>
        </section>
    }
}

#[component]
fn EmptyListAddFeed() -> impl IntoView {
    let controller = crate::feeds::use_feed_controller();
    view! { <Show when=move || controller.feeds().get().is_empty()>
        <button type="button" on:click=move |_| controller.add_open.set(true)>"Add a feed"</button>
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
