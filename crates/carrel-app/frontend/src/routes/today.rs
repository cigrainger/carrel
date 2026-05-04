//! Today list route.

use carrel_core::keymap::default;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

use crate::api::ItemSummary;
#[cfg(target_arch = "wasm32")]
use crate::api::{self, ItemFilter};
use crate::components::TodayList;
use crate::keymap::{use_action_handler, use_keymap_layer};

/// Items discovered recently.
#[component]
pub fn Today() -> impl IntoView {
    let items = RwSignal::new(Vec::<ItemSummary>::new());
    let cursor = RwSignal::new(0_usize);
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    let navigate = use_navigate();

    use_keymap_layer(default::list_layer());
    use_keymap_layer(default::item_action_layer());

    Effect::new(move |_| {
        let len = items.with(Vec::len);
        cursor.update(|index| {
            if len == 0 {
                *index = 0;
            } else if *index >= len {
                *index = len - 1;
            }
        });
    });

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
        let item_id =
            items.with_untracked(|rows| rows.get(cursor.get_untracked()).map(|row| row.id.clone()));
        if let Some(item_id) = item_id {
            open_navigate(&format!("/item/{item_id}"), NavigateOptions::default());
        }
    });

    use_action_handler("go-back", move || {});

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        Effect::new(move |_| {
            spawn_local(async move {
                match api::list_items(ItemFilter::today()).await {
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
        <section class="route-today">
            <header class="route-header">
                <h1>"Today"</h1>
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
                        <p class="quiet-state">"No items today."</p>
                    }.into_any(),
                    None => view! {
                        <TodayList items=items.get() selected_index=cursor.get()/>
                    }.into_any(),
                }}
            </Show>
        </section>
    }
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
