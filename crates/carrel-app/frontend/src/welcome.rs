//! One optional first-run sheet, separate from the user's reading data.

use leptos::prelude::*;

use crate::components::{Icon, IconName, Modal};
use crate::feeds::{self, use_feed_controller};
use crate::requests::Requests;
use crate::status::{StatusController, use_status_controller};

#[derive(Clone, Copy)]
pub(crate) struct WelcomeController {
    open: RwSignal<bool>,
    considered: RwSignal<bool>,
    status: StatusController,
}

impl WelcomeController {
    pub(crate) fn show(self) {
        self.considered.set(true);
        self.open.set(true);
    }

    fn dismiss(self) {
        self.considered.set(true);
        self.open.set(false);
        #[cfg(target_arch = "wasm32")]
        {
            let remembered = web_sys::window()
                .and_then(|window| window.local_storage().ok().flatten())
                .is_some_and(|storage| storage.set_item("carrel.welcome.v1", "done").is_ok());
            if !remembered {
                self.status
                    .warn("Could not remember your welcome choice on this desktop.");
            }
        }
    }
}

pub(crate) fn provide_welcome() {
    #[cfg(target_arch = "wasm32")]
    let completed = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("carrel.welcome.v1").ok().flatten())
        .as_deref()
        == Some("done");
    #[cfg(not(target_arch = "wasm32"))]
    let completed = false;
    provide_context(WelcomeController {
        open: RwSignal::new(false),
        considered: RwSignal::new(completed),
        status: use_status_controller(),
    });
}

pub(crate) fn use_welcome() -> WelcomeController {
    expect_context()
}

#[component]
pub(crate) fn Welcome() -> impl IntoView {
    let controller = use_welcome();
    #[cfg(target_arch = "wasm32")]
    {
        use leptos_router::hooks::use_location;
        let location = use_location();
        let feeds = use_feed_controller();
        if !controller.considered.get_untracked() {
            let guard = Requests::default();
            let cleanup = guard.clone();
            on_cleanup(move || cleanup.invalidate());
            let request = guard.begin();
            wasm_bindgen_futures::spawn_local(async move {
                let eligible = crate::api::welcome_is_needed().await;
                if !guard.accepts(request) || controller.considered.get_untracked() {
                    return;
                }
                controller.considered.set(true);
                let unobstructed = web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| {
                        document
                            .query_selector("dialog[open]")
                            .ok()
                            .flatten()
                            .is_none()
                    });
                let starting = matches!(
                    location.pathname.get_untracked().as_str(),
                    "/" | "/today" | "/library"
                );
                if matches!(eligible, Ok(true))
                    && starting
                    && unobstructed
                    && feeds.feeds().get_untracked().is_empty()
                {
                    controller.open.set(true);
                }
            });
        }
    }
    view! { <Show when=move || controller.open.get()><WelcomeSheet/></Show> }
}

#[component]
fn WelcomeSheet() -> impl IntoView {
    let controller = use_welcome();
    let feeds = use_feed_controller();
    let capture = crate::components::save_article::use_capture_dialog();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let close = UnsyncCallback::new(move |()| controller.dismiss());
    view! {
        <Modal label="Welcome to Carrel" class="feed-dialog welcome-dialog" on_cancel=close>
            <h2>"Welcome to Carrel"</h2>
            <p>"Your archive lives on this computer. Start with a source, a saved page or subscriptions from another reader."</p>
            <div class="welcome-choices">
                <button type="button" disabled=move || busy.get() on:click=move |_| { controller.dismiss(); feeds.add_open.set(true); }><Icon name=IconName::Add/><span><strong>"Add source"</strong><small>"Follow a website or feed."</small></span></button>
                <button type="button" disabled=move || busy.get() on:click=move |_| { controller.dismiss(); capture.0.set(true); }><Icon name=IconName::Library/><span><strong>"Save article"</strong><small>"Keep a page to read offline."</small></span></button>
                <button type="button" disabled=move || busy.get() || feeds.file_busy.get() on:click=move |_| {
                    busy.set(true); feeds.file_busy.set(true); error.set(None);
                    let guard = requests.get_value(); let request = guard.begin();
                    spawn(async move {
                        let result = feeds::subscription_file(feeds, true).await;
                        feeds.file_busy.try_set(false);
                        if !guard.accepts(request) {
                            if let Err(message) = result { controller.status.warn(format!("Could not import subscriptions: {message}")); }
                            return;
                        }
                        busy.try_set(false);
                        match result {
                            Ok(true) => controller.dismiss(),
                            Ok(false) => {},
                            Err(message) => { error.try_set(Some(message)); },
                        }
                    });
                }><Icon name=IconName::Import/><span><strong>"Import OPML"</strong><small>"Bring subscriptions from another reader."</small></span></button>
            </div>
            {move || error.get().map(|message| view! { <p class="feed-error" role="alert">{message}</p> })}
            <p class="welcome-note">"Sync and sharing are optional. You can set them up later in Settings."</p>
            <div class="feed-dialog-actions"><button type="button" on:click=move |_| close.run(())>"Not now"</button></div>
        </Modal>
    }
}

fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
    #[cfg(not(target_arch = "wasm32"))]
    drop(future);
}
