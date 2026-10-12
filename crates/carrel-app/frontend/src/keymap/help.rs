//! Live keymap reference overlay.

use carrel_core::keymap::Layer;
use leptos::prelude::*;

use crate::components::Modal;
use crate::keymap::provider::use_keymap_context;

/// Modal help overlay for the currently active keymap stack.
#[component]
pub fn KeymapHelp() -> impl IntoView {
    let context = use_keymap_context();

    view! {
        <Show when=move || context.help_open.get()>
                <Modal label="Keyboard reference" class="keymap-help" on_cancel=UnsyncCallback::new(move |()| context.help_open.set(false))>
                    <header class="keymap-help-header">
                        <h2>"Keyboard Reference"</h2>
                        <button
                            type="button"
                            aria-label="Close keyboard reference"
                            on:click=move |_| context.help_open.set(false)
                        >
                            "Close"
                        </button>
                    </header>
                    <KeyboardReference/>
                </Modal>
        </Show>
    }
}

/// The effective bindings, shared by help and the Settings keyboard section.
#[component]
pub(crate) fn KeyboardReference() -> impl IntoView {
    let context = use_keymap_context();
    view! { {move || context.available_layers().into_iter().map(layer_view).collect_view()} }
}

fn layer_view(layer: Layer) -> impl IntoView {
    let name = layer.name().to_string();
    let bindings = layer.bindings().to_vec();

    view! {
        <section class="keymap-help-layer">
            <h3>{name}</h3>
            <dl>
                {bindings
                    .into_iter()
                    .map(|binding| {
                        let sequence = binding.sequence.to_string();
                        let description = binding.description;

                        view! {
                            <div class="keymap-help-binding">
                                <dt><kbd>{sequence}</kbd></dt>
                                <dd>{description}</dd>
                            </div>
                        }
                    })
                    .collect_view()}
            </dl>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::keymap::{Action, Binding, Key, KeymapStack, Layer, Sequence};

    use super::*;
    use crate::keymap::provider::KeymapContext;

    #[test]
    fn help_overlay_renders_active_bindings() {
        let html = Owner::new().with(|| {
            let context = KeymapContext::new();
            let mut stack = KeymapStack::new();
            stack
                .push(Layer::new(
                    "test",
                    "Test Layer",
                    vec![Binding::new(
                        Sequence::single(Key::character('x')),
                        Action::named("test-action"),
                        "Do thing",
                    )],
                ))
                .unwrap();
            context.stack.set(stack);
            context.help_open.set(true);
            provide_context(context);
            crate::keymap::use_action_handler("test-action", || {});

            view! { <KeymapHelp/> }.to_html()
        });

        assert!(html.contains("<dialog"));
        assert!(html.contains("aria-label=\"Keyboard reference\""));
        assert!(html.contains("<kbd>x</kbd>"));
        assert!(html.contains("Do thing"));
    }
}
