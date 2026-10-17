//! Quiet, explicit desktop pairing alongside subscription management.

use leptos::prelude::*;
use serde_json::json;

use crate::{api, feeds};

/// Installation status is observed even when its settings section is closed.
#[derive(Clone, Copy)]
pub(crate) struct DeviceStatus {
    snapshot: RwSignal<api::SyncSnapshot>,
    #[cfg(target_arch = "wasm32")]
    feeds: feeds::FeedController,
    #[cfg(target_arch = "wasm32")]
    requests: StoredValue<crate::requests::Requests>,
}

impl DeviceStatus {
    pub(crate) fn refresh(self) {
        #[cfg(target_arch = "wasm32")]
        {
            let requests = self.requests.get_value();
            let request = requests.begin();
            let feeds = self.feeds;
            leptos::task::spawn_local(async move {
                if let Ok(current) = api::sync_status().await {
                    if !requests.accepts(request) {
                        return;
                    }
                    let changed = current.revision != self.snapshot.get_untracked().revision;
                    self.snapshot.set(current.clone());
                    if changed {
                        feeds.sync_revision.set(current.revision);
                        feeds::refresh_from_sync(feeds).await;
                    }
                }
            });
        }
    }
}

pub(crate) fn provide_device_status() {
    let status = DeviceStatus {
        snapshot: RwSignal::new(api::SyncSnapshot::default()),
        #[cfg(target_arch = "wasm32")]
        feeds: feeds::use_feed_controller(),
        #[cfg(target_arch = "wasm32")]
        requests: StoredValue::new(crate::requests::Requests::default()),
    };
    provide_context(status);
    #[cfg(target_arch = "wasm32")]
    {
        status.refresh();
        let requests = status.requests.get_value();
        on_cleanup(move || requests.invalidate());
        if let Ok(interval) =
            set_interval_with_handle(move || status.refresh(), std::time::Duration::from_secs(3))
        {
            on_cleanup(move || interval.clear());
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Overview,
    Setup,
    Join,
    Invite,
    Revoke,
}

#[component]
pub(crate) fn DevicesPanel() -> impl IntoView {
    let mode = RwSignal::new(Mode::Overview);
    let status = expect_context::<DeviceStatus>();
    let snapshot = status.snapshot;
    let name = RwSignal::new(String::new());
    let passphrase = RwSignal::new(String::new());
    let confirm = RwSignal::new(String::new());
    let invitation = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let selected_device = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let feeds = feeds::use_feed_controller();
    let requests = StoredValue::new(crate::requests::Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    status.refresh();
    #[cfg(target_arch = "wasm32")]
    on_cleanup(move || {
        let text = invitation.try_get_untracked().unwrap_or_default();
        leptos::task::spawn_local(cancel_invitation(text));
    });
    let submit = move |command: &'static str, args: serde_json::Value| {
        busy.set(true);
        error.set(None);
        passphrase.set(String::new());
        confirm.set(String::new());
        let guard = requests.get_value();
        let request = guard.begin();
        leptos::task::spawn_local(async move {
            let result = api::sync_action(command, args).await;
            status.refresh();
            if !guard.accepts(request) {
                if command == "create_sync_invitation"
                    && let Ok(Some(text)) = result
                {
                    cancel_invitation(text).await;
                }
                return;
            }
            match result {
                Ok(text) => {
                    if let Some(text) = text {
                        invitation.set(text);
                    } else {
                        invitation.set(String::new());
                        mode.set(Mode::Overview);
                    }
                    feeds::refresh_from_sync(feeds).await;
                }
                Err(problem) => error.set(Some(problem.message().into())),
            }
            busy.try_set(false);
        });
    };

    view! {
        <section class="devices-panel">
                <h3>"Your desktops"</h3>
                <p>"Keep your reading library on your own desktops. Changes wait locally while a desktop is offline."</p>
                {move || error.get().or_else(|| snapshot.get().error).map(|text| view! {<p class="feed-error" role="alert">{text}</p>})}
                <Show when=move || mode.get() == Mode::Overview>
                    <Show when=move || snapshot.get().enabled fallback=move || view! {
                        <div class="dialog-actions">
                            <button type="button" on:click=move |_| mode.set(Mode::Setup)>"Set up sync"</button>
                            <button type="button" on:click=move |_| mode.set(Mode::Join)>"Use an invitation"</button>
                        </div>
                    }>
                        <p class="sync-state">{move || if snapshot.get().active {"Sync is available when your desktops can connect."} else {"Sync is preparing. Reading remains available."}}</p>
                        <ul class="device-list">
                            <For each=move || snapshot.get().devices key=|device| device.key.clone() let:device>
                                {
                                    let removable = !device.local && !device.revoked;
                                    let key = StoredValue::new(device.key.clone());
                                    view! { <li><span>{device.name}{if device.local {" · this desktop"} else {""}}{if device.revoked {" · removed"} else {""}}</span>
                                        <Show when=move || removable && snapshot.get().can_invite>
                                            <button type="button" on:click=move |_| { selected_device.set(key.get_value()); mode.set(Mode::Revoke); }>"Remove"</button>
                                        </Show>
                                    </li> }
                                }
                            </For>
                        </ul>
                        <Show when=move || snapshot.get().can_invite>
                            <button type="button" on:click=move |_| mode.set(Mode::Invite)>"Pair another desktop"</button>
                        </Show>
                        <Show when=move || !snapshot.get().can_invite><p>"Create invitations from the desktop holding your recovery key."</p></Show>
                    </Show>
                </Show>
                <Show when=move || mode.get() != Mode::Overview>
                    <form on:submit=move |event| {
                        event.prevent_default();
                        let current = mode.get_untracked();
                        if current == Mode::Setup && passphrase.get_untracked() != confirm.get_untracked() { error.set(Some("The passphrases do not match.".into())); return; }
                        match current {
                            Mode::Setup => submit("enable_sync", json!({"passphrase":passphrase.get_untracked(), "name":name.get_untracked()})),
                            Mode::Join => submit("join_sync_invitation", json!({"invitation":invitation.get_untracked(), "name":name.get_untracked()})),
                            Mode::Invite => submit("create_sync_invitation", json!({"passphrase":passphrase.get_untracked()})),
                            Mode::Revoke => submit("revoke_sync_device", json!({"passphrase":passphrase.get_untracked(), "key":selected_device.get_untracked()})),
                            Mode::Overview => {},
                        }
                    }>
                        <Show when=move || matches!(mode.get(), Mode::Setup | Mode::Join)>
                            <label>"Desktop name"<input type="text" required maxlength="256" autocomplete="off" prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() != Mode::Join && invitation.get().is_empty()>
                            <label>"Recovery passphrase"<input type="password" required minlength="8" autocomplete="off" prop:value=move || passphrase.get() on:input=move |event| passphrase.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Setup>
                            <p>"This passphrase protects the recovery key stored on this desktop. Ordinary reading and sync do not require it."</p>
                            <label>"Repeat passphrase"<input type="password" required minlength="8" autocomplete="off" prop:value=move || confirm.get() on:input=move |event| confirm.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Join || !invitation.get().is_empty()>
                            <label>{move || if mode.get() == Mode::Join {"Paste invitation"} else {"Copy this invitation to your other desktop"}}
                                <textarea required rows="5" readonly=move || mode.get() == Mode::Invite prop:value=move || invitation.get() on:input=move |event| invitation.set(event_target_value(&event))/>
                            </label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Invite && !invitation.get().is_empty()>
                            <p>"Valid for five minutes and one desktop. Keep Settings open while pairing."</p>
                        </Show>
                        <div class="dialog-actions">
                            <button type="button" disabled=move || busy.get() on:click=move |_| {
                                let text = invitation.get_untracked();
                                mode.set(Mode::Overview);
                                invitation.set(String::new());
                                passphrase.set(String::new());
                                error.set(None);
                                leptos::task::spawn_local(cancel_invitation(text));
                            }>"Back"</button>
                            <Show when=move || mode.get() != Mode::Invite || invitation.get().is_empty()>
                                <button type="submit" disabled=move || busy.get()>{move || if busy.get() {"Working…"} else {match mode.get() {Mode::Setup=>"Enable sync", Mode::Join=>"Pair desktop", Mode::Revoke=>"Remove desktop", _=>"Create invitation"}}}</button>
                            </Show>
                        </div>
                    </form>
                </Show>
        </section>
    }
}

async fn cancel_invitation(invitation: String) {
    // A pending create is cleaned up by its guarded response once its own
    // credential is known; an empty form cannot cancel another form's work.
    if !invitation.is_empty() {
        let _ = api::sync_action("cancel_sync_invitation", json!({"invitation": invitation})).await;
    }
}
