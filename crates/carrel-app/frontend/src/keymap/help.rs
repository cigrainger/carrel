//! Live keymap reference overlay.

use carrel_core::keymap::Layer;
use leptos::prelude::*;

use crate::keymap::provider::use_keymap_context;

/// Modal help overlay for the currently active keymap stack.
#[component]
pub fn KeymapHelp() -> impl IntoView {
    let context = use_keymap_context();

    view! {
        <Show when=move || context.help_open.get()>
            <div class="keymap-help-backdrop">
                <section
                    class="keymap-help"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Keyboard reference"
                >
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
                    {move || {
                        context
                            .stack
                            .with(|stack| {
                                stack
                                    .layers()
                                    .iter()
                                    .rev()
                                    .cloned()
                                    .map(layer_view)
                                    .collect_view()
                            })
                    }}
                </section>
            </div>
        </Show>
    }
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

            view! { <KeymapHelp/> }.to_html()
        });

        let expected = r#"<div class="keymap-help-backdrop"><section role="dialog" aria-modal="true" aria-label="Keyboard reference" class="keymap-help"><header class="keymap-help-header"><h2>Keyboard Reference</h2><button type="button" aria-label="Close keyboard reference">Close</button></header><section class="keymap-help-layer"><h3>Test Layer</h3><dl><div class="keymap-help-binding"><dt><kbd>x</kbd></dt><dd>Do thing</dd></div><!></dl></section><!></section></div>"#;

        assert_eq!(html, expected);
    }
}
