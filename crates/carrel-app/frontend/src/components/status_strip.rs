//! Quiet application status surface.

use carrel_core::keymap::{Action, Binding, Key, Layer, Sequence};
use leptos::{html, prelude::*};

use crate::feeds::use_feed_controller;
use crate::keymap::{use_action_handler, use_keymap_layer, use_keymap_warnings};
use crate::status::use_status_controller;
use crate::theme::{Theme, use_theme_controller};

/// Bottom status strip.
#[component]
pub fn StatusStrip() -> impl IntoView {
    let keymap_warnings = use_keymap_warnings();
    let feed_status = use_feed_controller().status();
    let app_status = use_status_controller().message();
    let controller = use_feed_controller();
    let details_button = NodeRef::<html::Button>::new();

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

        </footer>
    }
}

/// Quick typography adjustments beside the page being read.
#[component]
pub(crate) fn AppearanceMenu() -> impl IntoView {
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

    on_cleanup(move || {
        expanded.try_set(false);
    });
    view! {
            <details class="appearance-menu" node_ref=menu
                on:toggle=move |_| {
                    expanded.set(menu.get_untracked().is_some_and(|menu| menu.has_attribute("open")));
                }
            >
            <summary node_ref=trigger aria-label="Reading appearance" title="Reading appearance" aria-expanded=move || expanded.get().to_string() aria-controls="appearance-options"
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
                <span class="appearance-letter" aria-hidden="true">"Aa"</span>
            </summary>
            <Show when=move || expanded.get()><PopoverDismissal name="appearance" menu=menu on_close=close/></Show>
            <Show when=move || expanded.get()><div id="appearance-options" class="appearance-panel">
            <ThemeControls/>
            <crate::preferences::ReadingControls density=false/>
            </div></Show>
            </details>
    }
}

#[component]
pub(crate) fn PopoverDismissal(
    name: &'static str,
    menu: NodeRef<html::Details>,
    on_close: UnsyncCallback<bool>,
) -> impl IntoView {
    use_keymap_layer(Layer::new(
        name,
        name,
        vec![
            Binding::new(
                Sequence::single(Key::escape()),
                Action::named(name),
                "Close popover",
            )
            .allow_in_input(),
        ],
    ));
    use_action_handler(name, move || on_close.run(true));

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
        let focus_listener =
            Closure::<dyn FnMut(web_sys::FocusEvent)>::new(move |event: web_sys::FocusEvent| {
                let next = event
                    .related_target()
                    .and_then(|target| target.dyn_into::<web_sys::Node>().ok());
                // macOS WebKit can blur a summary without focusing a clicked
                // button. Only a known outside focus target dismisses the popover.
                if let Some(next) = next
                    && !menu
                        .get_untracked()
                        .is_some_and(|menu| menu.contains(Some(&next)))
                {
                    on_close.run(false);
                }
            });
        let element = menu.get_untracked();
        if let Some(element) = element.as_ref() {
            let _ = element.add_event_listener_with_callback(
                "focusout",
                focus_listener.as_ref().unchecked_ref(),
            );
        }
        let listener = StoredValue::new_local((document, listener, element, focus_listener));
        on_cleanup(move || {
            listener.try_with_value(|(document, listener, element, focus_listener)| {
                let _ = document.remove_event_listener_with_callback(
                    "pointerdown",
                    listener.as_ref().unchecked_ref(),
                );
                if let Some(element) = element {
                    let _ = element.remove_event_listener_with_callback(
                        "focusout",
                        focus_listener.as_ref().unchecked_ref(),
                    );
                }
            });
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = menu;
}

/// Theme choices shared by Settings and the reader popover.
#[component]
pub(crate) fn ThemeControls() -> impl IntoView {
    let theme = use_theme_controller();
    view! {
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
                            }
                        >
                            {option.label()}
                        </button>
                    }
                }).collect_view()}
            </div>
            </fieldset>
    }
}
