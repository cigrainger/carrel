//! Reader link presentation preserves the cached article and its context.

use crate::status::StatusController;

pub(crate) fn open_original(url: String, status: StatusController) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        if crate::api::open_external_url(url).await.is_err() {
            status.warn("Could not open that link in your browser.");
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (url, status);
}

pub(crate) fn article_link(
    event: leptos::ev::MouseEvent,
    body_ref: leptos::prelude::NodeRef<leptos::html::Div>,
    original: Option<&str>,
    status: StatusController,
) {
    #[cfg(target_arch = "wasm32")]
    {
        use leptos::prelude::*;
        use wasm_bindgen::JsCast;
        if !matches!(event.button(), 0 | 1) {
            return;
        }
        let Some(anchor) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|element| element.closest("a[href]").ok().flatten())
        else {
            return;
        };
        let Some(body) = body_ref.get_untracked() else {
            return;
        };
        if !body.contains(Some(&anchor)) {
            return;
        }
        let href = anchor.get_attribute("href").unwrap_or_default();
        event.prevent_default();
        event.stop_propagation();
        if let Some(fragment) = cached_fragment(&href, original) {
            if let Some(target) = body
                .owner_document()
                .and_then(|document| document.get_element_by_id(fragment))
                && body.contains(Some(&target))
            {
                target.scroll_into_view();
                return;
            }
            if href.starts_with('#') {
                status.info("That section is not in the cached article.");
                return;
            }
        }
        open_original(href, status);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (event, body_ref, original, status);
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
fn cached_fragment<'a>(href: &'a str, original: Option<&str>) -> Option<&'a str> {
    let (base, fragment) = href.split_once('#')?;
    let original = original.map(|value| value.split('#').next().unwrap_or(value));
    (!fragment.is_empty() && (base.is_empty() || original == Some(base))).then_some(fragment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_sections_stay_in_the_reader_but_other_articles_open_externally() {
        let original = Some("https://example.com/article#old");
        assert_eq!(cached_fragment("#footnote", original), Some("footnote"));
        assert_eq!(
            cached_fragment("https://example.com/article#footnote", original),
            Some("footnote")
        );
        for href in [
            "https://example.com/another#footnote",
            "https://other.example/article#footnote",
            "#",
            "https://example.com/article",
        ] {
            assert_eq!(cached_fragment(href, original), None);
        }
        assert_eq!(cached_fragment("#footnote", None), Some("footnote"));
    }
}
