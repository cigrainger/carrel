//! Quiet application status surface.

use carrel_core::keymap::{Action, Binding, Key, Layer, Sequence};
use leptos::{html, prelude::*};

use crate::components::{Icon, IconName};
use crate::feeds::use_feed_controller;
use crate::keymap::{use_action_handler, use_keymap_layer, use_keymap_warnings};
use crate::status::use_status_controller;
use crate::theme::{Theme, use_theme_controller};

/// Bottom status strip.
#[component]
pub fn StatusStrip() -> impl IntoView {
    let theme = use_theme_controller();
    let keymap_warnings = use_keymap_warnings();
    let feed_status = use_feed_controller().status();
    let app_status = use_status_controller().message();
    let controller = use_feed_controller();
    let details_button = NodeRef::<html::Button>::new();
    let menu = NodeRef::<html::Details>::new();
    let trigger = NodeRef::<html::Summary>::new();
    let expanded = crate::preferences::use_preferences().appearance_open;
    Effect::new(move |_| {
        let open = expanded.get();
        if let Some(menu) = menu.get() {
            if open {
                let _ = menu.set_attribute("open", "");
            } else {
                let _ = menu.remove_attribute("open");
            }
        }
    });
    let close = UnsyncCallback::new(move |restore_focus: bool| {
        if let Some(menu) = menu.get_untracked() {
            let _ = menu.remove_attribute("open");
        }
        expanded.set(false);
        if restore_focus && let Some(trigger) = trigger.get_untracked() {
            let _ = trigger.focus();
        }
    });

    view! {
        <footer class="status-strip" aria-label="Status">
            {move || {
                keymap_warnings
                    .and_then(|warnings| warnings.get().first().cloned())
                    .map(|warning| {
                        let title = warning.clone();
                        view! { <span class="status-warning" title=title>{warning}</span> }.into_any()
                    })
                    .unwrap_or_else(|| {
                        if let Some(message) = app_status.get() {
                            let class = if message.warning {
                                "status-warning"
                            } else {
                                "status-message"
                            };
                            let title = message.text.clone();
                            return view! { <span class=class title=title>{message.text}</span> }.into_any();
                        }
                        let status = feed_status.get();
                        let class = if status.feed_errors + status.entry_errors + status.content_errors > 0 {
                            "status-warning"
                        } else {
                            "status-message"
                        };
                        let title = status.message.clone();
                        view! { <span class=class title=title>{status.message}</span> }.into_any()
                    })
            }}
            <Show when=move || { let status = feed_status.get(); !status.issues.is_empty() || status.feed_errors + status.entry_errors + status.content_errors > 0 }>
                <button node_ref=details_button class="status-details" type="button" on:click=move |_| {
                    if let Some(button) = details_button.get_untracked() { let _ = button.focus(); }
                    controller.activity_open.set(true);
                }>"Details"</button>
            </Show>
            <details class="appearance-menu" node_ref=menu
                on:toggle=move |_| {
                    expanded.set(menu.get_untracked().is_some_and(|menu| menu.has_attribute("open")));
                }
                on:focusout=move |event| {
                    #[cfg(target_arch = "wasm32")]
                    {
                        use wasm_bindgen::JsCast;
                        let next = event.related_target().and_then(|target| target.dyn_into::<web_sys::Node>().ok());
                        // macOS WebKit can blur the summary without focusing a
                        // clicked theme button. Only a known outside focus
                        // target dismisses; pointer dismissal is handled below.
                        if let Some(next) = next
                            && !menu.get_untracked().is_some_and(|menu| menu.contains(Some(&next)))
                        {
                            close.run(false);
                        }
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = event;
                }
            >
            <summary node_ref=trigger aria-expanded=move || expanded.get().to_string() aria-controls="appearance-options"
                on:click=move |event| {
                    // Register dismissal with the opening action, before the
                    // native details element's asynchronous toggle event.
                    event.prevent_default();
                    if expanded.get_untracked() {
                        close.run(false);
                    } else if let Some(menu) = menu.get_untracked() {
                        let _ = menu.set_attribute("open", "");
                        expanded.set(true);
                    }
                }
            >
                <Icon name=IconName::Appearance/><span>"Appearance"</span>
            </summary>
            <Show when=move || expanded.get()><AppearanceDismissal menu=menu on_close=close/></Show>
            <div id="appearance-options" class="appearance-panel">
            <fieldset class="appearance-theme"><legend>"Theme"</legend>
            <div class="theme-switcher" role="group" aria-label="Theme">
                {Theme::ALL.into_iter().map(|option| {
                    let current = theme.current();

                    view! {
                        <button
                            type="button"
                            aria-pressed=move || if current.get() == option { "true" } else { "false" }
                            on:click=move |_| {
                                theme.set(option);
                                close.run(true);
                            }
                        >
                            {option.label()}
                        </button>
                    }
                }).collect_view()}
            </div>
            </fieldset>
            <crate::preferences::ReadingControls/>
            </div>
            </details>
        </footer>
    }
}

#[component]
fn AppearanceDismissal(
    menu: NodeRef<html::Details>,
    on_close: UnsyncCallback<bool>,
) -> impl IntoView {
    use_keymap_layer(Layer::new(
        "appearance",
        "Appearance",
        vec![
            Binding::new(
                Sequence::single(Key::escape()),
                Action::named("close-appearance"),
                "Close appearance",
            )
            .allow_in_input(),
        ],
    ));
    use_action_handler("close-appearance", move || on_close.run(true));

    #[cfg(target_arch = "wasm32")]
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        use wasm_bindgen::{JsCast, closure::Closure};

        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let target = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Node>().ok());
            if !menu
                .get_untracked()
                .is_some_and(|menu| menu.contains(target.as_ref()))
            {
                on_close.run(false);
            }
        });
        let _ = document
            .add_event_listener_with_callback("pointerdown", listener.as_ref().unchecked_ref());
        let listener = StoredValue::new_local((document, listener));
        on_cleanup(move || {
            listener.try_with_value(|(document, listener)| {
                let _ = document.remove_event_listener_with_callback(
                    "pointerdown",
                    listener.as_ref().unchecked_ref(),
                );
            });
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = menu;
}
