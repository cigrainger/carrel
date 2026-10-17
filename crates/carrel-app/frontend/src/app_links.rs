//! Private addresses and native delivery, independent of the current route.

use carrel_core::app_link::AppLink;
use leptos::{html, prelude::*};

use crate::{components::Modal, requests::Requests, status::StatusController};

#[derive(Clone, Copy)]
pub(crate) struct AppLinksController {
    pub(crate) activation: RwSignal<u64>,
    fallback: RwSignal<Option<CopyLink>>,
    copies: StoredValue<Requests>,
    status: StatusController,
}

#[derive(Clone)]
struct CopyLink {
    text: String,
    private: bool,
}

impl AppLinksController {
    pub(crate) fn copy(self, link: AppLink) {
        self.copy_link(link.to_string(), true);
    }

    pub(crate) fn copy_original(self, url: String) {
        self.copy_link(url, false);
    }

    fn copy_link(self, text: String, private: bool) {
        let copy = crate::clipboard::start_copy(&text);
        let guard = self.copies.get_value();
        let generation = guard.begin();
        leptos::task::spawn_local(async move {
            let copied = crate::clipboard::finish_copy(copy).await;
            if !guard.accepts(generation) {
                return;
            }
            if copied {
                self.status.info(if private {
                    "Carrel link copied."
                } else {
                    "Link copied."
                });
            } else {
                self.fallback.set(Some(CopyLink { text, private }));
            }
        });
    }
}

pub(crate) fn provide_app_links() {
    let copies = Requests::default();
    let cleanup = copies.clone();
    on_cleanup(move || cleanup.invalidate());
    provide_context(AppLinksController {
        activation: RwSignal::new(0),
        fallback: RwSignal::new(None),
        copies: StoredValue::new(copies),
        status: crate::status::use_status_controller(),
    });
}

pub(crate) fn use_app_links() -> AppLinksController {
    expect_context()
}

/// Leaf views can also render in isolation, without an application controller.
#[component]
pub(crate) fn CopyLinkButton(#[prop(into)] link: Signal<Option<AppLink>>) -> impl IntoView {
    let controller = use_context::<AppLinksController>();
    view! {
        <button type="button" disabled=move || controller.is_none() || link.get().is_none()
            on:click=move |event| {
                event.stop_propagation();
                if let (Some(controller), Some(link)) = (controller, link.get_untracked()) {
                    controller.copy(link);
                }
            }>"Copy Carrel link"</button>
    }
}

#[component]
pub(crate) fn AppLinks() -> impl IntoView {
    let controller = use_app_links();
    #[cfg(target_arch = "wasm32")]
    install_delivery(controller);
    view! {
        <Show when=move || controller.fallback.get().is_some()>
            <CopyLinkDialog link=controller.fallback.get().expect("copy target")
                on_close=UnsyncCallback::new(move |()| controller.fallback.set(None))/>
        </Show>
    }
}

#[component]
fn CopyLinkDialog(link: CopyLink, on_close: UnsyncCallback<()>) -> impl IntoView {
    let label = if link.private {
        "Copy Carrel link"
    } else {
        "Copy link"
    };
    let field_label = if link.private {
        "Carrel link"
    } else {
        "Article link"
    };
    let field = NodeRef::<html::Textarea>::new();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if let Some(field) = field.get() {
            // Native dialog autofocus can reset a selection made before it
            // enters the top layer. Select after that focus step has finished.
            set_timeout(
                move || {
                    if field.is_connected() {
                        let _ = field.focus();
                        field.select();
                    }
                },
                std::time::Duration::ZERO,
            );
        }
    });
    view! {
        <Modal label=label class="note-dialog" on_cancel=on_close>
            <h2>{label}</h2>
            <p>"Clipboard access is unavailable. Copy the selected link with your usual copy shortcut."</p>
            <label for="carrel-link-copy">{field_label}</label>
            <textarea id="carrel-link-copy" node_ref=field rows="3" readonly=true autofocus=true
                prop:value=link.text on:focus=move |_| { if let Some(field) = field.get() { field.select(); } }/>
            <p>{if link.private { "This opens on desktops where the article or highlight is already saved. It does not share access." } else { "This opens the publisher’s original webpage." }}</p>
            <div class="note-dialog-actions"><button type="button" on:click=move |_| on_close.run(())>"Done"</button></div>
        </Modal>
    }
}

