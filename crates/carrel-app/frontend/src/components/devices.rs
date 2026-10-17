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
    let success = RwSignal::new(None::<&'static str>);
    let heading = NodeRef::<leptos::html::H3>::new();
    let name_input = NodeRef::<leptos::html::Input>::new();
    let passphrase_input = NodeRef::<leptos::html::Input>::new();
    let preferences = crate::preferences::use_preferences();
    let sharing = crate::sharing::use_sharing();
    let feeds = feeds::use_feed_controller();
    let requests = StoredValue::new(crate::requests::Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    status.refresh();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        let current = mode.get();
        if current == Mode::Overview {
            if let Some(heading) = heading.get() {
                set_timeout(
                    move || {
                        if heading.is_connected() {
                            let _ = heading.focus();
                        }
                    },
                    std::time::Duration::ZERO,
                );
            }
        } else if let Some(input) = if matches!(current, Mode::Setup | Mode::Join) {
            name_input.get()
        } else {
            passphrase_input.get()
        } {
            set_timeout(
                move || {
                    if input.is_connected() {
                        let _ = input.focus();
                    }
                },
                std::time::Duration::ZERO,
            );
        }
    });
    #[cfg(target_arch = "wasm32")]
    on_cleanup(move || {
        let text = invitation.try_get_untracked().unwrap_or_default();
        leptos::task::spawn_local(cancel_invitation(text));
    });
    let submit = move |command: &'static str, args: serde_json::Value| {
        busy.set(true);
        error.set(None);
        success.set(None);
        passphrase.set(String::new());
        confirm.set(String::new());
        let guard = requests.get_value();
        let request = guard.begin();
        leptos::task::spawn_local(async move {
            let result = api::sync_action(command, args).await;
            status.refresh();
            if result.is_ok() && matches!(command, "enable_sync" | "join_sync_invitation") {
                sharing.refresh();
            }
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
                        success.set(Some(match command {
                            "enable_sync" => "Your identity is ready. You can connect with a person or pair another desktop.",
                            "join_sync_invitation" => "This desktop is paired. Keep both apps open while your library copies; you can keep reading.",
                            "revoke_sync_device" => "Desktop removed. It can no longer sync; data already copied to it cannot be erased remotely.",
                            _ => "Done.",
                        }));
                    }
                    feeds::refresh_from_sync(feeds).await;
                }
                Err(problem) => error.set(Some(if problem.is_pairing_invitation_error() {
                    "That invitation is invalid, expired or already used. Create a fresh invitation on your other desktop, keep its Settings open, and paste the complete text here.".into()
                } else if problem.is_recovery_passphrase_error() {
                    "That passphrase did not unlock your recovery key. Try the passphrase you chose when creating this identity; your library is unchanged.".into()
                } else { problem.message().into() })),
            }
            busy.try_set(false);
        });
    };

    view! {
        <section class="devices-panel">
                <h3 node_ref=heading tabindex="-1">"Your desktops"</h3>
                <p>"Pair only desktops you own. A paired desktop receives your private library, highlights and notes."</p>
                <button type="button" class="pairing-people-link" on:click=move |_| preferences.open_settings(crate::preferences::SettingsSection::People)>"Connecting with a friend? Open People"</button>
                {move || error.get().or_else(|| snapshot.get().error).map(|text| view! {<p class="feed-error" role="alert">{text}</p>})}
                <Show when=move || mode.get() == Mode::Overview>
                    {move || success.get().map(|text| view! { <p class="pairing-result" role="status">{text}</p> })}
                    <Show when=move || snapshot.get().enabled fallback=move || view! {
                        <p>"First desktop? Create an identity to connect with people or sync your own desktops. Local reading needs no setup."</p>
                        <div class="dialog-actions">
                            <button type="button" on:click=move |_| { error.set(None); mode.set(Mode::Setup); }>"Create identity"</button>
                            <button type="button" on:click=move |_| { error.set(None); mode.set(Mode::Join); }>"Pair this desktop"</button>
                        </div>
                        <p>"Already use Carrel on another desktop? Use its invitation to keep the same identity."</p>
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
                        <Show when=move || !snapshot.get().can_invite><p>"To pair or remove another desktop, use the desktop where you created this identity. Its recovery key stays there."</p></Show>
                    </Show>
                </Show>
                <Show when=move || mode.get() != Mode::Overview>
                    <h4>{move || match mode.get() { Mode::Setup => "Create your identity", Mode::Join => "Pair this desktop", Mode::Invite => "Pair another desktop", Mode::Revoke => "Remove desktop", Mode::Overview => "" }}</h4>
                    <Show when=move || mode.get() == Mode::Join><ol class="pairing-steps"><li>"On your other desktop, open Settings → Devices → Pair another desktop."</li><li>"Create an invitation there and keep Settings open."</li><li>"Name this desktop and paste the complete invitation below."</li></ol></Show>
                    <Show when=move || mode.get() == Mode::Invite && invitation.get().is_empty()><p>"Unlock your recovery key to create a one-use invitation for another desktop you own."</p></Show>
                    <Show when=move || mode.get() == Mode::Revoke><p>"Remove "<strong>{move || snapshot.get().devices.into_iter().find(|device| device.key == selected_device.get()).map(|device| device.name).unwrap_or_else(|| "this desktop".into())}</strong>" from private sync? This stops future sync. It cannot erase the library already copied to that desktop."</p></Show>
                    <form on:submit=move |event| {
                        event.prevent_default();
                        if busy.get_untracked() { return; }
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
                            <label>"Desktop name"<input node_ref=name_input type="text" required maxlength="256" autocomplete="off" placeholder="For example, My MacBook" readonly=move || busy.get() prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() != Mode::Join && invitation.get().is_empty()>
                            <label>"Recovery passphrase"<input node_ref=passphrase_input type="password" required minlength="8" autocomplete="off" readonly=move || busy.get() prop:value=move || passphrase.get() on:input=move |event| passphrase.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Setup>
                            <p>"Use at least eight characters and keep this passphrase somewhere safe. It protects the recovery key on this desktop; Carrel cannot reset it. Ordinary reading and sync do not require it."</p>
                            <label>"Repeat passphrase"<input type="password" required minlength="8" autocomplete="off" readonly=move || busy.get() prop:value=move || confirm.get() on:input=move |event| confirm.set(event_target_value(&event))/></label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Join>
                            <label>"Paste invitation"
                                <textarea required rows="4" spellcheck="false" autocapitalize="off" autocomplete="off" readonly=move || busy.get() prop:value=move || invitation.get() on:input=move |event| invitation.set(event_target_value(&event))/>
                            </label>
                        </Show>
                        <Show when=move || mode.get() == Mode::Invite && !invitation.get().is_empty()>
                            <div class="pairing-invitation"><p>"On your other desktop, open Settings → Devices → Pair this desktop and paste this invitation."</p>
                            <super::copy_text::CopyTextField id="pairing-invitation" label="Invitation for your other desktop" button="Copy invitation" text=Signal::derive(move || invitation.get()) focus_on_mount=true/>
                            <p>"Valid for five minutes and one desktop. Keep these Settings open until the other desktop confirms it is paired. If the invitation expires, cancel it and create a fresh one."</p></div>
                        </Show>
                        <div class="dialog-actions">
                            <button type="button" disabled=move || busy.get() on:click=move |_| {
                                let text = invitation.get_untracked();
                                mode.set(Mode::Overview);
                                invitation.set(String::new());
                                passphrase.set(String::new());
                                confirm.set(String::new());
                                error.set(None);
                                leptos::task::spawn_local(cancel_invitation(text));
                            }>{move || if mode.get() == Mode::Invite && !invitation.get().is_empty() { "Cancel invitation" } else { "Back" }}</button>
                            <Show when=move || mode.get() != Mode::Invite || invitation.get().is_empty()>
                                <button type="submit" disabled=move || busy.get()>{move || if busy.get() {"Working…"} else {match mode.get() {Mode::Setup=>"Create identity", Mode::Join=>"Pair desktop", Mode::Revoke=>"Remove desktop", _=>"Create invitation"}}}</button>
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
