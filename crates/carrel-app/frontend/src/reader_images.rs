//! Image failure presentation, scoped to the current cached article.

use leptos::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Event, HtmlElement, HtmlImageElement};

pub(crate) fn install(body: &HtmlElement) {
    // Image errors do not bubble. Capture on the stable body so highlight
    // reattachment can replace its children without losing the listener.
    let listener = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if let Some(image) = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlImageElement>().ok())
        {
            recover_or_collapse(&image);
        }
    });
    let _ = body.add_event_listener_with_callback_and_bool(
        "error",
        listener.as_ref().unchecked_ref(),
        true,
    );
    let listener = StoredValue::new_local((body.clone(), listener));
    on_cleanup(move || {
        listener.try_with_value(|(body, listener)| {
            let _ = body.remove_event_listener_with_callback_and_bool(
                "error",
                listener.as_ref().unchecked_ref(),
                true,
            );
        });
    });

    // A cached response may have failed before the component effect ran.
    if let Ok(images) = body.query_selector_all("img") {
        for index in 0..images.length() {
            if let Some(image) = images
                .item(index)
                .and_then(|node| node.dyn_into::<HtmlImageElement>().ok())
                && image.complete()
                && image.natural_width() == 0
            {
                recover_or_collapse(&image);
            }
        }
    }
}

fn recover_or_collapse(image: &HtmlImageElement) {
    if image.has_attribute("data-carrel-image-unavailable") {
        return;
    }
    let sources = image
        .closest("picture")
        .ok()
        .flatten()
        .and_then(|picture| picture.query_selector_all("source[srcset]").ok());
    let has_responsive = image.has_attribute("srcset")
        || sources.as_ref().is_some_and(|sources| sources.length() > 0);
    let fallback = image
        .get_attribute("src")
        .filter(|src| !src.trim().is_empty());
    if has_responsive
        && let Some(fallback) = fallback
        && !image.has_attribute("data-carrel-image-fallback")
    {
        let _ = image.set_attribute("data-carrel-image-fallback", "");
        if let Some(sources) = sources {
            for index in 0..sources.length() {
                if let Some(source) = sources
                    .item(index)
                    .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
                {
                    let _ = source.remove_attribute("srcset");
                }
            }
        }
        let _ = image.remove_attribute("srcset");
        // Reset src explicitly: removing a picture's source candidates alone
        // can leave its image in the failed state even after currentSrc changes.
        let _ = image.remove_attribute("src");
        let _ = image.set_attribute("src", &fallback);
        return;
    }

    let Some(document) = image.owner_document() else {
        return;
    };
    let Some(parent) = image.parent_node() else {
        return;
    };
    let Ok(placeholder) = document.create_element("span") else {
        return;
    };
    let alt = image.alt();
    let label = if alt.trim().is_empty() {
        "Image unavailable".to_string()
    } else {
        format!("Image unavailable — {alt}")
    };
    placeholder.set_class_name("carrel-image-unavailable");
    let _ = placeholder.set_attribute("role", "img");
    let _ = placeholder.set_attribute("aria-label", &label);
    // Generated content keeps the label out of DOM text and highlight offsets.
    let _ = placeholder.set_attribute("data-label", &label);
    if parent.insert_before(&placeholder, Some(image)).is_ok() {
        let _ = image.set_attribute("data-carrel-image-unavailable", "");
        let _ = placeholder.append_child(image);
    }
}
