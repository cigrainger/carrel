//! Reading route.

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

use carrel_core::keymap::default;
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_navigate, use_params_map};

#[cfg(target_arch = "wasm32")]
use crate::api::{self, CreateHighlightNoteRequest, ItemStateRequest, ReadProgressUpdate};
use crate::api::{CreateHighlightRequest, HighlightDetail, ItemDetail};
#[cfg(target_arch = "wasm32")]
use crate::highlights;
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::{ListNavigation, use_list_navigation_context};
use crate::status::{StatusController, use_status_controller};

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingNoteDraft {
    request: CreateHighlightRequest,
    exact: String,
}

/// Full-page article reader.
#[component]
pub fn ReadingView() -> impl IntoView {
    let params = use_params_map();
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let item_id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let item = RwSignal::new(None::<ItemDetail>);
    let highlights = RwSignal::new(Vec::<HighlightDetail>::new());
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    let info_open = RwSignal::new(false);
    let pending_note = RwSignal::new(None::<PendingNoteDraft>);
    let restored_item = RwSignal::new(None::<String>);
    let scroll_ref = NodeRef::<html::Div>::new();
    let body_ref = NodeRef::<html::Div>::new();
    let navigate = use_navigate();
    let list_navigation = use_list_navigation_context();
    let status = use_status_controller();

    use_keymap_layer(default::reading_layer());
    use_keymap_layer(default::item_action_layer());
    use_keymap_layer(default::reading_extra_layer());

    let page_down_ref = scroll_ref;
    use_action_handler("page-down", move || {
        scroll_by_pages(page_down_ref, 0.85);
    });

    let page_up_ref = scroll_ref;
    use_action_handler("page-up", move || {
        scroll_by_pages(page_up_ref, -0.85);
    });

    let top_ref = scroll_ref;
    use_action_handler("article-top", move || {
        scroll_to(top_ref, 0.0);
    });

    let bottom_ref = scroll_ref;
    use_action_handler("article-bottom", move || {
        scroll_to_bottom(bottom_ref);
    });

    let back_navigate = navigate.clone();
    use_action_handler("go-back", move || {
        if info_open.get_untracked() {
            info_open.set(false);
        } else {
            navigate_back(list_navigation, back_navigate.clone());
        }
    });

    let next_navigate = navigate.clone();
    use_action_handler("next-item-mark-read", move || {
        navigate_adjacent(
            item,
            list_navigation,
            next_navigate.clone(),
            Direction::Next,
            status,
        );
    });

    let previous_navigate = navigate.clone();
    use_action_handler("prev-item-mark-read", move || {
        navigate_adjacent(
            item,
            list_navigation,
            previous_navigate.clone(),
            Direction::Previous,
            status,
        );
    });

    use_action_handler("mark-read", move || {
        mark_current_read(item, status);
    });

    use_action_handler("mark-unread", move || {
        mark_current_unread(item, status);
    });

    use_action_handler("toggle-star", move || {
        toggle_current_star(item, status);
    });

    use_action_handler("show-item-info", move || {
        info_open.update(|open| *open = !*open);
    });

    let highlight_body_ref = body_ref;
    use_action_handler("highlight-selection", move || {
        create_highlight_from_selection(item, highlight_body_ref, highlights, status);
    });

    let note_body_ref = body_ref;
    use_action_handler("highlight-with-note", move || {
        begin_highlight_note(item, note_body_ref, pending_note, status);
    });

    use_action_handler("archive-item", move || {
        status.info("Archive is not available yet.");
    });

    let restore_ref = scroll_ref;
    Effect::new(move |_| {
        let Some(detail) = item.get() else {
            return;
        };
        if restored_item.get_untracked().as_deref() == Some(detail.id.as_str()) {
            return;
        }

        restored_item.set(Some(detail.id));
        restore_scroll(restore_ref, detail.last_scroll);
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        Effect::new(move |_| {
            let id = item_id.get();
            item.set(None);
            highlights.set(Vec::new());
            error.set(None);
            loaded.set(false);
            restored_item.set(None);
            info_open.set(false);
            pending_note.set(None);

            spawn_local(async move {
                match api::get_item(id.clone()).await {
                    Ok(detail) => {
                        if detail.is_some() {
                            match api::list_item_highlights(id).await {
                                Ok(rows) => highlights.set(rows),
                                Err(err) => {
                                    status.warn(format!(
                                        "Could not load highlights: {}",
                                        err.message()
                                    ));
                                }
                            }
                        }
                        item.set(detail);
                    }
                    Err(err) => {
                        error.set(Some(err.message().to_string()));
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
        <section
            class="reading-route"
            aria-label="Reading view"
        >
            {
                let back_click = {
                    let navigate = navigate.clone();
                    UnsyncCallback::new(move |()| navigate_back(list_navigation, navigate.clone()))
                };
                let previous_click = {
                    let navigate = navigate.clone();
                    UnsyncCallback::new(move |()| {
                        navigate_adjacent(
                            item,
                            list_navigation,
                            navigate.clone(),
                            Direction::Previous,
                            status,
                        );
                    })
                };
                let next_click = {
                    let navigate = navigate.clone();
                    UnsyncCallback::new(move |()| {
                        navigate_adjacent(
                            item,
                            list_navigation,
                            navigate.clone(),
                            Direction::Next,
                            status,
                        );
                    })
                };
                let star_click = UnsyncCallback::new(move |()| toggle_current_star(item, status));
                let read_click = UnsyncCallback::new(move |()| toggle_current_read_state(item, status));
                let info_click = UnsyncCallback::new(move |()| info_open.update(|open| *open = !*open));

                view! {
                    {move || view! {
                        <ReaderActions
                            item=item.get()
                            can_previous=can_navigate_adjacent(item.get(), list_navigation.get(), Direction::Previous)
                            can_next=can_navigate_adjacent(item.get(), list_navigation.get(), Direction::Next)
                            info_open=info_open.get()
                            on_back=back_click
                            on_previous=previous_click
                            on_next=next_click
                            on_toggle_star=star_click
                            on_toggle_read=read_click
                            on_toggle_info=info_click
                        />
                    }}
                }
            }
            {move || {
                pending_note
                    .get()
                    .map(|draft| {
                        let request = draft.request.clone();
                        let exact = draft.exact;
                        let confirm = UnsyncCallback::new(move |body: String| {
                            confirm_highlight_note(
                                request.clone(),
                                body,
                                highlights,
                                pending_note,
                                status,
                            );
                        });
                        let cancel = UnsyncCallback::new(move |()| {
                            pending_note.set(None);
                            status.info("Highlight note cancelled.");
                        });
                        view! {
                            <HighlightNoteOverlay
                                exact=exact
                                on_confirm=confirm
                                on_cancel=cancel
                            />
                        }.into_any()
                    })
            }}
            <div
                class="reader-scroll"
                node_ref=scroll_ref
                on:scroll=move |_| persist_progress(item, scroll_ref)
            >
                <Show
                    when=move || loaded.get()
                    fallback=|| view! { <div class="reading-empty" aria-hidden="true"></div> }
                >
                    {move || match error.get() {
                        Some(message) => view! {
                            <p class="quiet-state reader-message">{message}</p>
                        }.into_any(),
                        None => item.get()
                            .map(|detail| {
                                let info_detail = detail.clone();
                                let article_highlights = highlights.get();
                                let close_info = UnsyncCallback::new(move |()| info_open.set(false));
                                view! {
                                    <Show when=move || info_open.get()>
                                        <ItemInfo item=info_detail.clone() on_close=close_info/>
                                    </Show>
                                    <ArticleContent
                                        item=detail
                                        highlights=article_highlights
                                        body_ref=body_ref
                                        status=status
                                    />
                                }.into_any()
                            })
                            .unwrap_or_else(|| view! {
                                <p class="quiet-state reader-message">"Item not found."</p>
                            }.into_any()),
                    }}
                </Show>
            </div>
        </section>
    }
}

/// Render persistent reader controls.
#[component]
pub fn ReaderActions(
    item: Option<ItemDetail>,
    can_previous: bool,
    can_next: bool,
    info_open: bool,
    on_back: UnsyncCallback<()>,
    on_previous: UnsyncCallback<()>,
    on_next: UnsyncCallback<()>,
    on_toggle_star: UnsyncCallback<()>,
    on_toggle_read: UnsyncCallback<()>,
    on_toggle_info: UnsyncCallback<()>,
) -> impl IntoView {
    let loaded = item.is_some();
    let original_url = item.as_ref().and_then(|detail| detail.primary_url.clone());
    let star_label = item
        .as_ref()
        .map(|detail| star_action_label(detail.starred))
        .unwrap_or("Star");
    let read_label = item
        .as_ref()
        .map(|detail| read_action_label(&detail.read_state))
        .unwrap_or("Mark read");
    let info_label = if info_open { "Hide info" } else { "Item info" };

    view! {
        <nav class="reader-actions" aria-label="Reader controls">
            <div class="reader-action-group">
                <button
                    type="button"
                    class="reader-action-primary"
                    on:click=move |_| on_back.run(())
                >
                    "Back"
                </button>
                <button
                    type="button"
                    disabled=!can_previous
                    aria-label="Previous item"
                    on:click=move |_| on_previous.run(())
                >
                    "Previous"
                </button>
                <button
                    type="button"
                    disabled=!can_next
                    aria-label="Next item"
                    on:click=move |_| on_next.run(())
                >
                    "Next"
                </button>
            </div>
            <div class="reader-action-group">
                <button
                    type="button"
                    disabled=!loaded
                    aria-pressed=move || item.as_ref().is_some_and(|detail| detail.starred).to_string()
                    on:click=move |_| on_toggle_star.run(())
                >
                    {star_label}
                </button>
                <button
                    type="button"
                    disabled=!loaded
                    on:click=move |_| on_toggle_read.run(())
                >
                    {read_label}
                </button>
                <button
                    type="button"
                    disabled=!loaded
                    aria-pressed=info_open.to_string()
                    on:click=move |_| on_toggle_info.run(())
                >
                    {info_label}
                </button>
            </div>
            <div class="reader-action-group">
                {original_url
                    .map(|url| view! {
                        <a class="reader-action-link" href=url target="_blank" rel="noreferrer">
                            "Open original"
                        </a>
                    }.into_any())
                    .unwrap_or_else(|| view! {
                        <button type="button" disabled=true>"Open original"</button>
                    }.into_any())}
            </div>
        </nav>
    }
}

/// User-initiated item metadata surface.
#[component]
pub fn ItemInfo(item: ItemDetail, on_close: UnsyncCallback<()>) -> impl IntoView {
    let url = item
        .primary_url
        .clone()
        .unwrap_or_else(|| "No canonical URL".to_string());
    let feed_url = item
        .feed_url
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());
    let published = item
        .published_at
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());
    let minutes = item
        .estimated_read_minutes
        .map(|minutes| format!("{minutes} min"))
        .unwrap_or_else(|| "Unknown".to_string());
    let words = item
        .word_count
        .map(|words| format!("{words} words"))
        .unwrap_or_else(|| "Unknown".to_string());
    let cache_state = cache_state_label(&item).to_string();
    let shape_tags = item.shape_tags.clone();
    let has_shape_tags = !shape_tags.is_empty();

    view! {
        <aside class="reader-info" aria-label="Item info">
            <header>
                <h2>"Item info"</h2>
                <button type="button" aria-label="Close item info" on:click=move |_| on_close.run(())>
                    "Close"
                </button>
            </header>
            <dl>
                <div>
                    <dt>"Source"</dt>
                    <dd>{item.source_name}</dd>
                </div>
                <div>
                    <dt>"Feed"</dt>
                    <dd class="reader-info-url">{feed_url}</dd>
                </div>
                <div>
                    <dt>"Canonical URL"</dt>
                    <dd class="reader-info-url">{url}</dd>
                </div>
                <div>
                    <dt>"Cache"</dt>
                    <dd>{cache_state}</dd>
                </div>
                <div>
                    <dt>"Discovered"</dt>
                    <dd>{item.discovered_at}</dd>
                </div>
                <div>
                    <dt>"Published"</dt>
                    <dd>{published}</dd>
                </div>
                <div>
                    <dt>"Reading time"</dt>
                    <dd>{minutes}</dd>
                </div>
                <div>
                    <dt>"Word count"</dt>
                    <dd>{words}</dd>
                </div>
                <Show when=move || has_shape_tags>
                    <div>
                        <dt>"Content shape"</dt>
                        <dd class="reader-info-tags">
                            {shape_tags
                                .iter()
                                .map(|tag| view! { <span>{tag.clone()}</span> })
                                .collect_view()}
                        </dd>
                    </div>
                </Show>
            </dl>
        </aside>
    }
}

/// Small focus-contained note overlay for `H`.
#[component]
pub fn HighlightNoteOverlay(
    exact: String,
    on_confirm: UnsyncCallback<String>,
    on_cancel: UnsyncCallback<()>,
) -> impl IntoView {
    let note = RwSignal::new(String::new());
    let note_ref = NodeRef::<html::Textarea>::new();
    let cancel_ref = NodeRef::<html::Button>::new();
    let save_ref = NodeRef::<html::Button>::new();

    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(textarea) = note_ref.get() {
            let _ = textarea.focus();
        }
    });

    let submit = move || {
        let body = note.get_untracked();
        if !body.trim().is_empty() {
            on_confirm.run(body);
        }
    };

    view! {
        <div class="note-dialog-backdrop">
            <section
                class="note-dialog"
                role="dialog"
                aria-modal="true"
                aria-label="Highlight note"
                on:keydown=move |event| handle_note_dialog_keydown(event, note_ref, cancel_ref, save_ref, submit, on_cancel)
            >
                <h2>"Highlight note"</h2>
                <blockquote>{exact}</blockquote>
                <textarea
                    node_ref=note_ref
                    rows="5"
                    aria-label="Note"
                    prop:value=move || note.get()
                    on:input=move |event| note.set(event_target_value(&event))
                ></textarea>
                <div class="note-dialog-actions">
                    <button
                        type="button"
                        node_ref=cancel_ref
                        on:click=move |_| on_cancel.run(())
                    >
                        "Cancel"
                    </button>
                    <button
                        type="button"
                        node_ref=save_ref
                        disabled=move || note.with(|value| value.trim().is_empty())
                        on:click=move |_| submit()
                    >
                        "Save"
                    </button>
                </div>
            </section>
        </div>
    }
}

/// Render the article payload itself.
#[component]
pub fn ArticleContent(
    item: ItemDetail,
    highlights: Vec<HighlightDetail>,
    body_ref: NodeRef<html::Div>,
    status: StatusController,
) -> impl IntoView {
    let byline = item.byline.clone().or_else(|| {
        if item.creators.is_empty() {
            None
        } else {
            Some(item.creators.join(", "))
        }
    });
    let has_byline = byline.is_some();
    let byline_label = byline.unwrap_or_default();
    let date = item.published_at.clone().unwrap_or(item.time_label.clone());
    let star_label = if item.starred { "Starred" } else { "" };
    let read_label = if item.read_state == "read" {
        "Read"
    } else {
        ""
    };
    let has_original = item.primary_url.is_some();
    let content_html = item.content_html.clone();

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (&highlights, status);

    #[cfg(target_arch = "wasm32")]
    {
        let content_html = content_html.clone();
        let highlights = highlights.clone();
        Effect::new(move |_| {
            let Some(body) = body_ref.get() else {
                return;
            };
            let focus_id = highlights::current_hash_highlight_id();
            let failed = highlights::render_highlights(
                &body,
                &content_html,
                &highlights,
                focus_id.as_deref(),
            );
            if let Some(focus_id) = focus_id
                && failed.iter().any(|id| id == &focus_id)
            {
                status.warn("Could not locate that highlight in the cached article.");
            }
        });
    }

    view! {
        <article class="reader-article" lang=item.language.clone().unwrap_or_else(|| "en".to_string())>
            <header class="reader-header">
                <p class="reader-source">{item.source_name.clone()}</p>
                <h1>{item.title.clone()}</h1>
                <div class="reader-meta">
                    <Show when=move || has_byline>
                        <span>{byline_label.clone()}</span>
                    </Show>
                    <span>{item.length_label.clone()}</span>
                    <span>{date}</span>
                    <Show when=move || !star_label.is_empty()>
                        <span>{star_label}</span>
                    </Show>
                    <Show when=move || !read_label.is_empty()>
                        <span>{read_label}</span>
                    </Show>
                </div>
            </header>
            <div class="reader-body" node_ref=body_ref inner_html=content_html></div>
            <footer class="reader-footer">
                <span class="reader-end">"end of article"</span>
                <Show when=move || has_original>
                    <a
                        href=item.primary_url.clone().unwrap_or_default()
                        target="_blank"
                        rel="noreferrer"
                    >
                        "Open original"
                    </a>
                </Show>
            </footer>
        </article>
    }
}

fn star_action_label(starred: bool) -> &'static str {
    if starred { "Unstar" } else { "Star" }
}

fn read_action_label(read_state: &str) -> &'static str {
    if read_state == "read" {
        "Mark unread"
    } else {
        "Mark read"
    }
}

fn cache_state_label(item: &ItemDetail) -> &'static str {
    if item.readable_blob_id.is_some() {
        "Readable cache ready"
    } else if item.content_html.is_empty() {
        "No readable cache"
    } else {
        "Feed summary fallback"
    }
}

