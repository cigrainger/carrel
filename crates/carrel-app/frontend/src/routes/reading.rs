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
use crate::api::{self, CreateHighlightNoteRequest, ReadProgressUpdate};
use crate::api::{CreateHighlightRequest, HighlightDetail, ItemDetail};
use crate::components::{Icon, IconName, Modal};
use crate::feeds::{FeedController, use_feed_controller};
#[cfg(target_arch = "wasm32")]
use crate::highlights;
use crate::item_actions::{ItemAction, ItemActions, ItemState, use_item_actions};
use crate::keymap::{use_action_handler, use_keymap_layer};
use crate::navigation::{ListNavigation, use_list_navigation_context, use_reading_sidebar};
use crate::progress::{ProgressWriter, use_progress_writer};
#[cfg(target_arch = "wasm32")]
use crate::requests::Requests;
use crate::status::{StatusController, use_status_controller};

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingNoteDraft {
    request: CreateHighlightRequest,
    exact: String,
    body: String,
    saved_highlight: Option<HighlightDetail>,
    failure: Option<String>,
}

impl PendingNoteDraft {
    fn new(request: CreateHighlightRequest, exact: String) -> Self {
        Self {
            request,
            exact,
            body: String::new(),
            saved_highlight: None,
            failure: None,
        }
    }
}

