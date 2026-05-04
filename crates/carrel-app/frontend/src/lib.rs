//! Leptos webview for the Carrel desktop app.

#![deny(unsafe_code)]

pub mod api;
mod components;
mod routes;
mod theme;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::{Sidebar, StatusStrip};

/// Root Leptos application.
#[component]
pub fn App() -> impl IntoView {
    let theme = theme::provide_theme_controller();

    view! {
        <Router>
            <div class="app-shell" data-theme=move || theme.current().get().as_str()>
                <Sidebar/>
                <main class="main-content" aria-label="Main content">
                    <Routes fallback=|| view! { <routes::NotFound/> }>
                        <Route path=path!("") view=routes::Today/>
                        <Route path=path!("/today") view=routes::Today/>
                        <Route path=path!("/friends") view=routes::Friends/>
                        <Route path=path!("/library") view=routes::Library/>
                        <Route path=path!("/highlights") view=routes::Highlights/>
                        <Route path=path!("/lists") view=routes::Lists/>
                    </Routes>
                </main>
                <StatusStrip/>
            </div>
        </Router>
    }
}
