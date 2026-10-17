//! Recovery for an address that does not identify a current application view.

use leptos::prelude::*;
use leptos_router::components::A;

/// Keep an unknown route recoverable without restarting the desktop app.
#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <section class="route-panel">
            <h1>"Page not found"</h1>
            <p>"This address does not match a Carrel view."</p>
            <A href="/library">"Open Library"</A>
        </section>
    }
}
