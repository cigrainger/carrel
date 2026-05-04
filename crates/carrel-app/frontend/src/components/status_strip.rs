//! Quiet application status surface.

use leptos::prelude::*;

use crate::theme::{Theme, use_theme_controller};

/// Bottom status strip.
#[component]
pub fn StatusStrip() -> impl IntoView {
    let theme = use_theme_controller();

    view! {
        <footer class="status-strip" aria-label="Status">
            <span>"Local store ready"</span>
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
