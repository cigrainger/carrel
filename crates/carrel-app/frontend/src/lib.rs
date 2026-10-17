//! Leptos webview for the Carrel desktop app.

#![deny(unsafe_code)]
#![recursion_limit = "256"]

mod annotation_actions;
pub mod api;
mod components;
mod desktop;
mod feeds;
mod highlights;
mod item_actions;
mod keymap;
mod links;
mod navigation;
mod preferences;
mod progress;
#[cfg(target_arch = "wasm32")]
mod reader_images;
mod requests;
mod routes;
mod selection;
mod sharing;
mod status;
mod theme;
mod writes;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::{Sidebar, StatusStrip};
use crate::keymap::{GlobalKeymapActions, KeymapProvider};

/// Root Leptos application.
#[component]
pub fn App() -> impl IntoView {
    provide_context(writes::PendingWrites::default());
    let theme = theme::provide_theme_controller();
    preferences::provide_preferences();
    navigation::provide_list_navigation_context();
    let reading_sidebar = navigation::use_reading_sidebar();
    feeds::provide_feed_controller();
    components::devices::provide_device_status();
    components::provide_context_bar();
    status::provide_status_controller();
    item_actions::provide_item_actions();
    annotation_actions::provide_annotation_actions();
    progress::provide_progress_writer();
    components::save_article::provide_capture_dialog();
    sharing::provide_sharing();
    desktop::install_close_handler();

    view! {
        <Router>
            <KeymapProvider>
                <GlobalKeymapActions/>
                <components::save_article::ArticleCapture/>
                <sharing::ShareDialog/>
                <components::FeedActivity/>
                <components::settings::Settings/>
                <div class="app-shell" class:reader-sidebar-open=move || reading_sidebar.0.get() data-theme=move || theme.current().get().as_str()>
                    <components::FieldNavigation/>
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
                            <Route path=path!("/feeds/:url") view=routes::FeedRoute/>
                            <Route path=path!("/folders/:folder") view=routes::FolderRoute/>
                            <Route path=path!("/item/:id") view=routes::ReadingView/>
                        </Routes>
                    </main>
                    <StatusStrip/>
                </div>
            </KeymapProvider>
        </Router>
    }
}