#[cfg(target_arch = "wasm32")]
fn install_delivery(controller: AppLinksController) {
    use crate::api::AppLinkResolution;
    use leptos_router::{
        NavigateOptions,
        hooks::{use_location, use_navigate},
    };

    let location = use_location();
    let navigate = use_navigate();
    let navigation = crate::navigation::use_list_navigation_context();
    let preferences = crate::preferences::use_preferences();
    let (_, remember_list) = crate::keymap::action("remember-list-context");
    let apply = UnsyncCallback::new(move |resolution: AppLinkResolution| {
        match resolution {
            AppLinkResolution::Open { path } => {
                remember_list.run(());
                if let Some(id) = path
                    .strip_prefix("/item/")
                    .and_then(|rest| rest.split('#').next())
                {
                    let query = location.search.get_untracked();
                    let mut origin = location.pathname.get_untracked();
                    if !query.is_empty() {
                        origin.push('?');
                        origin.push_str(&query);
                    }
                    navigation.enter_link(id, &origin);
                }
                preferences.appearance_open.set(false);
                navigate(
                    &path,
                    NavigateOptions {
                        scroll: false,
                        ..Default::default()
                    },
                );
                // Hash-only and repeated identical links must refocus too.
                controller.activation.update(|value| *value += 1);
                controller.status.info("Opened Carrel link.");
            }
            AppLinkResolution::MissingItem => controller
                .status
                .warn("That article is not saved on this desktop."),
            AppLinkResolution::MissingHighlight => controller
                .status
                .warn("That highlight is missing or was removed on this desktop."),
            AppLinkResolution::Invalid => {
                controller.status.warn("That Carrel link is not supported.")
            }
        }
    });

    let wake = RwSignal::new(0_u64);
    let fetching = RwSignal::new(false);
    let pending = RwSignal::new(None::<AppLinkResolution>);
    let lifetime = Requests::default();
    let alive = lifetime.begin();
    let delivery = Requests::default();
    let cleanup_lifetime = lifetime.clone();
    let cleanup_delivery = delivery.clone();
    on_cleanup(move || {
        cleanup_lifetime.invalidate();
        cleanup_delivery.invalidate();
    });

    let listener = window_event_listener(
        leptos::ev::Custom::<web_sys::Event>::new("carrel-link-ready"),
        move |_| {
            pending.set(None);
            wake.update(|value| *value += 1);
        },
    );
    on_cleanup(move || listener.remove());
    Effect::new(move |_| {
        let generation = delivery.begin();
        if let Some(resolution) = pending.get() {
            deliver_when_ready(
                resolution,
                apply,
                controller.status,
                delivery.clone(),
                generation,
                false,
            );
        }
    });

    // One consumer at a time: a wake during a query invalidates its result and
    // schedules another read. The native inbox retains only the latest intent.
    Effect::new(move |_| {
        let requested = wake.get();
        if fetching.get_untracked() {
            return;
        }
        fetching.set(true);
        let lifetime = lifetime.clone();
        leptos::task::spawn_local(async move {
            let result = crate::api::take_app_link().await;
            if !lifetime.accepts(alive) {
                return;
            }
            let newer = wake.get_untracked() != requested;
            if !newer {
                match result {
                    Ok(value) => pending.set(value),
                    Err(_) => controller
                        .status
                        .warn("Could not open that Carrel link. Try it again."),
                }
            }
            // Do not track fetching=false as a reason to poll an empty inbox.
            fetching.set(false);
            if newer {
                wake.update(|value| *value += 1);
            }
        });
    });
}

/// A link arriving over a dialog waits for an explicit close. Checking the
/// mounted element also covers the frame before native show_modal runs.
#[cfg(target_arch = "wasm32")]
fn deliver_when_ready(
    resolution: crate::api::AppLinkResolution,
    apply: UnsyncCallback<crate::api::AppLinkResolution>,
    status: StatusController,
    guard: Requests,
    generation: u64,
    waiting: bool,
) {
    set_timeout(
        move || {
            if !guard.accepts(generation) {
                return;
            }
            let dialog = web_sys::window()
                .and_then(|window| window.document())
                .is_some_and(|document| document.query_selector("dialog").ok().flatten().is_some());
            if dialog {
                if !waiting {
                    status.info(
                        "Carrel link is waiting. Close the current dialog when you are ready.",
                    );
                }
                deliver_when_ready(resolution, apply, status, guard, generation, true);
            } else {
                apply.run(resolution);
            }
        },
        if waiting {
            std::time::Duration::from_millis(250)
        } else {
            std::time::Duration::ZERO
        },
    );
}