/// Full-page article reader.
#[component]
pub fn ReadingView() -> impl IntoView {
    let params = use_params_map();
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let item_id = Memo::new(move |_| params.read().get("id").unwrap_or_default());
    let item = RwSignal::new(None::<ItemDetail>);
    let highlights = RwSignal::new(Vec::<HighlightDetail>::new());
    let highlights_ready = RwSignal::new(false);
    let annotation_actions = crate::annotation_actions::use_annotation_actions();
    let visible_highlights =
        Signal::derive(move || annotation_actions.project_highlights(highlights.get()));
    let error = RwSignal::new(None::<String>);
    let loaded = RwSignal::new(false);
    let reload = RwSignal::new(0_u64);
    let info_open = RwSignal::new(false);
    let pending_note = RwSignal::new(None::<PendingNoteDraft>);
    let note_saving = RwSignal::new(false);
    let restored_item = RwSignal::new(None::<String>);
    let scroll_ref = NodeRef::<html::Div>::new();
    let body_ref = NodeRef::<html::Div>::new();
    let navigate = use_navigate();
    let list_navigation = use_list_navigation_context();
    let status = use_status_controller();
    let feeds = use_feed_controller();
    let actions = use_item_actions();
    let sharing = crate::sharing::use_sharing();
    let sidebar = use_reading_sidebar();
    let progress_writer = use_progress_writer();
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let cleanup_writer = progress_writer.clone();
    on_cleanup(move || cleanup_writer.flush());
    let progress_writer = StoredValue::new(progress_writer);
    let projected = Memo::new(move |_| {
        item.get().map(|mut detail| {
            let state = actions.state.get().project(
                &detail.id,
                ItemState {
                    read: detail.read_state.clone(),
                    starred: detail.starred,
                },
            );
            detail.read_state = state.read;
            detail.starred = state.starred;
            detail
        })
    });
    use_action_handler("toggle-reading-sidebar", move || sidebar.toggle());

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
            projected,
            list_navigation,
            next_navigate.clone(),
            Direction::Next,
            actions,
            true,
        );
    });

    let previous_navigate = navigate.clone();
    use_action_handler("prev-item", move || {
        navigate_adjacent(
            projected,
            list_navigation,
            previous_navigate.clone(),
            Direction::Previous,
            actions,
            false,
        );
    });

    let legacy_previous = navigate.clone();
    use_action_handler("prev-item-mark-read", move || {
        navigate_adjacent(
            projected,
            list_navigation,
            legacy_previous.clone(),
            Direction::Previous,
            actions,
            true,
        )
    });
    let plain_next = navigate.clone();
    use_action_handler("next-item", move || {
        navigate_adjacent(
            projected,
            list_navigation,
            plain_next.clone(),
            Direction::Next,
            actions,
            false,
        )
    });

    use_action_handler("mark-read", move || {
        request_current(projected, actions, ItemAction::Read(true));
    });

    use_action_handler("mark-unread", move || {
        request_current(projected, actions, ItemAction::Read(false));
    });

    use_action_handler("toggle-star", move || {
        request_current(projected, actions, ItemAction::Star);
    });
    for action in ["share-last-audience", "share-audience-picker"] {
        use_action_handler(action, move || {
            if let Some(article) = projected.get_untracked() {
                sharing.open(article.id, article.title);
            }
        });
    }

    use_action_handler("show-item-info", move || {
        info_open.update(|open| *open = !*open);
    });

    let highlight_body_ref = body_ref;
    use_action_handler("highlight-selection", move || {
        create_highlight_from_selection(
            item,
            highlight_body_ref,
            highlights,
            status,
            feeds,
            writes,
        );
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
        let requests = Requests::default();
        let cleanup = requests.clone();
        on_cleanup(move || cleanup.invalidate());
        let remote_requests = Requests::default();
        let cleanup = remote_requests.clone();
        on_cleanup(move || cleanup.invalidate());
        Effect::new(move |_| {
            let _ = feeds.sync_revision.get();
            let Some(previous) = item.get_untracked() else {
                return;
            };
            let id = previous.id;
            let generation = remote_requests.begin();
            let guard = remote_requests.clone();
            let action_revision = actions.state.with_untracked(|state| state.revision());
            let scroll_y = scroll_ref
                .get_untracked()
                .map(|element| f64::from(element.scroll_top()));
            spawn_local(async move {
                let Ok(detail) = api::get_item(id.clone()).await else {
                    return;
                };
                if !guard.accepts(generation) || item_id.get_untracked() != id {
                    return;
                }
                actions
                    .state
                    .update(|state| state.rebase(action_revision, std::slice::from_ref(&id)));
                // Replace backend data without clearing the reader, its local
                // note draft, or the user's active scroll position.
                item.set(detail);
                restore_scroll(scroll_ref, scroll_y);
                if let Ok(rows) = api::list_item_highlights(id.clone()).await
                    && guard.accepts(generation)
                    && item_id.get_untracked() == id
                {
                    highlights.set(rows);
                    highlights_ready.set(true);
                }
            });
        });
        Effect::new(move |_| {
            let id = item_id.get();
            let _ = reload.get();
            progress_writer.get_value().flush();
            let generation = requests.begin();
            let guard = requests.clone();
            let action_revision = actions.state.with_untracked(|state| state.revision());
            item.set(None);
            highlights.set(Vec::new());
            highlights_ready.set(false);
            error.set(None);
            loaded.set(false);
            restored_item.set(None);
            info_open.set(false);
            pending_note.set(None);
            note_saving.set(false);

            spawn_local(async move {
                let result = api::get_item(id.clone()).await;
                if !guard.accepts(generation) {
                    return;
                }
                match result {
                    Ok(mut detail) => {
                        if let Some(detail) = detail.as_ref() {
                            actions.state.update(|state| {
                                state.rebase(action_revision, std::slice::from_ref(&detail.id))
                            });
                        }
                        let has_item = detail.is_some();
                        if let Some(detail) = detail.as_mut() {
                            let latest = progress_writer.get_value().latest(&detail.id);
                            if let Some(latest) = latest {
                                detail.last_scroll = Some(latest.scroll_y);
                                detail.read_progress = Some(latest.progress);
                            }
                            progress_writer.get_value().record(ReadProgressUpdate {
                                item_id: detail.id.clone(),
                                progress: detail.read_progress.unwrap_or(0.0),
                                scroll_y: detail.last_scroll.unwrap_or(0.0),
                            });
                        }
                        item.set(detail);
                        loaded.set(true);
                        if has_item {
                            match api::list_item_highlights(id).await {
                                Ok(mut rows) if guard.accepts(generation) => {
                                    // A newly saved highlight may have arrived while this query ran.
                                    for saved in highlights.get_untracked() {
                                        if !rows.iter().any(|row| row.id == saved.id) {
                                            rows.push(saved);
                                        }
                                    }
                                    highlights.set(rows);
                                    highlights_ready.set(true);
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    if guard.accepts(generation) {
                                        status.warn(format!(
                                            "Could not load highlights: {}",
                                            err.message()
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        error.set(Some(err.message().to_string()));
                    }
                }
                if guard.accepts(generation) {
                    loaded.set(true);
                }
            });
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        loaded.set(true);
    }

    let complete_navigate = navigate.clone();
    let complete = UnsyncCallback::new(move |()| {
        navigate_adjacent(
            projected,
            list_navigation,
            complete_navigate.clone(),
            Direction::Next,
            actions,
            true,
        )
    });
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
                             projected,
                            list_navigation,
                            navigate.clone(),
                            Direction::Previous,
                             actions,
                             false,
                        );
                    })
                };
                let next_click = {
                    let navigate = navigate.clone();
                    UnsyncCallback::new(move |()| {
                        navigate_adjacent(
                             projected,
                            list_navigation,
                            navigate.clone(),
                            Direction::Next,
                             actions,
                             false,
                        );
                    })
                };
                let star_click = UnsyncCallback::new(move |()| request_current(projected, actions, ItemAction::Star));
                let read_click = UnsyncCallback::new(move |()| {
                    let read = projected.get_untracked().is_some_and(|detail| detail.read_state != "read");
                    request_current(projected, actions, ItemAction::Read(read));
                });
                let info_click = UnsyncCallback::new(move |()| info_open.update(|open| *open = !*open));

                view! {
                    <crate::components::ContextControls><ReaderActions
                            item=projected
                            can_previous=Signal::derive(move || can_navigate_adjacent(item.get(), list_navigation.get(), Direction::Previous))
                            can_next=Signal::derive(move || can_navigate_adjacent(item.get(), list_navigation.get(), Direction::Next))
                            info_open=info_open
                            on_back=back_click
                            on_previous=previous_click
                            on_next=next_click
                            on_toggle_star=star_click
                            on_toggle_read=read_click
                            on_toggle_info=info_click
                            on_share=UnsyncCallback::new(move |()| { if let Some(article) = projected.get_untracked() { sharing.open(article.id,article.title); } })
                            on_open_original=UnsyncCallback::new(move |url| crate::links::open_original(url, status))
                            back_label=list_navigation.get().map(|context| format!("Back to {}", context.return_label)).unwrap_or_else(|| "Back to Today".into())
                        /></crate::components::ContextControls>
                }
            }
            {move || {
                pending_note
                    .get()
                    .map(|draft| {
                        let confirm = UnsyncCallback::new(move |body: String| {
                            if body.trim().is_empty() {
                                status.info("Write a note before saving.");
                                return;
                            }
                            if note_saving.get_untracked() {return;}
                            let Some(mut draft) = pending_note.get_untracked() else {return;};
                            draft.body = body; draft.failure = None;
                            pending_note.set(Some(draft.clone()));
                            note_saving.set(true);
                            confirm_highlight_note(
                                draft,
                                item,
                                highlights,
                                status,
                                feeds,
                                writes,
                                (pending_note, note_saving),
                            );
                        });
                        let cancel = UnsyncCallback::new(move |()| {
                            if note_saving.get_untracked() {return;}
                            pending_note.set(None);
                            status.info("Highlight note cancelled.");
                        });
                        view! {
                            <HighlightNoteOverlay
                                exact=draft.exact
                                initial_body=draft.body
                                failure=draft.failure
                                saving=note_saving
                                on_confirm=confirm
                                on_cancel=cancel
                            />
                        }.into_any()
                    })
            }}
            <crate::selection::SelectionActions item=item body_ref=body_ref
                on_highlight=UnsyncCallback::new(move |request| write_highlight(request, item, highlights, status, feeds, writes))
                on_note=UnsyncCallback::new(move |request: CreateHighlightRequest| {
                    let carrel_core::annotation::HighlightLocation::HtmlText(selector) = &request.location;
                    pending_note.set(Some(PendingNoteDraft::new(request.clone(), selector.exact.clone())));
                })
            />
            <div
                class="reader-scroll"
                node_ref=scroll_ref
                on:scroll=move |_| persist_progress(item, scroll_ref, &progress_writer.get_value())
            >
                <Show
                    when=move || loaded.get()
                    fallback=|| view! { <div class="reading-empty" aria-hidden="true"></div> }
                >
                    {move || match error.get() {
                        Some(message) => view! {
                            <div class="quiet-state reader-message" role="status"><p>{message}</p><button type="button" on:click=move |_| reload.update(|token| *token += 1)>"Try again"</button></div>
                        }.into_any(),
                        None => item.get()
                            .map(|detail| {
                                let info_detail = detail.clone();
                                let close_info = UnsyncCallback::new(move |()| info_open.set(false));
                                view! {
                                    <Show when=move || info_open.get()>
                                        <ItemInfo item=info_detail.clone() on_close=close_info/>
                                    </Show>
                                    <ArticleContent
                                        item=detail
                                         highlights=visible_highlights
                                         highlights_ready=highlights_ready
                                         body_ref=body_ref
                                         status=status
                                         can_next=Signal::derive(move || can_navigate_adjacent(item.get(), list_navigation.get(), Direction::Next))
                                         on_complete=complete
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

/// Render persistent reader controls without replacing focused buttons on a state change.
#[component]
pub fn ReaderActions(
    #[prop(into)] item: Signal<Option<ItemDetail>>,
    #[prop(into)] can_previous: Signal<bool>,
    #[prop(into)] can_next: Signal<bool>,
    #[prop(into)] info_open: Signal<bool>,
    on_back: UnsyncCallback<()>,
    on_previous: UnsyncCallback<()>,
    on_next: UnsyncCallback<()>,
    on_toggle_star: UnsyncCallback<()>,
    on_toggle_read: UnsyncCallback<()>,
    on_toggle_info: UnsyncCallback<()>,
    #[prop(default = UnsyncCallback::new(|()| {}))] on_share: UnsyncCallback<()>,
    #[prop(default = String::from("Back"))] back_label: String,
    #[prop(default = UnsyncCallback::new(|_: String| {}))] on_open_original: UnsyncCallback<String>,
) -> impl IntoView {
    let share_button = NodeRef::<html::Button>::new();
    view! {
        <nav class="reader-actions" aria-label="Reader controls">
            <div class="reader-action-group">
                <button type="button" class="reader-action-primary" aria-label=back_label.clone() on:click=move |_| on_back.run(())><Icon name=IconName::Back/><span class="reader-return-label">{back_label.clone()}</span></button>
                <button type="button" disabled=move || !can_previous.get() aria-label="Previous item" title="Previous item (P)" on:click=move |_| on_previous.run(())><Icon name=IconName::Previous/></button>
                <button type="button" disabled=move || !can_next.get() aria-label="Next item" title="Next item" on:click=move |_| on_next.run(())><Icon name=IconName::Next/></button>
            </div>
            <div class="reader-action-group">
                <button type="button" disabled=move || item.with(Option::is_none)
                    aria-pressed=move || item.with(|item| item.as_ref().is_some_and(|detail| detail.starred).to_string())
                    class="reader-star-action"
                    aria-label=move || item.with(|item| star_action_label(item.as_ref().is_some_and(|detail| detail.starred)))
                    title=move || item.with(|item| if item.as_ref().is_some_and(|detail| detail.starred) { "Unstar (S)" } else { "Star (S)" })
                    on:click=move |_| on_toggle_star.run(())>
                    <Icon name=IconName::Star/>
                </button>
                <button type="button" class="reader-read-action" disabled=move || item.with(Option::is_none)
                    aria-label=move || item.with(|item| read_action_label(item.as_ref().map_or("unread", |detail| detail.read_state.as_str())))
                    title=move || item.with(|item| if item.as_ref().is_some_and(|detail| detail.read_state == "read") { "Mark unread (⇧M)" } else { "Mark read (M)" })
                    aria-pressed=move || item.with(|item| item.as_ref().is_some_and(|detail| detail.read_state == "read").to_string())
                    on:click=move |_| on_toggle_read.run(())>
                    <Icon name=IconName::Read/>
                </button>
                <button type="button" disabled=move || item.with(Option::is_none)
                    aria-label=move || if info_open.get() { "Hide info" } else { "Item info" }
                    title=move || if info_open.get() { "Hide info (I)" } else { "Item info (I)" }
                    aria-pressed=move || info_open.get().to_string() on:click=move |_| on_toggle_info.run(())>
                    <Icon name=IconName::Info/>
                </button>
            </div>
            <div class="reader-action-group">
                <button node_ref=share_button type="button" disabled=move || item.with(Option::is_none) aria-label="Share article" title="Share article (C)" on:click=move |_| { if let Some(button) = share_button.get_untracked() { let _ = button.focus(); } on_share.run(()); }><Icon name=IconName::Share/></button>
                <Show when=move || item.with(|item| item.as_ref().is_some_and(|detail| detail.primary_url.is_some()))
                    fallback=|| view! { <button type="button" disabled=true aria-label="Open original" title="Open original"><Icon name=IconName::Original/></button> }>
                    <a class="reader-action-link" aria-label="Open original" title="Open original" href=move || item.with(|item| item.as_ref().and_then(|detail| detail.primary_url.clone()).unwrap_or_default()) target="_blank" rel="noreferrer"
                        on:click=move |event| { event.prevent_default(); if let Some(url) = item.with_untracked(|item| item.as_ref().and_then(|detail| detail.primary_url.clone())) { on_open_original.run(url); } }
                        on:auxclick=move |event| { if event.button() == 1 { event.prevent_default(); if let Some(url) = item.with_untracked(|item| item.as_ref().and_then(|detail| detail.primary_url.clone())) { on_open_original.run(url); } } }
                    ><Icon name=IconName::Original/></a>
                </Show>
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
                <button type="button" aria-label="Close item info" title="Close item info (Esc)" on:click=move |_| on_close.run(())>
                    <Icon name=IconName::Close/>
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
            {item.shared_notes.into_iter().map(|share| view! { <section class="received-vouch"><h3>{format!("Shared by {}",share.from)}</h3><p>{share.date}</p>{share.note.map(|note| view! { <blockquote>{note}</blockquote> })}</section> }).collect_view()}
        </aside>
    }
}

/// Small focus-contained note overlay for `H`.
#[component]
pub fn HighlightNoteOverlay(
    exact: String,
    #[prop(default = String::new())] initial_body: String,
    #[prop(optional_no_strip)] failure: Option<String>,
    #[prop(default = Signal::stored(false), into)] saving: Signal<bool>,
    on_confirm: UnsyncCallback<String>,
    on_cancel: UnsyncCallback<()>,
) -> impl IntoView {
    let note = RwSignal::new(initial_body);
    let note_ref = NodeRef::<html::Textarea>::new();

    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(textarea) = note_ref.get() {
            let _ = textarea.focus();
        }
    });

    let submit = move || {
        let body = note.get_untracked();
        if !body.trim().is_empty() && !saving.get_untracked() {
            on_confirm.run(body);
        }
    };

    view! {
        <Modal label="Highlight note" class="note-dialog" on_cancel=on_cancel>
            <section
                on:keydown=move |event| handle_note_dialog_keydown(event, submit)
            >
                <h2>"Highlight note"</h2>
                <blockquote>{exact}</blockquote>
                <textarea
                    node_ref=note_ref
                    rows="5"
                    aria-label="Note"
                    autofocus=true
                    readonly=move || saving.get()
                    prop:value=move || note.get()
                    on:input=move |event| note.set(event_target_value(&event))
                ></textarea>
                {failure.map(|message| view! {<p class="annotation-error" role="alert">{message}</p>})}
                <div class="note-dialog-actions">
                    <button
                        type="button"
                        disabled=move || saving.get()
                        on:click=move |_| on_cancel.run(())
                    >
                        "Cancel"
                    </button>
                    <button
                        type="button"
                        disabled=move || saving.get() || note.with(|value| value.trim().is_empty())
                        on:click=move |_| submit()
                    >
                        "Save"
                    </button>
                </div>
            </section>
        </Modal>
    }
}

/// Render the article payload itself.
#[component]
pub fn ArticleContent(
    item: ItemDetail,
    #[prop(into)] highlights: Signal<Vec<HighlightDetail>>,
    #[prop(default = Signal::stored(true), into)] highlights_ready: Signal<bool>,
    body_ref: NodeRef<html::Div>,
    status: StatusController,
    #[prop(into)] can_next: Signal<bool>,
    on_complete: UnsyncCallback<()>,
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
    let date = item
        .published_at
        .as_deref()
        .map(publication_date_label)
        .unwrap_or(item.time_label.clone());
    let has_original = item.primary_url.is_some();
    let content_html = item.content_html.clone();
    let has_cache = item.readable_blob_id.is_some();
    let has_text = !content_html.trim().is_empty();
    let link_original = StoredValue::new(item.primary_url.clone());

    #[cfg(not(target_arch = "wasm32"))]
    let _ = (&highlights, highlights_ready, status);

    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            if let Some(body) = body_ref.get() {
                crate::reader_images::install(&body);
            }
        });
        let content_html = content_html.clone();
        Effect::new(move |_| {
            let Some(body) = body_ref.get() else {
                return;
            };
            let hash_id = highlights::current_hash_highlight_id();
            let focus_id =
                highlights::focus_target_after_load(highlights_ready.get(), hash_id.as_deref());
            let failed =
                highlights::render_highlights(&body, &content_html, &highlights.get(), focus_id);
            if let Some(focus_id) = focus_id
                && failed.iter().any(|id| id == focus_id)
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
                </div>
            </header>
            <Show when=move || !has_cache>
                <aside class="reader-cache-notice" role="note">
                    {if has_text { "The full article is not cached. This is the saved feed summary." } else { "No article text is cached. Open the original when you are online." }}
                </aside>
            </Show>
            <div class="reader-body" dir="auto" node_ref=body_ref inner_html=content_html
                on:click=move |event| crate::links::article_link(event, body_ref, link_original.with_value(|url| url.clone()).as_deref(), status)
                on:auxclick=move |event| crate::links::article_link(event, body_ref, link_original.with_value(|url| url.clone()).as_deref(), status)
            ></div>
            <footer class="reader-footer">
                <Show when=move || has_original>
                    <a
                        href=item.primary_url.clone().unwrap_or_default()
                        target="_blank"
                        rel="noreferrer"
                        on:click=move |event| { event.prevent_default(); if let Some(url) = link_original.get_value() { crate::links::open_original(url, status); } }
                        on:auxclick=move |event| { if event.button() == 1 { event.prevent_default(); if let Some(url) = link_original.get_value() { crate::links::open_original(url, status); } } }
                    >
                        <Icon name=IconName::Original/><span>"Open original"</span>
                    </a>
                </Show>
                <button type="button" class="reader-completion" title="Mark read and continue (N)" on:click=move |_| on_complete.run(())>
                    <Icon name=IconName::Read/>
                    <span>{move || if can_next.get() { "Mark read & next" } else { "Mark read & return" }}</span>
                </button>
            </footer>
        </article>
    }
}

fn publication_date_label(value: &str) -> String {
    let Ok(date) =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
    else {
        return value.to_string();
    };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {}",
        date.day(),
        months[date.month() as usize - 1],
        date.year()
    )
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
    feeds: FeedController,
    writes: StoredValue<crate::writes::PendingWrites>,
) {
    let Some(request) = capture_highlight_request(item, body_ref, status) else {
        return;
    };
    write_highlight(request, item, highlights, status, feeds, writes);
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
    pending_note.set(Some(PendingNoteDraft::new(request, exact)));
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

#[cfg(any(test, target_arch = "wasm32"))]
fn is_current_item(item: RwSignal<Option<ItemDetail>>, item_id: &str) -> bool {
    item.try_with_untracked(|current| {
        current
            .as_ref()
            .is_some_and(|current| current.id == item_id)
    })
    .unwrap_or(false)
}

fn write_highlight(
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    request: CreateHighlightRequest,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] item: RwSignal<
        Option<ItemDetail>,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] highlights: RwSignal<
        Vec<HighlightDetail>,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] feeds: FeedController,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] writes: StoredValue<
        crate::writes::PendingWrites,
    >,
) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        let guard = writes.get_value().begin();
        spawn_local(async move {
            let _guard = guard;
            match api::create_highlight(request).await {
                Ok(highlight) => {
                    if is_current_item(item, &highlight.item_id) {
                        highlights.try_update(|rows| rows.push(highlight));
                        highlights::clear_current_selection();
                    }
                    status.info("Highlighted.");
                    feeds.bump_refresh();
                }
                Err(error) => {
                    status.warn(format!("Could not save highlight: {}", error.message()));
                }
            }
        });
    }
}

