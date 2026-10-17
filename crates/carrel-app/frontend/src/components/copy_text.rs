//! Explicit clipboard writes with a complete, stable manual-copy fallback.

use crate::{
    clipboard::{finish_copy, start_copy},
    requests::Requests,
};
use leptos::{html, prelude::*};

#[component]
pub(crate) fn CopyTextField(
    id: &'static str,
    label: &'static str,
    button: &'static str,
    text: Signal<String>,
    #[prop(default = false)] focus_on_mount: bool,
) -> impl IntoView {
    let field = NodeRef::<html::Textarea>::new();
    let displayed = RwSignal::new(text.get_untracked());
    let focused = RwSignal::new(false);
    let message = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    Effect::new(move |_| {
        let next = text.get();
        if !focused.get() && displayed.get_untracked() != next {
            displayed.set(next);
            message.set(None);
        }
    });
    #[cfg(target_arch = "wasm32")]
    if focus_on_mount {
        Effect::new(move |_| {
            if let Some(field) = field.get() {
                set_timeout(
                    move || {
                        if field.is_connected() {
                            let _ = field.focus();
                            field.select();
                        }
                    },
                    std::time::Duration::ZERO,
                );
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = focus_on_mount;
    view! { <div class="contact-copy"><label for=id>{label}</label><textarea node_ref=field id=id rows="2" readonly=true prop:value=move ||displayed.get() on:focus=move |_| {focused.set(true);if let Some(area)=field.get(){area.select();}} on:blur=move |_|focused.set(false)/>
        <button type="button" disabled=move ||busy.get() || displayed.get().is_empty() on:click=move |_| {
            let guard=requests.get_value();let request=guard.begin();busy.set(true);message.set(None);
            // Start within pointer/keyboard activation, before awaiting anything.
            let copy=start_copy(&displayed.get_untracked());
            leptos::task::spawn_local(async move {
                let copied=finish_copy(copy).await;
                if !guard.accepts(request) {return;}
                if copied {message.try_set(Some(format!("{label} copied.")));}
                else {if let Some(area)=field.get_untracked(){let _=area.focus();area.select();}message.try_set(Some("Clipboard access is unavailable. The complete text is selected; use Copy from the Edit menu or your usual copy shortcut.".into()));}
                busy.try_set(false);
            });
        }>{button}</button>{move ||message.get().map(|text|view!{<p role="status" class="copy-status">{text}</p>})}
    </div> }
}
