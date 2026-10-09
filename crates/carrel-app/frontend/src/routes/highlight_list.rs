//! Local highlights route.

use carrel_core::keymap::default;
use leptos::ev;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

#[cfg(target_arch = "wasm32")]
use crate::api;
use crate::api::HighlightListRow;
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::ensure_list_cursor_visible;
use crate::navigation::{
    ListNavigation, ListSnapshot, list_scroll_y, restore_list_scroll, restored_index,
    use_list_navigation_context,
};
#[cfg(target_arch = "wasm32")]
use crate::requests::Requests;
use crate::status::{StatusController, use_status_controller};

#[cfg(target_arch = "wasm32")]
const HIGHLIGHTS_LIMIT: usize = 100;
const RETURN_PATH: &str = "/highlights";

/// Browse local highlights.
#[component]
pub fn Highlights() -> impl IntoView {
    let list_navigation = use_list_navigation_context();
    let snapshot = list_navigation.snapshot(RETURN_PATH);
    let rows = RwSignal::new(
        snapshot
            .as_ref()
            .map(|snapshot| snapshot.highlights.clone())
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
    let navigate = use_navigate();
    let status = use_status_controller();

    use_keymap_layer(default::list_layer());

    let restore = RwSignal::new(snapshot);
    let previous_ids = RwSignal::new(Vec::<String>::new());
    Effect::new(move |_| {
        if !loaded.get() {
            return;
        }
        let current = rows.get();
        if let Some(snapshot) = restore.get_untracked() {
            cursor.set(restored_index(
                &current
                    .iter()
                    .map(|row| row.highlight.id.clone())
                    .collect::<Vec<_>>(),
                snapshot.selected_id.as_deref(),
                snapshot.selected_index,
                &snapshot
                    .highlights
                    .iter()
                    .map(|row| row.highlight.id.clone())
                    .collect::<Vec<_>>(),
            ));
            restore.set(None);
            restore_list_scroll(RETURN_PATH.into(), snapshot.scroll_y, true);
        } else {
            let original = previous_ids.get_untracked();
            cursor.update(|index| {
                *index = restored_index(
                    &current
                        .iter()
                        .map(|row| row.highlight.id.clone())
                        .collect::<Vec<_>>(),
                    original.get(*index).map(String::as_str),
                    *index,
                    &original,
                );
            });
        }
        previous_ids.set(current.iter().map(|row| row.highlight.id.clone()).collect());
    });

    use_action_handler("next-item", move || {
        move_cursor(rows, cursor, CursorMove::Next);
    });
    use_action_handler("prev-item", move || {
        move_cursor(rows, cursor, CursorMove::Previous);
    });
    use_action_handler("next-unread-item", move || {
        move_cursor(rows, cursor, CursorMove::Next);
    });
    use_action_handler("prev-unread-item", move || {
        move_cursor(rows, cursor, CursorMove::Previous);
    });
    use_action_handler("top-of-list", move || {
        cursor.set(0);
        ensure_list_cursor_visible(RETURN_PATH.into());
    });
    use_action_handler("end-of-list", move || {
        let len = rows.with_untracked(Vec::len);
        if len > 0 {
            cursor.set(len - 1);
            ensure_list_cursor_visible(RETURN_PATH.into());
        }
    });

    let open_navigate = navigate.clone();
    use_action_handler("open-item", move || {
        open_highlight_at(
            rows,
            cursor,
            list_navigation,
            open_navigate.clone(),
            status,
            cursor.get_untracked(),
        );
    });

    let row_navigate = navigate.clone();
    let open_row = UnsyncCallback::new(move |index| {
        open_highlight_at(
            rows,
            cursor,
            list_navigation,
            row_navigate.clone(),
            status,
            index,
        );
    });

    use_action_handler("go-back", move || {
        navigate("/today", NavigateOptions::default());
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;
        let requests = Requests::default();
        let cleanup = requests.clone();
        on_cleanup(move || cleanup.invalidate());
        let feeds = crate::feeds::use_feed_controller();
        Effect::new(move |_| {
            let _ = reload.get();
            let _ = feeds.refresh_token().get();
            let generation = requests.begin();
            let guard = requests.clone();
            spawn_local(async move {
                let result = api::list_highlights(Some(HIGHLIGHTS_LIMIT)).await;
                if !guard.accepts(generation) {
                    return;
                }
                match result {
                    Ok(highlights) => {
                        let focus = crate::navigation::list_has_focus();
                        let scroll_y = list_scroll_y();
                        rows.set(highlights);
                        if focus {
                            restore_list_scroll(RETURN_PATH.into(), scroll_y, true);
                        }
                        error.set(None);
                    }
                    Err(err) => {
                        let message = err.message().to_string();
                        status.warn(format!("Could not load highlights: {message}"));
                        error.set(Some(message));
                    }
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
        <section class="route-list highlights-route" data-list-path=RETURN_PATH>
            <header class="route-header route-list-header">
                <div>
                    <h1>"Highlights"</h1>
                    <p>"Passages you marked for return."</p>
                </div>
            </header>
            {move || error.get().map(|message| view! { <div class="quiet-state" role="status"><p>{message}</p><button type="button" on:click=move |_| reload.update(|token| *token += 1)>"Try again"</button></div> })}
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="quiet-state" aria-hidden="true"></div> }
            >
                <Show when=move || !rows.get().is_empty() fallback=move || view! {
                    <Show when=move || error.get().is_none()><p class="quiet-state">"No highlights yet. Mark a passage in the reader and it will appear here."</p></Show>
                }>
                        <HighlightList
                            rows=rows
                            selected_index=cursor
                            on_open=open_row
                            on_select=UnsyncCallback::new(move |index| cursor.set(index))
                        />
                </Show>
            </Show>
        </section>
    }
}

#[component]
fn HighlightList(
    #[prop(into)] rows: Signal<Vec<HighlightListRow>>,
    #[prop(into)] selected_index: Signal<usize>,
    on_open: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
) -> impl IntoView {
    view! {
        <ul class="highlight-list" aria-label="Highlights">
            <For
                each=move || { rows.get().into_iter().enumerate().collect::<Vec<_>>() }
                key=|(index, row)| (*index, row.highlight.id.clone())
                let:row
            >
                {
                    let (index, row) = row;
                    let row = highlight_row_data(rows, index, row);
                    view! {
                        <HighlightRow
                            index=index
                            row=row
                            selected=Signal::derive(move || index == selected_index.get())
                            on_open=on_open
                            on_select=on_select
                        />
                    }
                }
            </For>
        </ul>
    }
}

fn highlight_row_data(
    rows: Signal<Vec<HighlightListRow>>,
    index: usize,
    fallback: HighlightListRow,
) -> Memo<HighlightListRow> {
    Memo::new(move |_| {
        rows.with(|rows| {
            rows.get(index)
                .filter(|row| row.highlight.id == fallback.highlight.id)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        })
    })
}

#[component]
fn HighlightRow(
    index: usize,
    #[prop(into)] row: Signal<HighlightListRow>,
    selected: Signal<bool>,
    on_open: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
) -> impl IntoView {
    view! {
        <li class="highlight-row" class:is-selected=move || selected.get() on:focusin=move |_| on_select.run(index)
            on:click=move |_| { if should_open_from_row_click() { on_open.run(index); } }>
            <span class="highlight-row-mark" aria-hidden="true"></span>
            <div class="highlight-row-body">
                <blockquote>{move || row.get().highlight.quoted_text}</blockquote>
                {move || row.get().note_excerpt.map(|note| view! { <p class="highlight-note">{note}</p> })}
                <div class="highlight-row-footer">
                    <p class="highlight-source">
                        <span>{move || row.get().item_title}</span><span>{move || row.get().item_source_name}</span>
                        <span>{move || created_date_label(&row.get().highlight.created_at)}</span>
                    </p>
                    <button type="button" class="highlight-open" on:click=move |event: ev::MouseEvent| { event.stop_propagation(); on_open.run(index); }>"Open source"</button>
                </div>
            </div>
        </li>
    }
}

fn open_highlight_at(
    rows: RwSignal<Vec<HighlightListRow>>,
    cursor: RwSignal<usize>,
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions),
    status: StatusController,
    index: usize,
) {
    let list_context = rows.with_untracked(|rows| {
        let row = rows.get(index)?;
        Some((
            row.highlight.item_id.clone(),
            row.highlight.id.clone(),
            ListNavigation {
                item_ids: unique_item_ids(rows),
                current_id: row.highlight.item_id.clone(),
                return_path: RETURN_PATH.to_string(),
                return_label: "Highlights".to_string(),
            },
        ))
    });
    let Some((item_id, highlight_id, list_context)) = list_context else {
        status.info("No highlight selected.");
        return;
    };

    cursor.set(index);
    list_navigation.remember(
        RETURN_PATH.into(),
        ListSnapshot {
            highlights: rows.get_untracked(),
            selected_id: Some(highlight_id.clone()),
            selected_index: index,
            scroll_y: list_scroll_y(),
            ..Default::default()
        },
    );
    list_navigation.set(list_context);
    navigate(
        &format!("/item/{item_id}#highlight-{highlight_id}"),
        NavigateOptions::default(),
    );
}

fn unique_item_ids(rows: &[HighlightListRow]) -> Vec<String> {
    let mut ids = Vec::new();
    for row in rows {
        if !ids.iter().any(|id| id == &row.highlight.item_id) {
            ids.push(row.highlight.item_id.clone());
        }
    }
    ids
}

fn created_date_label(created_at: &str) -> String {
    created_at
        .split_once('T')
        .map(|(date, _)| date)
        .filter(|date| !date.is_empty())
        .unwrap_or(created_at)
        .to_string()
}

#[derive(Clone, Copy)]
enum CursorMove {
    Next,
    Previous,
}

fn move_cursor(
    rows: RwSignal<Vec<HighlightListRow>>,
    cursor: RwSignal<usize>,
    direction: CursorMove,
) {
    let len = rows.with_untracked(Vec::len);
    if len == 0 {
        cursor.set(0);
        return;
    }

    cursor.update(|index| match direction {
        CursorMove::Next => *index = (*index + 1).min(len - 1),
        CursorMove::Previous => *index = index.saturating_sub(1),
    });
    ensure_list_cursor_visible(RETURN_PATH.into());
}

#[cfg(target_arch = "wasm32")]
fn should_open_from_row_click() -> bool {
    web_sys::window()
        .and_then(|window| window.get_selection().ok().flatten())
        .map(|selection| {
            selection
                .to_string()
                .as_string()
                .unwrap_or_default()
                .trim()
                .is_empty()
        })
        .unwrap_or(true)
}

#[cfg(not(target_arch = "wasm32"))]
fn should_open_from_row_click() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use carrel_core::annotation::{HighlightLocation, HtmlTextSelector};

    use super::*;

    #[test]
    fn highlight_list_renders_quote_note_and_source_context() {
        let rows = vec![highlight_row(Some("Remember this phrasing."))];

        let html = Owner::new().with(|| {
            view! {
                <HighlightList
                    rows=Signal::stored(rows)
                    selected_index=Signal::stored(0)
                    on_open=UnsyncCallback::new(|_: usize| {})
                    on_select=UnsyncCallback::new(|_: usize| {})
                />
            }
            .to_html()
        });

        assert!(html.contains(r#"class="highlight-row is-selected""#));
        assert!(html.contains("The quiet web matters."));
        assert!(html.contains("Remember this phrasing."));
        assert!(html.contains("A Quiet Web"));
        assert!(html.contains("Example"));
        assert!(html.contains("2026-05-11"));
    }

    #[test]
    fn highlight_row_omits_note_when_absent() {
        let rows = vec![highlight_row(None)];

        let html = Owner::new().with(|| {
            view! {
                <HighlightList
                    rows=Signal::stored(rows)
                    selected_index=Signal::stored(0)
                    on_open=UnsyncCallback::new(|_: usize| {})
                    on_select=UnsyncCallback::new(|_: usize| {})
                />
            }
            .to_html()
        });

        assert!(!html.contains("highlight-note"));
    }

    #[test]
    fn a_refreshed_note_updates_an_existing_highlight_row() {
        Owner::new().with(|| {
            let initial = highlight_row(None);
            let rows = RwSignal::new(vec![initial.clone()]);
            let row = highlight_row_data(rows.into(), 0, initial);
            assert!(row.get_untracked().note_excerpt.is_none());
            rows.update(|rows| {
                rows[0].note_excerpt = Some("Saved after returning".into());
                rows[0].item_title = "Updated title".into();
            });
            assert_eq!(
                row.get_untracked().note_excerpt.as_deref(),
                Some("Saved after returning")
            );
            assert_eq!(row.get_untracked().item_title, "Updated title");
        });
    }

    fn highlight_row(note_excerpt: Option<&str>) -> HighlightListRow {
        HighlightListRow {
            highlight: crate::api::HighlightDetail {
                id: "highlight-1".to_string(),
                item_id: "item-1".to_string(),
                quoted_text: "The quiet web matters.".to_string(),
                location: HighlightLocation::HtmlText(HtmlTextSelector {
                    content_blob_id: Some("blob-1".to_string()),
                    exact: "The quiet web matters.".to_string(),
                    prefix: String::new(),
                    suffix: String::new(),
                    start: 0,
                    end: 22,
                }),
                location_label: None,
                color: Some("yellow".to_string()),
                created_by: "device".to_string(),
                created_at: "2026-05-11T05:30:00Z".to_string(),
                created_at_micros: 1_778_476_200_000_000,
                signature: "signature".to_string(),
            },
            item_title: "A Quiet Web".to_string(),
            item_source_name: "Example".to_string(),
            item_primary_url: Some("https://example.com/quiet".to_string()),
            note_excerpt: note_excerpt.map(str::to_string),
        }
    }
}