fn confirm_highlight_note(
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] mut draft: PendingNoteDraft,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] item: RwSignal<
        Option<ItemDetail>,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] highlights: RwSignal<
        Vec<HighlightDetail>,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] status: StatusController,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] feeds: FeedController,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] writes: StoredValue<
        crate::writes::PendingWrites,
    >,
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))] editor: (
        RwSignal<Option<PendingNoteDraft>>,
        RwSignal<bool>,
    ),
) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen_futures::spawn_local;

        let guard = writes.get_value().begin();
        spawn_local(async move {
            let _guard = guard;
            let result = note_highlight(&mut draft, api::create_highlight).await;
            match result {
                Ok(highlight) => {
                    if is_current_item(item, &highlight.item_id) {
                        highlights.try_update(|rows| {
                            if !rows.iter().any(|row| row.id == highlight.id) {
                                rows.push(highlight.clone());
                            }
                        });
                    }
                    let note_request = CreateHighlightNoteRequest {
                        highlight_id: highlight.id.clone(),
                        body: draft.body.clone(),
                    };
                    match api::create_highlight_note(note_request).await {
                        Ok(_) => {
                            if is_current_item(item, &highlight.item_id) {
                                highlights::clear_current_selection();
                                editor.0.set(None);
                                editor.1.set(false);
                            }
                            status.info("Highlight note saved.");
                            feeds.bump_refresh();
                        }
                        Err(error) => {
                            if is_current_item(item, &highlight.item_id) {
                                draft.failure = Some(format!(
                                    "The highlight was saved, but its note could not be saved. Your draft is still here; retrying will use the same highlight. {}",
                                    error.message()
                                ));
                                editor.0.set(Some(draft));
                                editor.1.set(false);
                            }
                            status.warn(format!(
                                "Highlight saved, but note failed: {}",
                                error.message()
                            ));
                            feeds.bump_refresh();
                        }
                    }
                }
                Err(error) => {
                    if is_current_item(item, &draft.request.item_id) {
                        draft.failure = Some(format!(
                            "Could not save this highlight and note. Your draft is still here. {}",
                            error.message()
                        ));
                        editor.0.set(Some(draft));
                        editor.1.set(false);
                    }
                    status.warn(format!("Could not save highlight: {}", error.message()));
                }
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = &mut draft;
}

#[cfg(any(test, target_arch = "wasm32"))]
async fn note_highlight<F, Fut>(
    draft: &mut PendingNoteDraft,
    create: F,
) -> Result<HighlightDetail, crate::api::ApiError>
where
    F: FnOnce(CreateHighlightRequest) -> Fut,
    Fut: std::future::Future<Output = Result<HighlightDetail, crate::api::ApiError>>,
{
    if let Some(highlight) = &draft.saved_highlight {
        return Ok(highlight.clone());
    }
    let highlight = create(draft.request.clone()).await?;
    // A failed note write must retry against this same confirmed highlight.
    draft.saved_highlight = Some(highlight.clone());
    Ok(highlight)
}

#[cfg(target_arch = "wasm32")]
fn handle_note_dialog_keydown(event: ev::KeyboardEvent, submit: impl Fn() + Copy + 'static) {
    if event.key() == "Enter" && (event.meta_key() || event.ctrl_key()) {
        event.prevent_default();
        event.stop_propagation();
        submit();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_note_dialog_keydown(_event: ev::KeyboardEvent, _submit: impl Fn() + Copy + 'static) {}

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
    item: Memo<Option<ItemDetail>>,
    list_navigation: crate::navigation::ListNavigationContext,
    navigate: impl Fn(&str, NavigateOptions) + Clone + 'static,
    direction: Direction,
    actions: ItemActions,
    complete: bool,
) {
    let Some(current_id) =
        item.with_untracked(|detail| detail.as_ref().map(|detail| detail.id.clone()))
    else {
        return;
    };
    if complete {
        request_current(item, actions, ItemAction::Read(true));
    }
    let Some(next_navigation) = next_list_navigation(list_navigation.get(), &current_id, direction)
    else {
        if complete {
            navigate_back(list_navigation, navigate);
        }
        return;
    };

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

fn request_current(item: Memo<Option<ItemDetail>>, actions: ItemActions, action: ItemAction) {
    if let Some(detail) = item.get_untracked() {
        actions.request(
            detail.id,
            ItemState {
                read: detail.read_state,
                starred: detail.starred,
            },
            action,
        );
    }
}

fn persist_progress(
    item: RwSignal<Option<ItemDetail>>,
    scroll_ref: NodeRef<html::Div>,
    writer: &ProgressWriter,
) {
    #[cfg(target_arch = "wasm32")]
    {
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
        writer.record(ReadProgressUpdate {
            item_id: detail.id,
            progress,
            scroll_y,
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (item, scroll_ref, writer);
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
    if let Some(element) = scroll_ref.try_get_untracked().flatten() {
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
    fn partial_note_retry_reuses_the_confirmed_highlight_and_keeps_the_draft() {
        use std::{
            cell::Cell,
            future::{Future, ready},
            task::{Context, Poll, Waker},
        };
        fn complete<T>(future: impl Future<Output = T>) -> T {
            let mut future = std::pin::pin!(future);
            match future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            {
                Poll::Ready(value) => value,
                Poll::Pending => panic!("fixture should be immediately ready"),
            }
        }
        let location = carrel_core::annotation::HighlightLocation::HtmlText(
            carrel_core::annotation::HtmlTextSelector {
                content_blob_id: None,
                exact: "quiet".into(),
                prefix: String::new(),
                suffix: String::new(),
                start: 0,
                end: 5,
            },
        );
        let request = CreateHighlightRequest {
            item_id: "item".into(),
            location: location.clone(),
            location_label: None,
            color: None,
        };
        let saved = HighlightDetail {
            id: "confirmed".into(),
            item_id: "item".into(),
            quoted_text: "quiet".into(),
            location,
            location_label: None,
            color: None,
            created_by: String::new(),
            created_at: String::new(),
            created_at_micros: 1,
            signature: String::new(),
        };
        let mut draft = PendingNoteDraft::new(request.clone(), "quiet".into());
        draft.body = "Retained café draft".into();
        let calls = Cell::new(0);
        for _ in 0..2 {
            let actual = complete(note_highlight(&mut draft, |actual| {
                assert_eq!(actual, request);
                calls.set(calls.get() + 1);
                ready(Ok(saved.clone()))
            }))
            .unwrap();
            assert_eq!(actual, saved);
            draft.failure = Some("note failed after highlight succeeded".into());
        }
        assert_eq!(calls.get(), 1);
        assert_eq!(draft.body, "Retained café draft");
        assert_eq!(draft.saved_highlight, Some(saved.clone()));

        let mut fresh = PendingNoteDraft::new(request, "quiet".into());
        fresh.body = "First attempt failed".into();
        assert!(
            complete(note_highlight(&mut fresh, |_| ready(Err(
                crate::api::ApiError::new("creation failed")
            ))))
            .is_err()
        );
        assert!(fresh.saved_highlight.is_none());
        assert_eq!(fresh.body, "First attempt failed");
        assert_eq!(
            complete(note_highlight(&mut fresh, |_| ready(Ok(saved.clone())))).unwrap(),
            saved
        );
    }

    #[test]
    fn a_highlight_save_can_only_update_the_article_it_belongs_to() {
        Owner::new().with(|| {
            let item = RwSignal::new(Some(sample_item()));
            assert!(is_current_item(item, "item-1"));
            assert!(!is_current_item(item, "item-2"));
            let mut different = sample_item();
            different.id = "item-2".into();
            item.set(Some(different));
            assert!(!is_current_item(item, "item-1"));
            item.set(None);
            assert!(!is_current_item(item, "item-1"));
        });
    }

    #[test]
    fn a_highlight_result_after_route_cleanup_is_ignored() {
        let owner = Owner::new();
        let item = owner.with(|| RwSignal::new(Some(sample_item())));
        owner.cleanup();
        assert!(!is_current_item(item, "item-1"));
    }

    #[test]
    fn final_completion_marks_read_and_returns_to_the_originating_filter() {
        Owner::new().with(|| {
            crate::status::provide_status_controller();
            crate::feeds::provide_feed_controller();
            crate::item_actions::provide_item_actions();
            let actions = use_item_actions();
            let navigation = crate::navigation::provide_list_navigation_context();
            navigation.set(ListNavigation {
                item_ids: vec!["item-1".into()],
                current_id: "item-1".into(),
                return_path: "/unread".into(),
                return_label: "Library".into(),
            });
            let item = Memo::new(|_| Some(sample_item()));
            let destinations = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let recorded = destinations.clone();
            navigate_adjacent(
                item,
                navigation,
                move |path, _| recorded.lock().unwrap().push(path.into()),
                Direction::Next,
                actions,
                true,
            );
            assert_eq!(*destinations.lock().unwrap(), vec!["/unread"]);
            assert_eq!(
                actions
                    .state
                    .get_untracked()
                    .project(
                        "item-1",
                        ItemState {
                            read: "unread".into(),
                            starred: true
                        }
                    )
                    .read,
                "read"
            );
        });
    }

    #[test]
    fn browsing_next_and_previous_does_not_mark_either_article_read() {
        for (current, direction, destination) in [
            ("item-1", Direction::Next, "/item/item-2"),
            ("item-2", Direction::Previous, "/item/item-1"),
        ] {
            Owner::new().with(|| {
                crate::status::provide_status_controller();
                crate::feeds::provide_feed_controller();
                crate::item_actions::provide_item_actions();
                let actions = use_item_actions();
                let navigation = crate::navigation::provide_list_navigation_context();
                navigation.set(ListNavigation {
                    item_ids: vec!["item-1".into(), "item-2".into()],
                    current_id: current.into(),
                    return_path: "/library".into(),
                    return_label: "Library".into(),
                });
                let item = Memo::new(move |_| {
                    let mut detail = sample_item();
                    detail.id = current.into();
                    Some(detail)
                });
                let destinations = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
                let recorded = destinations.clone();
                navigate_adjacent(
                    item,
                    navigation,
                    move |path, _| recorded.lock().unwrap().push(path.into()),
                    direction,
                    actions,
                    false,
                );
                assert_eq!(*destinations.lock().unwrap(), vec![destination]);
                assert!(!actions.state.get_untracked().is_pending(current));
            });
        }
    }

    #[test]
    fn completion_from_a_direct_article_link_returns_to_today() {
        Owner::new().with(|| {
            crate::status::provide_status_controller();
            crate::feeds::provide_feed_controller();
            crate::item_actions::provide_item_actions();
            let actions = use_item_actions();
            let navigation = crate::navigation::provide_list_navigation_context();
            let item = Memo::new(|_| Some(sample_item()));
            let destinations = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let recorded = destinations.clone();
            navigate_adjacent(
                item,
                navigation,
                move |path, _| recorded.lock().unwrap().push(path.into()),
                Direction::Next,
                actions,
                true,
            );
            assert_eq!(*destinations.lock().unwrap(), vec!["/today"]);
            assert!(actions.state.get_untracked().is_pending("item-1"));
        });
    }

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
                    can_next=true
                    on_complete=UnsyncCallback::new(|()| {})
                />
            }
            .to_html()
        });

        assert!(html.contains("<h1>A quiet page</h1>"));
        assert!(html.contains("<p>First paragraph.</p>"));
        assert!(html.contains("<pre><code>let x = 1;</code></pre>"));
        assert!(html.contains("Example"));
        assert!(html.contains("30 Apr 2026"));
        assert!(!html.contains("2026-04-30T00:00:00Z"));
    }

    #[test]
    fn uncached_content_identifies_a_summary_and_an_unavailable_article() {
        for (text, message) in [
            ("<p>Saved introduction.</p>", "saved feed summary"),
            ("", "No article text is cached"),
        ] {
            let html = Owner::new().with(|| {
                let status = crate::status::provide_status_controller();
                let mut item = sample_item();
                item.readable_blob_id = None;
                item.content_html = text.into();
                view! { <ArticleContent item=item highlights=Vec::new() body_ref=NodeRef::new() status=status can_next=false on_complete=UnsyncCallback::new(|()| {})/> }.to_html()
            });
            assert!(html.contains(message));
            assert!(html.contains("Mark read &amp; return"));
            assert!(!html.contains("end of article"));
        }
    }

    #[test]
    fn publication_dates_use_calendar_dates_and_keep_unrecognized_values() {
        assert_eq!(
            publication_date_label("2024-02-29T23:30:00-07:00"),
            "29 Feb 2024"
        );
        assert_eq!(publication_date_label("2026-12-01T00:00:00Z"), "1 Dec 2026");
        assert_eq!(publication_date_label("unknown"), "unknown");
    }

    #[test]
    fn reader_actions_render_action_state_labels() {
        let html = Owner::new().with(|| {
            let item = RwSignal::new(None::<ItemDetail>);
            let controls = view! {
                <ReaderActions
                    item=item
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
            };
            item.set(Some(sample_item()));
            controls.to_html()
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
            shared_notes: Vec::new(),
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
            read_progress: Some(0.5),
            discovered_at_micros: 1,
        }
    }
}