fn create_highlight_from_selection(
    item: RwSignal<Option<ItemDetail>>,
    body_ref: NodeRef<html::Div>,
    highlights: RwSignal<Vec<HighlightDetail>>,
    status: StatusController,
) {
    let Some(request) = capture_highlight_request(item, body_ref, status) else {
        return;
    };
    write_highlight(request, highlights, status);
}

fn begin_highlight_note(
    item: RwSignal<Option<ItemDetail>>,
    body_ref: NodeRef<html::Div>,
    pending_note: RwSignal<Option<PendingNoteDraft>>,
    status: StatusController,
) {
    let Some(request) = capture_highlight_request(item, body_ref, status) else {
        return;
    };
    let exact = match &request.location {
        carrel_core::annotation::HighlightLocation::HtmlText(selector) => selector.exact.clone(),
    };
    pending_note.set(Some(PendingNoteDraft { request, exact }));
}

fn capture_highlight_request(
    item: RwSignal<Option<ItemDetail>>,
    body_ref: NodeRef<html::Div>,
    status: StatusController,
) -> Option<CreateHighlightRequest> {
    let Some(detail) = item.get_untracked() else {
        status.info("Open an item before highlighting.");
        return None;
    };

    #[cfg(target_arch = "wasm32")]
    {
        let Some(body) = body_ref.get_untracked() else {
            status.info("Article text is not ready yet.");
            return None;
        };
        let request = highlights::capture_current_selection(
            &body,
            detail.id,
            detail.readable_blob_id.clone(),
        );
        if request.is_none() {
            status.info("Select text in the article to highlight.");
        }
        request
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (body_ref, detail);
        status.info("Highlight capture is available in the desktop app.");
        None
    }
}

