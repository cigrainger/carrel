//! Native modal behavior keeps the background inert and restores focus.

use leptos::html;
use leptos::prelude::*;

#[component]
pub(crate) fn Modal(
    label: &'static str,
    class: &'static str,
    on_cancel: UnsyncCallback<()>,
    children: Children,
) -> impl IntoView {
    let dialog = NodeRef::<html::Dialog>::new();
    #[cfg(target_arch = "wasm32")]
    {
        Effect::new(move |_| {
            if let Some(dialog) = dialog.get() {
                let _ = dialog.show_modal();
            }
        });
        on_cleanup(move || {
            if let Some(dialog) = dialog.try_get_untracked().flatten() {
                dialog.close();
            }
            // A successful retry can remove the warning control that opened
            // this dialog. Keep keyboard access in the surviving reading view.
            if let Some(document) = web_sys::window().and_then(|window| window.document())
                && document
                    .active_element()
                    .is_none_or(|element| element.tag_name() == "BODY")
            {
                use wasm_bindgen::JsCast;
                for selector in [
                    ".is-selected .item-open",
                    ".is-selected .highlight-open",
                    ".reader-actions button",
                    ".field-destinations a.is-active",
                ] {
                    if let Ok(Some(element)) = document.query_selector(selector)
                        && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
                    {
                        let options = web_sys::FocusOptions::new();
                        options.set_prevent_scroll(true);
                        let _ = element.focus_with_options(&options);
                        break;
                    }
                }
            }
        });
    }
    view! {
        <dialog
            node_ref=dialog
            class=class
            aria-label=label
            on:cancel=move |event: leptos::ev::Event| {
                event.prevent_default();
                on_cancel.run(());
            }
            on:keydown=move |event| {
                if event.key() == "Escape" && !event.default_prevented() {
                    event.prevent_default();
                    event.stop_propagation();
                    on_cancel.run(());
                }
            }
        >
            {children()}
        </dialog>
    }
}
