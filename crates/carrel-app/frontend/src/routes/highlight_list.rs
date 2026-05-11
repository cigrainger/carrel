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
use crate::navigation::{ListNavigation, use_list_navigation_context};
use crate::status::{StatusController, use_status_controller};

#[cfg(target_arch = "wasm32")]
const HIGHLIGHTS_LIMIT: usize = 100;
const RETURN_PATH: &str = "/highlights";

/// Browse local highlights.
#[component]
pub fn Highlights() -> impl IntoView {
    let rows = RwSignal::new(Vec::<HighlightListRow>::new());
    let cursor = RwSignal::new(0_usize);
    let loaded = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let navigate = use_navigate();
    let list_navigation = use_list_navigation_context();
    let status = use_status_controller();

    use_keymap_layer(default::list_layer());

    Effect::new(move |_| clamp_cursor(rows, cursor));

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
    });
    use_action_handler("end-of-list", move || {
        let len = rows.with_untracked(Vec::len);
        if len > 0 {
            cursor.set(len - 1);
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

        spawn_local(async move {
            match api::list_highlights(Some(HIGHLIGHTS_LIMIT)).await {
                Ok(highlights) => {
                    rows.set(highlights);
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
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        loaded.set(true);
    }

    view! {
        <section class="route-list highlights-route">
            <header class="route-header route-list-header">
                <div>
                    <h1>"Highlights"</h1>
                    <p>"Passages you marked for return."</p>
                </div>
            </header>
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="quiet-state" aria-hidden="true"></div> }
            >
                {move || match error.get() {
                    Some(message) => view! {
                        <p class="quiet-state">{message}</p>
                    }.into_any(),
                    None if rows.get().is_empty() => view! {
                        <p class="quiet-state">"No highlights yet. Mark a passage in the reader and it will appear here."</p>
                    }.into_any(),
                    None => view! {
                        <HighlightList
                            rows=rows.get()
                            selected_index=cursor.get()
                            on_open=open_row
                        />
                    }.into_any(),
                }}
            </Show>
        </section>
    }
}

#[component]
fn HighlightList(
    rows: Vec<HighlightListRow>,
    selected_index: usize,
    on_open: UnsyncCallback<usize>,
) -> impl IntoView {
    let indexed_rows = rows.into_iter().enumerate().collect::<Vec<_>>();

    view! {
        <ul class="highlight-list" aria-label="Highlights">
            <For
                each=move || indexed_rows.clone()
                key=|(_, row)| row.highlight.id.clone()
                let:row
            >
                {
                    let (index, row) = row;
                    view! {
                        <HighlightRow
                            index=index
                            row=row
                            selected=index == selected_index
                            on_open=on_open
                        />
                    }
                }
            </For>
        </ul>
    }
}

#[component]
fn HighlightRow(
    index: usize,
    row: HighlightListRow,
    selected: bool,
    on_open: UnsyncCallback<usize>,
) -> impl IntoView {
    let class = if selected {
        "highlight-row is-selected"
    } else {
        "highlight-row"
    };
    let date = created_date_label(&row.highlight.created_at);
    let note = row.note_excerpt.clone();
    let row_open = on_open;
    let button_open = on_open;

    view! {
        <li
            class=class
            aria-selected=selected.to_string()
            on:click=move |_| {
                if should_open_from_row_click() {
                    row_open.run(index);
                }
            }
        >
            <span class="highlight-row-mark" aria-hidden="true"></span>
            <div class="highlight-row-body">
                <blockquote>{row.highlight.quoted_text.clone()}</blockquote>
                {note.map(|note| view! { <p class="highlight-note">{note}</p> })}
                <div class="highlight-row-footer">
                    <p class="highlight-source">
                        <span>{row.item_title.clone()}</span>
                        <span>{row.item_source_name.clone()}</span>
                        <span>{date}</span>
                    </p>
                    <button
                        type="button"
                        class="highlight-open"
                        on:click=move |event: ev::MouseEvent| {
                            event.stop_propagation();
                            button_open.run(index);
                        }
                    >
                        "Open source"
                    </button>
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
            },
        ))
    });
    let Some((item_id, highlight_id, list_context)) = list_context else {
        status.info("No highlight selected.");
        return;
    };

    cursor.set(index);
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

fn clamp_cursor(rows: RwSignal<Vec<HighlightListRow>>, cursor: RwSignal<usize>) {
    let len = rows.with_untracked(Vec::len);
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
                    rows=rows
                    selected_index=0
                    on_open=UnsyncCallback::new(|_: usize| {})
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
                    rows=rows
                    selected_index=0
                    on_open=UnsyncCallback::new(|_: usize| {})
                />
            }
            .to_html()
        });

        assert!(!html.contains("highlight-note"));
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