fn write_highlight(
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    request: CreateHighlightRequest,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] highlights: RwSignal<
        Vec<HighlightDetail>,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            match api::create_highlight(request).await {
                Ok(highlight) => {
                    highlights.update(|rows| rows.push(highlight));
                    highlights::clear_current_selection();
                    status.info("Highlighted.");
                }
                Err(error) => {
                    status.warn(format!("Could not save highlight: {}", error.message()));
                }
            }
        });
    }
}

fn confirm_highlight_note(
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    request: CreateHighlightRequest,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] body: String,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] highlights: RwSignal<
        Vec<HighlightDetail>,
    >,
    pending_note: RwSignal<Option<PendingNoteDraft>>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
) {
    if body.trim().is_empty() {
        status.info("Write a note before saving.");
        return;
    }

    pending_note.set(None);

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            match api::create_highlight(request).await {
                Ok(highlight) => {
                    let note_request = CreateHighlightNoteRequest {
                        highlight_id: highlight.id.clone(),
                        body,
                    };
                    match api::create_highlight_note(note_request).await {
                        Ok(_) => {
                            highlights.update(|rows| rows.push(highlight));
                            highlights::clear_current_selection();
                            status.info("Highlight note saved.");
                        }
                        Err(error) => {
                            highlights.update(|rows| rows.push(highlight));
                            highlights::clear_current_selection();
                            status.warn(format!(
                                "Highlight saved, but note failed: {}",
                                error.message()
                            ));
                        }
                    }
                }
                Err(error) => {
                    status.warn(format!("Could not save highlight: {}", error.message()));
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn handle_note_dialog_keydown(
    event: ev::KeyboardEvent,
    note_ref: NodeRef<html::Textarea>,
    cancel_ref: NodeRef<html::Button>,
    save_ref: NodeRef<html::Button>,
    submit: impl Fn() + Copy + 'static,
    on_cancel: UnsyncCallback<()>,
) {
    match event.key().as_str() {
        "Escape" => {
            event.prevent_default();
            event.stop_propagation();
            on_cancel.run(());
        }
        "Enter" if event.meta_key() || event.ctrl_key() => {
            event.prevent_default();
            event.stop_propagation();
            submit();
        }
        "Tab" => trap_note_dialog_focus(&event, note_ref, cancel_ref, save_ref),
        _ => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_note_dialog_keydown(
    _event: ev::KeyboardEvent,
    _note_ref: NodeRef<html::Textarea>,
    _cancel_ref: NodeRef<html::Button>,
    _save_ref: NodeRef<html::Button>,
    _submit: impl Fn() + Copy + 'static,
    _on_cancel: UnsyncCallback<()>,
) {
}

#[cfg(target_arch = "wasm32")]
fn trap_note_dialog_focus(
    event: &web_sys::KeyboardEvent,
    note_ref: NodeRef<html::Textarea>,
    cancel_ref: NodeRef<html::Button>,
    save_ref: NodeRef<html::Button>,
) {
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(active) = document.active_element() else {
        return;
    };

    let Some(note) = note_ref.get_untracked() else {
        return;
    };
    let Some(save) = save_ref.get_untracked() else {
        return;
    };

    if event.shift_key() && active.is_same_node(Some(note.unchecked_ref())) {
        event.prevent_default();
        if let Some(cancel) = cancel_ref.get_untracked() {
            let _ = cancel.focus();
        }
    } else if !event.shift_key() && active.is_same_node(Some(save.unchecked_ref())) {
        event.prevent_default();
        let _ = note.focus();
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Next,
    Previous,
}

fn navigate_back(
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions),
) {
    let return_path = list_navigation
        .get()
        .map(|navigation| navigation.return_path)
        .unwrap_or_else(|| "/today".to_string());
    navigate(&return_path, NavigateOptions::default());
}

fn navigate_adjacent(
    item: RwSignal<Option<ItemDetail>>,
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions) + Clone + 'static,
    direction: Direction,
    status: StatusController,
) {
    let Some(current_id) =
        item.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()))
    else {
        return;
    };
    let Some(next_navigation) = next_list_navigation(list_navigation.get(), &current_id, direction)
    else {
        return;
    };

    mark_current_read(item, status);
    let next_id = next_navigation.current_id.clone();
    list_navigation.set(next_navigation);
    navigate(&format!("/item/{next_id}"), NavigateOptions::default());
}

fn can_navigate_adjacent(
    item: Option<ItemDetail>,
    context: Option<ListNavigation>,
    direction: Direction,
) -> bool {
    let Some(item) = item else {
        return false;
    };
    next_list_navigation(context, &item.id, direction).is_some()
}

fn next_list_navigation(
    context: Option<ListNavigation>,
    current_id: &str,
    direction: Direction,
) -> Option<ListNavigation> {
    let mut context = context?;
    let current = context
        .item_ids
        .iter()
        .position(|item_id| item_id == current_id)?;
    let next = match direction {
        Direction::Next => current.checked_add(1)?,
        Direction::Previous => current.checked_sub(1)?,
    };
    let next_id = context.item_ids.get(next)?.clone();
    context.current_id = next_id;
    Some(context)
}

fn toggle_current_read_state(item: RwSignal<Option<ItemDetail>>, status: StatusController) {
    let read_state = item.with_untracked(|detail| {
        detail
            .as_ref()
            .map(|detail| detail.read_state.clone())
            .unwrap_or_default()
    });
    if read_state == "read" {
        mark_current_unread(item, status);
    } else {
        mark_current_read(item, status);
    }
}

fn mark_current_read(item: RwSignal<Option<ItemDetail>>, status: StatusController) {
    set_current_read_state(item, "read", status);
}

fn mark_current_unread(item: RwSignal<Option<ItemDetail>>, status: StatusController) {
    set_current_read_state(item, "unread", status);
}

fn set_current_read_state(
    item: RwSignal<Option<ItemDetail>>,
    next_state: &'static str,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
) {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let Some((item_id, previous_state)) = item.with_untracked(|detail| {
        detail
            .as_ref()
            .map(|detail| (detail.id.clone(), detail.read_state.clone()))
    }) else {
        return;
    };

    item.update(|detail| {
        if let Some(detail) = detail {
            detail.read_state = next_state.to_string();
        }
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            let result = if next_state == "read" {
                api::mark_item_read(ItemStateRequest {
                    item_id: item_id.clone(),
                })
                .await
            } else {
                api::mark_item_unread(ItemStateRequest {
                    item_id: item_id.clone(),
                })
                .await
            };
            if let Err(error) = result {
                reconcile_read_state(item, &item_id, previous_state);
                status.warn(format!("Could not update read state: {}", error.message()));
            }
        });
    }
}

fn toggle_current_star(
    item: RwSignal<Option<ItemDetail>>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
) {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let Some((item_id, previous_starred)) = item.with_untracked(|detail| {
        detail
            .as_ref()
            .map(|detail| (detail.id.clone(), detail.starred))
    }) else {
        return;
    };

    item.update(|detail| {
        if let Some(detail) = detail {
            detail.starred = !detail.starred;
        }
    });

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        spawn_local(async move {
            match api::toggle_item_star(ItemStateRequest {
                item_id: item_id.clone(),
            })
            .await
            {
                Ok(starred) => reconcile_star(item, &item_id, starred),
                Err(error) => {
                    reconcile_star(item, &item_id, previous_starred);
                    status.warn(format!("Could not update star: {}", error.message()));
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn reconcile_read_state(item: RwSignal<Option<ItemDetail>>, item_id: &str, read_state: String) {
    item.update(|detail| {
        if let Some(detail) = detail.as_mut().filter(|detail| detail.id == item_id) {
            detail.read_state = read_state;
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn reconcile_star(item: RwSignal<Option<ItemDetail>>, item_id: &str, starred: bool) {
    item.update(|detail| {
        if let Some(detail) = detail.as_mut().filter(|detail| detail.id == item_id) {
            detail.starred = starred;
        }
    });
}

fn persist_progress(item: RwSignal<Option<ItemDetail>>, scroll_ref: NodeRef<html::Div>) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        let Some(detail) = item.get_untracked() else {
            return;
        };
        let Some(element) = scroll_ref.get_untracked() else {
            return;
        };
        let max_scroll = (element.scroll_height() - element.client_height()).max(0);
        let scroll_y = f64::from(element.scroll_top().max(0));
        let progress = if max_scroll == 0 {
            1.0
        } else {
            (scroll_y / f64::from(max_scroll)).clamp(0.0, 1.0)
        };

        spawn_local(async move {
            let _ = api::update_read_progress(ReadProgressUpdate {
                item_id: detail.id,
                progress,
                scroll_y,
            })
            .await;
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (item, scroll_ref);
    }
}

fn scroll_by_pages(scroll_ref: NodeRef<html::Div>, pages: f64) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        let delta = f64::from(element.client_height()) * pages;
        scroll_to(scroll_ref, f64::from(element.scroll_top()) + delta);
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, pages);
}

fn scroll_to(scroll_ref: NodeRef<html::Div>, scroll_y: f64) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        let max_scroll = f64::from((element.scroll_height() - element.client_height()).max(0));
        element.set_scroll_top(scroll_y.clamp(0.0, max_scroll).round() as i32);
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, scroll_y);
}

fn scroll_to_bottom(scroll_ref: NodeRef<html::Div>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(element) = scroll_ref.get_untracked() {
        element.set_scroll_top(element.scroll_height());
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = scroll_ref;
}

fn restore_scroll(scroll_ref: NodeRef<html::Div>, scroll_y: Option<f64>) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(scroll_y) = scroll_y else {
            return;
        };
        let _ = set_timeout_with_handle(
            move || {
                scroll_to(scroll_ref, scroll_y);
            },
            Duration::from_millis(0),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (scroll_ref, scroll_y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_content_renders_cached_html() {
        let item = sample_item();

        let html = Owner::new().with(|| {
            let status = crate::status::provide_status_controller();
            let body_ref = NodeRef::<html::Div>::new();
            view! {
                <ArticleContent
                    item=item
                    highlights=Vec::new()
                    body_ref=body_ref
                    status=status
                />
            }
            .to_html()
        });

        assert!(html.contains("<h1>A quiet page</h1>"));
        assert!(html.contains("<p>First paragraph.</p>"));
        assert!(html.contains("<pre><code>let x = 1;</code></pre>"));
        assert!(html.contains("Starred"));
    }

    #[test]
    fn reader_actions_render_action_state_labels() {
        let html = Owner::new().with(|| {
            view! {
                <ReaderActions
                    item=Some(sample_item())
                    can_previous=true
                    can_next=false
                    info_open=false
                    on_back=UnsyncCallback::new(|()| {})
                    on_previous=UnsyncCallback::new(|()| {})
                    on_next=UnsyncCallback::new(|()| {})
                    on_toggle_star=UnsyncCallback::new(|()| {})
                    on_toggle_read=UnsyncCallback::new(|()| {})
                    on_toggle_info=UnsyncCallback::new(|()| {})
                />
            }
            .to_html()
        });

        assert!(html.contains("Unstar"));
        assert!(html.contains("Mark read"));
        assert!(html.contains("Item info"));
        assert!(html.contains("Open original"));
        assert!(html.contains("aria-label=\"Previous item\""));
        assert!(html.contains("aria-label=\"Next item\""));
    }

    #[test]
    fn item_info_renders_reader_metadata() {
        let html = Owner::new().with(|| {
            view! {
                <ItemInfo
                    item=sample_item()
                    on_close=UnsyncCallback::new(|()| {})
                />
            }
            .to_html()
        });

        assert!(html.contains("Item info"));
        assert!(html.contains("https://example.com/feed.xml"));
        assert!(html.contains("https://example.com/page"));
        assert!(html.contains("Readable cache ready"));
        assert!(html.contains("2026-05-01T12:00:00Z"));
        assert!(html.contains("1200 words"));
        assert!(html.contains("long form"));
        assert!(html.contains("code"));
    }

    fn sample_item() -> ItemDetail {
        ItemDetail {
            id: "item-1".to_string(),
            title: "A quiet page".to_string(),
            source_name: "Example".to_string(),
            feed_url: Some("https://example.com/feed.xml".to_string()),
            published_at: Some("2026-04-30T00:00:00Z".to_string()),
            language: Some("en".to_string()),
            length_label: "5 min".to_string(),
            estimated_read_minutes: Some(5),
            word_count: Some(1200),
            time_label: "2d ago".to_string(),
            discovered_at: "2026-05-01T12:00:00Z".to_string(),
            read_state: "unread".to_string(),
            starred: true,
            primary_url: Some("https://example.com/page".to_string()),
            summary: None,
            content_html: "<p>First paragraph.</p><pre><code>let x = 1;</code></pre>".to_string(),
            creators: vec!["Ada".to_string()],
            byline: None,
            readable_blob_id: Some("abc123".to_string()),
            shape_tags: vec!["long form".to_string(), "code".to_string()],
            last_scroll: Some(12.0),
            discovered_at_micros: 1,
        }
    }
}
