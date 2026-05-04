//! Placeholder route surfaces for the first desktop skeleton.

use leptos::prelude::*;

/// Friends route placeholder.
#[component]
pub fn Friends() -> impl IntoView {
    placeholder("Friends")
}

/// Library route placeholder.
#[component]
pub fn Library() -> impl IntoView {
    placeholder("Library")
}

/// Highlights route placeholder.
#[component]
pub fn Highlights() -> impl IntoView {
    placeholder("Highlights")
}

/// Lists route placeholder.
#[component]
pub fn Lists() -> impl IntoView {
    placeholder("Lists")
}

/// Fallback route.
#[component]
pub fn NotFound() -> impl IntoView {
    placeholder("Not found")
}

fn placeholder(title: &'static str) -> impl IntoView {
    view! {
        <section class="route-panel">
            <h1>{title}</h1>
        </section>
    }
}
