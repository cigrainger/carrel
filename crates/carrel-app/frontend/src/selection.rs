//! Mouse access to the same durable selection used by the reader keymap.

use leptos::html;
use leptos::prelude::*;

use crate::api::{CreateHighlightRequest, ItemDetail};

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct SelectedText {
    request: CreateHighlightRequest,
    left: f64,
    top: f64,
}

#[component]
pub(crate) fn SelectionActions(
    item: RwSignal<Option<ItemDetail>>,
    body_ref: NodeRef<html::Div>,
    on_highlight: UnsyncCallback<CreateHighlightRequest>,
    on_note: UnsyncCallback<CreateHighlightRequest>,
) -> impl IntoView {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::{JsCast, closure::Closure};

        let selected = RwSignal::new(None::<SelectedText>);
        Effect::new(move |_| {
            let _ = item.get();
            selected.set(None);
        });
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                // Keep the captured request while a keyboard user focuses its controls.
                let in_controls = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.active_element())
                    .is_some_and(|element| {
                        element
                            .closest(".selection-actions")
                            .ok()
                            .flatten()
                            .is_some()
                    });
                if !in_controls {
                    selected.try_set(capture_selection(item, body_ref));
                }
            });
            let _ = document.add_event_listener_with_callback(
                "selectionchange",
                listener.as_ref().unchecked_ref(),
            );
            let listener = StoredValue::new_local((document, listener));
            on_cleanup(move || {
                listener.try_with_value(|(document, listener)| {
                    let _ = document.remove_event_listener_with_callback(
                        "selectionchange",
                        listener.as_ref().unchecked_ref(),
                    );
                });
            });
        }
        let resize = window_event_listener(leptos::ev::resize, move |_| {
            selected.try_set(None);
        });
        on_cleanup(move || resize.remove());
        Effect::new(move |_| {
            if let Some(scroll) = body_ref
                .get()
                .and_then(|body| body.closest(".reader-scroll").ok().flatten())
            {
                let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    selected.try_set(None);
                });
                let _ = scroll
                    .add_event_listener_with_callback("scroll", listener.as_ref().unchecked_ref());
                let listener = StoredValue::new_local((scroll, listener));
                on_cleanup(move || {
                    listener.try_with_value(|(scroll, listener)| {
                        let _ = scroll.remove_event_listener_with_callback(
                            "scroll",
                            listener.as_ref().unchecked_ref(),
                        );
                    });
                });
            }
        });
        view! {
            {move || selected.get().map(|selection| {
                let highlight = selection.request.clone();
                let note = selection.request;
                view! {
                    <div class="selection-actions" role="toolbar" aria-label="Selected text"
                        style:left=format!("{}px", selection.left)
                        style:top=format!("{}px", selection.top)
                        on:mousedown=move |event| event.prevent_default()
                    >
                        <button type="button" title="Highlight selection (h)" on:click=move |_| {
                            on_highlight.run(highlight.clone());
                            selected.set(None);
                        }>"Highlight"</button>
                        <button type="button" title="Highlight and add note (H)" on:click=move |_| {
                            on_note.run(note.clone());
                            selected.set(None);
                        }>"Add note"</button>
                    </div>
                }
            })}
        }
        .into_any()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (item, body_ref, on_highlight, on_note);
        ().into_any()
    }
}

#[cfg(target_arch = "wasm32")]
fn capture_selection(
    item: RwSignal<Option<ItemDetail>>,
    body_ref: NodeRef<html::Div>,
) -> Option<SelectedText> {
    let detail = item.try_get_untracked().flatten()?;
    let body = body_ref.try_get_untracked().flatten()?;
    let window = web_sys::window()?;
    let range = window
        .get_selection()
        .ok()
        .flatten()?
        .get_range_at(0)
        .ok()?;
    let bounds = range.get_bounding_client_rect();
    let scroll = body
        .closest(".reader-scroll")
        .ok()
        .flatten()?
        .get_bounding_client_rect();
    if bounds.bottom() < scroll.top() || bounds.top() > scroll.bottom() {
        return None;
    }
    let request =
        crate::highlights::capture_current_selection(&body, detail.id, detail.readable_blob_id)?;
    let left = bounds.left().clamp(
        scroll.left() + 8.0,
        (scroll.right() - 240.0).max(scroll.left() + 8.0),
    );
    let top = (bounds.bottom() + 8.0).clamp(
        scroll.top() + 8.0,
        (scroll.bottom() - 52.0).max(scroll.top() + 8.0),
    );
    Some(SelectedText { request, left, top })
}
