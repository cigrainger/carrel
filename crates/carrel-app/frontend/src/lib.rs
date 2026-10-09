//! Leptos webview for the Carrel desktop app.

#![deny(unsafe_code)]
#![recursion_limit = "256"]

pub mod api;
mod components;
mod feeds;
mod highlights;
mod item_actions;
mod keymap;
mod navigation;
mod progress;
mod requests;
mod routes;
mod status;
mod theme;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::{Sidebar, StatusStrip};
use crate::keymap::{GlobalKeymapActions, KeymapProvider};

/// Root Leptos application.
#[component]
pub fn App() -> impl IntoView {
    let theme = theme::provide_theme_controller();
    navigation::provide_list_navigation_context();
    let reading_sidebar = navigation::use_reading_sidebar();
    feeds::provide_feed_controller();
    status::provide_status_controller();
    item_actions::provide_item_actions();
    progress::provide_progress_writer();

    view! {
        <Router>
            <KeymapProvider>
                <GlobalKeymapActions/>
                <div class="app-shell" class:reader-sidebar-open=move || reading_sidebar.0.get() data-theme=move || theme.current().get().as_str()>
                    <Sidebar/>
                    <main class="main-content" id="main-content" aria-label="Main content">
                        <Routes fallback=|| view! { <routes::NotFound/> }>
                            <Route path=path!("") view=routes::Today/>
                            <Route path=path!("/today") view=routes::Today/>
                            <Route path=path!("/friends") view=routes::Friends/>
                            <Route path=path!("/library") view=routes::Library/>
                            <Route path=path!("/unread") view=routes::Unread/>
                            <Route path=path!("/reading") view=routes::Reading/>
                            <Route path=path!("/starred") view=routes::Starred/>
                            <Route path=path!("/highlights") view=routes::Highlights/>
                            <Route path=path!("/lists") view=routes::Lists/>
                            <Route path=path!("/feeds/:url") view=routes::FeedRoute/>
                            <Route path=path!("/item/:id") view=routes::ReadingView/>
                        </Routes>
                    </main>
                    <StatusStrip/>
                </div>
            </KeymapProvider>
        </Router>
    }
}
