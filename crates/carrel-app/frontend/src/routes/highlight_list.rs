//! Local highlights route.

use std::collections::BTreeSet;

use carrel_core::keymap::default;
use leptos::ev;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;

#[cfg(target_arch = "wasm32")]
use crate::api;
use crate::api::{AnnotationCursor, HighlightListRow, NoteDetail};
use crate::components::Modal;
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::ensure_list_cursor_visible;
use crate::navigation::{
    ListNavigation, ListSnapshot, list_scroll_y, restore_list_scroll, restored_index,
    use_list_navigation_context,
};
#[cfg(target_arch = "wasm32")]
use crate::requests::Requests;
use crate::status::{StatusController, use_status_controller};

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
    let next_cursor = RwSignal::new(
        snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.annotation_cursor.clone()),
    );
    let complete = RwSignal::new(
        snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.annotations_complete),
    );
    let expanded = RwSignal::new(
        snapshot
            .as_ref()
            .map(|snapshot| snapshot.expanded_highlights.clone())
            .unwrap_or_default(),
    );
    let paging = RwSignal::new(false);
    let load_more = RwSignal::new(0_u64);
    let retry_older = RwSignal::new(false);
    let actions = crate::annotation_actions::use_annotation_actions();
    let displayed = Signal::derive(move || actions.project_rows(rows.get()));
    let editor = RwSignal::new(None::<EditDraft>);
    let edit_body = RwSignal::new(String::new());
    let editor_epoch = RwSignal::new(0_u64);
    let refreshing_note = RwSignal::new(false);
    let edit_error = RwSignal::new(None::<String>);
    let conflicted = RwSignal::new(false);
    let removal = RwSignal::new(None::<HighlightListRow>);
    let file_busy = RwSignal::new(false);
    let loaded = RwSignal::new(snapshot.is_some());
    let error = RwSignal::new(None::<String>);
    let reload = RwSignal::new(0_u64);
    let navigate = use_navigate();
    let status = use_status_controller();
    #[cfg(target_arch = "wasm32")]
    let writes = StoredValue::new(crate::writes::use_pending_writes());

    let app_links = crate::app_links::use_app_links();
    use_action_handler("copy-carrel-link", move || {
        if let Some(row) = displayed.get_untracked().get(cursor.get_untracked())
            && let Ok(link) = carrel_core::app_link::AppLink::highlight(&row.highlight.id)
        {
            app_links.copy(link);
        }
    });
    use_action_handler("remember-list-context", move || {
        let current = displayed.get_untracked();
        let index = cursor.get_untracked();
        list_navigation.remember(
            RETURN_PATH.into(),
            ListSnapshot {
                selected_id: current.get(index).map(|row| row.highlight.id.clone()),
                selected_index: index,
                scroll_y: list_scroll_y(),
                annotation_cursor: next_cursor.get_untracked(),
                annotations_complete: complete.get_untracked(),
                expanded_highlights: expanded.get_untracked(),
                highlights: current,
                ..Default::default()
            },
        );
    });
    use_keymap_layer(default::list_layer());

    let restore = RwSignal::new(snapshot);
    let previous_ids = RwSignal::new(Vec::<String>::new());
    Effect::new(move |_| {
        if !loaded.get() {
            return;
        }
        let current = displayed.get();
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
        move_cursor(displayed, cursor, CursorMove::Next);
    });
    use_action_handler("prev-item", move || {
        move_cursor(displayed, cursor, CursorMove::Previous);
    });
    use_action_handler("next-unread-item", move || {
        move_cursor(displayed, cursor, CursorMove::Next);
    });
    use_action_handler("prev-unread-item", move || {
        move_cursor(displayed, cursor, CursorMove::Previous);
    });
    use_action_handler("top-of-list", move || {
        cursor.set(0);
        ensure_list_cursor_visible(RETURN_PATH.into());
    });
    use_action_handler("end-of-list", move || {
        let len = displayed.with_untracked(Vec::len);
        if len > 0 {
            cursor.set(len - 1);
            ensure_list_cursor_visible(RETURN_PATH.into());
        }
    });

    let open_navigate = navigate.clone();
    use_action_handler("open-item", move || {
        open_highlight_at(
            displayed,
            cursor,
            list_navigation,
            open_navigate.clone(),
            status,
            cursor.get_untracked(),
            (
                next_cursor.get_untracked(),
                complete.get_untracked(),
                expanded.get_untracked(),
            ),
        );
    });

    let row_navigate = navigate.clone();
    let open_row = UnsyncCallback::new(move |index| {
        open_highlight_at(
            displayed,
            cursor,
            list_navigation,
            row_navigate.clone(),
            status,
            index,
            (
                next_cursor.get_untracked(),
                complete.get_untracked(),
                expanded.get_untracked(),
            ),
        );
    });

    use_action_handler("go-back", move || {
        navigate("/today", NavigateOptions::default());
    });

    let start_edit = UnsyncCallback::new(move |(row, note): (HighlightListRow, NoteDetail)| {
        if actions.pending(&note.id) {
            return;
        }
        edit_error.set(None);
        conflicted.set(false);
        editor_epoch.update(|epoch| *epoch += 1);
        refreshing_note.set(false);
        edit_body.set(note.body.clone());
        editor.set(Some(EditDraft {
            quote: row.highlight.quoted_text,
            note,
            latest_body: None,
        }));
    });
    let start_remove = UnsyncCallback::new(move |row: HighlightListRow| removal.set(Some(row)));
    let save_note = UnsyncCallback::new(move |body: String| {
        let Some(draft) = editor.get_untracked() else {
            return;
        };
        if actions.pending(&draft.note.id) || conflicted.get_untracked() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let guard = writes.get_value().begin();
            let note = draft.note.clone();
            actions.edit(note.clone(), body.clone());
            edit_error.set(None);
            wasm_bindgen_futures::spawn_local(async move {
                let result = if note.conflicts.is_empty() {
                    api::edit_highlight_note(api::EditHighlightNoteRequest {
                        note_id: note.id.clone(),
                        body,
                        expected_revision_id: note.revision_id.clone(),
                    })
                    .await
                } else {
                    let mut expected_revision_ids = note
                        .conflicts
                        .iter()
                        .map(|version| version.revision_id.clone())
                        .collect::<Vec<_>>();
                    expected_revision_ids.sort();
                    api::resolve_highlight_note(api::ResolveHighlightNoteRequest {
                        note_id: note.id.clone(),
                        body,
                        expected_revision_ids,
                    })
                    .await
                };
                drop(guard);
                match result {
                    Ok(saved) => {
                        actions.saved(saved);
                        if !editor.is_disposed() {
                            editor.set(None);
                            reload.update(|token| *token += 1);
                        }
                        status.info("Note saved.");
                    }
                    Err(error) => {
                        actions.edit_failed(note);
                        if !editor.is_disposed() {
                            conflicted.set(error.is_annotation_conflict());
                            edit_error.set(Some(if error.is_annotation_conflict() { "Another version arrived while you were editing. Review the latest versions before saving; your draft will stay here.".into() } else { format!("Could not save this note. Your draft is still here. {}", error.message()) }));
                            reload.update(|token| *token += 1);
                        }
                        status.warn("Could not save note. Your draft is still in the editor.");
                    }
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (body, draft);
    });
    let refresh_note = UnsyncCallback::new(move |()| {
        let Some(draft) = editor.get_untracked() else {
            return;
        };
        if refreshing_note.get_untracked() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let epoch = editor_epoch.get_untracked();
            refreshing_note.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::review_highlight_note(draft.note.id.clone()).await;
                if editor.is_disposed()
                    || editor_epoch.get_untracked() != epoch
                    || editor.get_untracked().is_none()
                {
                    return;
                }
                refreshing_note.set(false);
                match result {
                    Ok(note) => {
                        let latest_body = note.conflicts.is_empty().then(|| note.body.clone());
                        editor.set(Some(EditDraft {
                            quote: draft.quote,
                            note,
                            latest_body,
                        }));
                        edit_error.set(None);
                        conflicted.set(false);
                        reload.update(|token| *token += 1);
                    }
                    Err(error) => edit_error.set(Some(format!(
                        "Could not load the latest versions. Your draft is still here. {}",
                        error.message()
                    ))),
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = draft;
    });
    let confirm_remove = UnsyncCallback::new(move |()| {
        let Some(row) = removal.get_untracked() else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        {
            let guard = writes.get_value().begin();
            let id = row.highlight.id.clone();
            let original_ids = displayed
                .get_untracked()
                .into_iter()
                .map(|row| row.highlight.id)
                .collect::<Vec<_>>();
            let remaining_ids = original_ids
                .iter()
                .filter(|existing| *existing != &id)
                .cloned()
                .collect::<Vec<_>>();
            let neighbour = remaining_ids
                .get(restored_index(
                    &remaining_ids,
                    Some(&id),
                    cursor.get_untracked(),
                    &original_ids,
                ))
                .cloned();
            actions.remove(id.clone());
            removal.set(None);
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::remove_highlight(id.clone()).await;
                drop(guard);
                match result {
                    Ok(()) => {
                        actions.removed(&id);
                        status.info("Highlight removed.");
                        if !reload.is_disposed() {
                            reload.update(|token| *token += 1);
                        }
                    }
                    Err(error) => {
                        let return_selection = !cursor.is_disposed()
                            && displayed.with_untracked(|rows| {
                                rows.get(cursor.get_untracked())
                                    .map(|row| row.highlight.id.clone())
                            }) == neighbour;
                        actions.removal_failed(&id);
                        if return_selection {
                            let ids = displayed
                                .get_untracked()
                                .into_iter()
                                .map(|row| row.highlight.id)
                                .collect::<Vec<_>>();
                            if let Some(index) = ids.iter().position(|existing| existing == &id) {
                                previous_ids.set(ids);
                                cursor.set(index);
                                restore_list_scroll(RETURN_PATH.into(), list_scroll_y(), true);
                            }
                        }
                        status.warn(format!("Could not remove highlight: {}", error.message()));
                    }
                }
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = row;
    });
    let export = UnsyncCallback::new(move |()| {
        #[cfg(target_arch = "wasm32")]
        {
            if file_busy.get_untracked() || actions.has_pending() {
                return;
            }
            file_busy.set(true);
            let writes = writes.get_value();
            let guard = writes.begin();
            wasm_bindgen_futures::spawn_local(async move {
                let _guard = guard;
                match api::export_annotations().await {
                    Ok(true) => status.info("Highlights and notes exported."),
                    Ok(false) => {}
                    Err(error) => {
                        status.warn(format!("Could not export annotations: {}", error.message()))
                    }
                }
                if !file_busy.is_disposed() {
                    file_busy.set(false);
                }
            });
        }
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;
        let requests = Requests::default();
        let cleanup = requests.clone();
        on_cleanup(move || cleanup.invalidate());
        let feeds = crate::feeds::use_feed_controller();
        let previous_more = StoredValue::new(0_u64);
        Effect::new(move |_| {
            let more = load_more.get();
            let append = more != previous_more.get_value();
            previous_more.set_value(more);
            let _ = reload.get();
            let _ = feeds.refresh_token().get();
            let generation = requests.begin();
            let guard = requests.clone();
            let boundary = next_cursor.get_untracked();
            let to_end = complete.get_untracked();
            paging.set(true);
            spawn_local(async move {
                let result = fetch_highlights(
                    if append { boundary.clone() } else { None },
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
                        let focus = crate::navigation::list_has_focus();
                        let scroll_y = list_scroll_y();
                        next_cursor.set(page.next_cursor.clone());
                        complete.set(page.next_cursor.is_none());
                        let mut highlights = if append {
                            rows.get_untracked()
                        } else {
                            Vec::new()
                        };
                        for row in page.items {
                            if let Some(existing) = highlights
                                .iter_mut()
                                .find(|existing| existing.highlight.id == row.highlight.id)
                            {
                                *existing = row;
                            } else {
                                highlights.push(row);
                            }
                        }
                        rows.set(highlights);
                        if focus && !append {
                            restore_list_scroll(RETURN_PATH.into(), scroll_y, true);
                        }
                        error.set(None);
                        retry_older.set(false);
                    }
                    Ok(None) => return,
                    Err(err) => {
                        let message = err.message().to_string();
                        status.warn(format!("Could not load highlights: {message}"));
                        error.set(Some(message));
                        retry_older.set(append);
                    }
                }
                loaded.set(true);
                paging.set(false);
            });
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        loaded.set(true);
        let _ = (HIGHLIGHTS_LIMIT, file_busy, export);
    }

    view! {
        <section class="route-list highlights-route" data-list-path=RETURN_PATH>
            <header class="route-header route-list-header">
                <div>
                    <h1>"Highlights"</h1>
                    <p>"Passages you marked for return."</p>
                </div>
                <button type="button" disabled=move || file_busy.get() || actions.has_pending() on:click=move |_| export.run(())>"Export Markdown"</button>
            </header>
            {move || error.get().map(|message| view! { <div class="quiet-state" role="status"><p>{message}</p><button type="button" disabled=move || paging.get() on:click=move |_| { if retry_older.get_untracked() {load_more.update(|token| *token += 1);} else {reload.update(|token| *token += 1);} }>"Try again"</button></div> })}
            <Show
                when=move || loaded.get()
                fallback=|| view! { <div class="quiet-state" aria-hidden="true"></div> }
            >
                <Show when=move || !displayed.get().is_empty() fallback=move || view! {
                    <Show when=move || error.get().is_none()><p class="quiet-state">"No highlights yet. Mark a passage in the reader and it will appear here."</p></Show>
                }>
                        <HighlightList
                            rows=displayed
                            selected_index=cursor
                            on_open=open_row
                            on_select=UnsyncCallback::new(move |index| cursor.set(index))
                            on_edit=start_edit on_remove=start_remove
                            expanded=expanded
                            on_toggle_notes=UnsyncCallback::new(move |id: String| expanded.update(|expanded| {if !expanded.remove(&id) {expanded.insert(id);}}))
                        />
                </Show>
            </Show>
            <Show when=move || loaded.get() && next_cursor.get().is_some()>
                <div class="archive-footer"><button type="button" disabled=move || paging.get() on:click=move |_| load_more.update(|token| *token += 1)>"Load older"</button></div>
            </Show>
            {move || editor.get().map(|draft| {
                let id = draft.note.id.clone();
                view! { <EditNoteOverlay draft=draft body=edit_body error=edit_error conflicted=conflicted pending=Signal::derive(move || actions.pending(&id))
                    on_save=save_note on_cancel=UnsyncCallback::new(move |()| {editor_epoch.update(|epoch| *epoch += 1); refreshing_note.set(false); editor.set(None);})
                    on_reload=refresh_note reload_pending=refreshing_note /> }
            })}
            {move || removal.get().map(|row| view! {
                <Modal label="Remove highlight" class="note-dialog" on_cancel=UnsyncCallback::new(move |()| removal.set(None))>
                    <section><h2>"Remove highlight?"</h2><blockquote>{row.highlight.quoted_text}</blockquote>
                    <p>"This highlight and its attached notes will disappear from review and exports."</p>
                    <div class="note-dialog-actions"><button type="button" on:click=move |_| removal.set(None)>"Cancel"</button><button type="button" on:click=move |_| confirm_remove.run(())>"Remove highlight"</button></div></section>
                </Modal>
            })}
        </section>
    }
}

#[component]
fn HighlightList(
    #[prop(into)] rows: Signal<Vec<HighlightListRow>>,
    #[prop(into)] selected_index: Signal<usize>,
    on_open: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
    on_edit: UnsyncCallback<(HighlightListRow, NoteDetail)>,
    on_remove: UnsyncCallback<HighlightListRow>,
    #[prop(into)] expanded: Signal<BTreeSet<String>>,
    on_toggle_notes: UnsyncCallback<String>,
) -> impl IntoView {
    view! {
        <ul class="highlight-list" aria-label="Highlights">
            <For
                each=move || { rows.get().into_iter().enumerate().collect::<Vec<_>>() }
                key=|(_, row)| row.highlight.id.clone()
                let:row
            >
                {
                    let (_, row) = row;
                    let id = row.highlight.id.clone();
                    let expanded_id = id.clone();
                    let index = Signal::derive(move || rows.with(|rows| rows.iter().position(|row| row.highlight.id == id).unwrap_or(0)));
                    let row = highlight_row_data(rows, index, row);
                    view! {
                        <HighlightRow
                            index=index
                            row=row
                            selected=Signal::derive(move || index.get() == selected_index.get())
                            on_open=on_open
                            on_select=on_select
                            on_edit=on_edit on_remove=on_remove
                            expanded=Signal::derive(move || expanded.with(|expanded| expanded.contains(&expanded_id)))
                            on_toggle_notes=on_toggle_notes
                        />
                    }
                }
            </For>
        </ul>
    }
}

fn highlight_row_data(
    rows: Signal<Vec<HighlightListRow>>,
    index: Signal<usize>,
    fallback: HighlightListRow,
) -> Memo<HighlightListRow> {
    Memo::new(move |_| {
        rows.with(|rows| {
            rows.get(index.get())
                .filter(|row| row.highlight.id == fallback.highlight.id)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        })
    })
}

#[component]
fn HighlightRow(
    index: Signal<usize>,
    #[prop(into)] row: Signal<HighlightListRow>,
    selected: Signal<bool>,
    on_open: UnsyncCallback<usize>,
    on_select: UnsyncCallback<usize>,
    on_edit: UnsyncCallback<(HighlightListRow, NoteDetail)>,
    on_remove: UnsyncCallback<HighlightListRow>,
    expanded: Signal<bool>,
    on_toggle_notes: UnsyncCallback<String>,
) -> impl IntoView {
    let actions = crate::annotation_actions::use_annotation_actions();
    view! {
        <li class="highlight-row" class:is-selected=move || selected.get() on:focusin=move |_| on_select.run(index.get_untracked())
            on:click=move |_| { if should_open_from_row_click() { on_open.run(index.get_untracked()); } }>
            <div class="highlight-row-body">
                <p class="highlight-source">
                    <span>{move || row.get().item_title}</span><span>{move || row.get().item_source_name}</span>
                    <span>{move || created_date_label(&row.get().highlight.created_at)}</span>
                </p>
                <blockquote>{move || row.get().highlight.quoted_text}</blockquote>
                <Show when=move || !expanded.get()>{move || row.get().note_excerpt.map(|note| view! { <p class="highlight-note">{note}</p> })}</Show>
                <Show when=move || expanded.get()>
                    <div class="highlight-notes" on:click=move |event| event.stop_propagation()>
                        <For each=move || row.get().notes key=|note| note.id.clone() let:note>
                            {let id = note.id.clone(); let edit_id = id.clone(); let pending_id = StoredValue::new(id.clone()); let click_id = StoredValue::new(id.clone()); let label_id = StoredValue::new(id.clone());
                            view! { <div class="highlight-note-full"><p>{move || row.get().notes.into_iter().find(|note| note.id == id).map(|note| note.body).unwrap_or_default()}</p>
                                <Show when=move || row.get().notes.iter().any(|note| note.id == edit_id && (note.can_edit || !note.conflicts.is_empty()))>
                                    <button type="button" disabled=move || pending_id.with_value(|id| actions.pending(id)) on:click=move |_| {
                                        let current = row.get_untracked(); if let Some(note) = current.notes.iter().find(|note| click_id.with_value(|id| &note.id == id)) {on_edit.run((current.clone(), note.clone()));}
                                    }>{move || if row.get().notes.iter().any(|note| label_id.with_value(|id| &note.id == id) && !note.conflicts.is_empty()) {"Review conflict"} else {"Edit note"}}</button>
                                </Show></div> }}
                        </For>
                    </div>
                </Show>
                <div class="highlight-row-footer">
                    <div class="highlight-row-actions">
                    <crate::app_links::CopyLinkButton link=Signal::derive(move || carrel_core::app_link::AppLink::highlight(&row.get().highlight.id).ok())/>
                    <Show when=move || !expanded.get() && row.get().notes.iter().any(|note| !note.conflicts.is_empty())>
                        <button type="button" on:click=move |event: ev::MouseEvent| {
                            event.stop_propagation();
                            let current = row.get_untracked();
                            if let Some(note) = current.notes.iter().find(|note| !note.conflicts.is_empty()) {on_edit.run((current.clone(), note.clone()));}
                        }>"Review conflict"</button>
                    </Show>
                    <Show when=move || !row.get().notes.is_empty()><button type="button" aria-expanded=move || expanded.get().to_string() on:click=move |event: ev::MouseEvent| {event.stop_propagation(); on_toggle_notes.run(row.get_untracked().highlight.id);}>{move || if expanded.get() {"Hide notes"} else {"Notes"}}</button></Show>
                    <button type="button" class="highlight-open" on:click=move |event: ev::MouseEvent| { event.stop_propagation(); on_open.run(index.get_untracked()); }>"Open source"</button>
                    <Show when=move || row.get().can_remove><button type="button" on:click=move |event: ev::MouseEvent| {event.stop_propagation(); on_remove.run(row.get_untracked());}>"Remove"</button></Show>
                    </div>
                </div>
            </div>
        </li>
    }
}

#[derive(Clone, Debug)]
struct EditDraft {
    quote: String,
    note: NoteDetail,
    latest_body: Option<String>,
}

#[component]
fn EditNoteOverlay(
    draft: EditDraft,
    body: RwSignal<String>,
    error: RwSignal<Option<String>>,
    conflicted: RwSignal<bool>,
    pending: Signal<bool>,
    on_save: UnsyncCallback<String>,
    on_cancel: UnsyncCallback<()>,
    on_reload: UnsyncCallback<()>,
    reload_pending: RwSignal<bool>,
) -> impl IntoView {
    let resolution = !draft.note.conflicts.is_empty();
    let can_edit = draft.note.can_edit;
    let show_draft = can_edit || body.get_untracked() != draft.note.body;
    let label = if resolution {
        "Review note conflict"
    } else {
        "Edit note"
    };
    let versions = draft.note.conflicts;
    let textarea = NodeRef::<leptos::html::Textarea>::new();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(textarea) = textarea.get() {
            let _ = textarea.focus();
        }
    });
    let submit = move || {
        if !pending.get_untracked()
            && !conflicted.get_untracked()
            && can_edit
            && !body.with_untracked(|body| body.trim().is_empty())
        {
            on_save.run(body.get_untracked());
        }
    };
    let cancel = UnsyncCallback::new(move |()| {
        if !pending.get_untracked() {
            on_cancel.run(());
        }
    });
    view! {
        <Modal label=label class="note-dialog" on_cancel=cancel>
            <section on:keydown=move |event: ev::KeyboardEvent| {
                if event.key() == "Enter" && (event.meta_key() || event.ctrl_key()) {event.prevent_default(); event.stop_propagation(); submit();}
            }>
                <h2>{label}</h2><blockquote>{draft.quote}</blockquote>
                {resolution.then(|| view! {<p>{if can_edit {"These versions were edited independently. Keep one or combine them below."} else {"You can review these versions here. This device cannot change the note."}}</p>})}
                <div class="note-conflict-versions">
                    {versions.into_iter().map(|version| {
                        let saved_body = StoredValue::new(version.body.clone());
                        view! { <section class="note-conflict-version">
                            <p class="note-version-source">{version.device_label}" · "{version.saved_at}</p>
                            <p class="note-version-body">{version.body}</p>
                            <Show when=move || can_edit><button type="button" disabled=move || pending.get() on:click=move |_| body.set(saved_body.get_value())>"Use this version"</button></Show>
                        </section> }
                    }).collect_view()}
                </div>
                {draft.latest_body.map(|latest| view! {<section class="note-conflict-version"><p class="note-version-source">"Latest saved note"</p><p class="note-version-body">{latest}</p></section>})}
                <Show when=move || show_draft>
                    <label for="note-edit-draft" class="note-version-source note-draft-label">"Your draft"</label>
                    <textarea id="note-edit-draft" rows="8" aria-label="Note" node_ref=textarea autofocus=true readonly=move || pending.get() || !can_edit prop:value=move || body.get()
                        on:input=move |event| body.set(event_target_value(&event))></textarea>
                </Show>
                {move || error.get().map(|message| view! {<p class="annotation-error" role="alert">{message}</p>})}
                <Show when=move || conflicted.get()><button type="button" disabled=move || reload_pending.get() on:click=move |_| on_reload.run(())>"Review latest versions"</button></Show>
                <div class="note-dialog-actions"><button type="button" disabled=move || pending.get() on:click=move |_| cancel.run(())>"Cancel"</button>
                    <Show when=move || can_edit><button type="button" disabled=move || pending.get() || conflicted.get() || body.with(|body| body.trim().is_empty()) on:click=move |_| submit()>{if resolution {"Save resolution"} else {"Save"}}</button></Show></div>
            </section>
        </Modal>
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_highlights(
    after: Option<AnnotationCursor>,
    boundary: Option<AnnotationCursor>,
    to_end: bool,
    guard: &Requests,
    generation: u64,
) -> Result<Option<api::AnnotationPage<HighlightListRow>>, api::ApiError> {
    let mut after = after;
    let mut items = Vec::new();
    loop {
        let page = api::list_highlights(Some(HIGHLIGHTS_LIMIT), after).await?;
        if !guard.accepts(generation) {
            return Ok(None);
        }
        let reached = boundary.as_ref().is_some_and(|boundary| {
            page.items.iter().any(|row| {
                row.highlight.created_at_micros < boundary.created_at_micros
                    || (row.highlight.created_at_micros == boundary.created_at_micros
                        && row.highlight.id >= boundary.id)
            })
        });
        items.extend(page.items);
        if page.next_cursor.is_none() || (!to_end && (boundary.is_none() || reached)) {
            return Ok(Some(api::AnnotationPage {
                items,
                next_cursor: page.next_cursor,
            }));
        }
        after = page.next_cursor;
    }
}

fn open_highlight_at(
    rows: Signal<Vec<HighlightListRow>>,
    cursor: RwSignal<usize>,
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions),
    status: StatusController,
    index: usize,
    archive: (Option<AnnotationCursor>, bool, BTreeSet<String>),
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
            annotation_cursor: archive.0,
            annotations_complete: archive.1,
            expanded_highlights: archive.2,
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
    rows: Signal<Vec<HighlightListRow>>,
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
                    on_edit=UnsyncCallback::new(|_| {}) on_remove=UnsyncCallback::new(|_| {})
                    expanded=Signal::stored(BTreeSet::new()) on_toggle_notes=UnsyncCallback::new(|_| {})
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
                    on_edit=UnsyncCallback::new(|_| {}) on_remove=UnsyncCallback::new(|_| {})
                    expanded=Signal::stored(BTreeSet::new()) on_toggle_notes=UnsyncCallback::new(|_| {})
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
            let row = highlight_row_data(rows.into(), Signal::stored(0), initial);
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
            notes: Vec::new(),
            can_remove: true,
        }
    }
}
