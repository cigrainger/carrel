//! Primary desktop navigation.

use leptos::prelude::*;
use leptos_router::components::A;

/// Sidebar navigation root.
#[component]
pub fn Sidebar() -> impl IntoView {
    view! {
        <aside class="sidebar" aria-label="Primary">
            <div class="sidebar-brand">"Carrel"</div>
            <nav class="sidebar-nav">
                <A href="/today">"Today"</A>
                <A href="/friends">"Friends"</A>
                <A href="/library">"Library"</A>
                <A href="/highlights">"Highlights"</A>
                <A href="/lists">"Lists"</A>
            </nav>
            <div class="sidebar-section">
                <h2>"Feeds"</h2>
                <p>"No feeds yet"</p>
            </div>
        </aside>
    }
}
