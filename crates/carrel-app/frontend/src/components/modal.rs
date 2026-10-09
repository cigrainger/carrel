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
