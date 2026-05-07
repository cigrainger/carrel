//! Quiet application status surface.

use leptos::prelude::*;

use crate::feeds::use_feed_controller;
use crate::keymap::use_keymap_warnings;
use crate::theme::{Theme, use_theme_controller};

/// Bottom status strip.
#[component]
pub fn StatusStrip() -> impl IntoView {
    let theme = use_theme_controller();
    let keymap_warnings = use_keymap_warnings();
    let feed_status = use_feed_controller().status();

    view! {
        <footer class="status-strip" aria-label="Status">
            {move || {
                keymap_warnings
                    .and_then(|warnings| warnings.get().first().cloned())
                    .map(|warning| view! { <span class="status-warning">{warning}</span> }.into_any())
                    .unwrap_or_else(|| {
                        let status = feed_status.get();
                        let class = if status.feed_errors + status.entry_errors + status.content_errors > 0 {
                            "status-warning"
                        } else {
                            "status-message"
                        };
                        view! { <span class=class>{status.message}</span> }.into_any()
                    })
            }}
            <div class="theme-switcher" role="group" aria-label="Theme">
                {Theme::ALL.into_iter().map(|option| {
                    let current = theme.current();

                    view! {
                        <button
                            type="button"
                            aria-pressed=move || if current.get() == option { "true" } else { "false" }
                            on:click=move |_| theme.set(option)
                        >
                            {option.label()}
                        </button>
                    }
                }).collect_view()}
            </div>
        </footer>
    }
}
