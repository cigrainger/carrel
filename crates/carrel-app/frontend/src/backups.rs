//! Backup forms own only drafts and native review tokens, never filesystem paths.
use crate::{api, requests::Requests};
use leptos::{html, prelude::*};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupStatus {
    can_restore: bool,
    pending: bool,
    recovery_pending: bool,
    has_master_key: bool,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupPreview {
    app_version: String,
    date: String,
    master: Option<String>,
    has_master_key: bool,
    cache_incomplete: bool,
}

#[component]
pub(crate) fn BackupPanel() -> impl IntoView {
    let status = RwSignal::new(None::<BackupStatus>);
    let preview = RwSignal::new(None::<BackupPreview>);
    let password = RwSignal::new(String::new());
    let repeat = RwSignal::new(String::new());
    let restore_password = RwSignal::new(String::new());
    let token = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);
    let requests = StoredValue::new(Requests::default());
    let loads = StoredValue::new(Requests::default());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let progress = StoredValue::new(crate::progress::use_progress_writer());
    let preferences = crate::preferences::use_preferences();
    let preview_heading = NodeRef::<html::H5>::new();
    let restore_input = NodeRef::<html::Input>::new();
    let restore_focus = RwSignal::new(false);
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if preferences.settings_section.get() != crate::preferences::SettingsSection::Backups {
            return;
        }
        if preview.get().is_some() {
            if let Some(heading) = preview_heading.get() {
                set_timeout(
                    move || {
                        if heading.is_connected() {
                            let _ = heading.focus();
                        }
                    },
                    std::time::Duration::ZERO,
                );
            }
        } else if restore_focus.get()
            && let Some(input) = restore_input.get()
        {
            restore_focus.set(false);
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
    let cleanup = requests.get_value();
    let load_cleanup = loads.get_value();
    on_cleanup(move || {
        cleanup.invalidate();
        load_cleanup.invalidate();
        password.try_set(String::new());
        repeat.try_set(String::new());
        restore_password.try_set(String::new());
        if let Some(Some(id)) = token.try_get_untracked() {
            spawn(async move {
                let _ =
                    api::local_call::<()>("cancel_backup_review", json!({"requestId":id})).await;
            });
        }
    });
    let load = UnsyncCallback::new(move |()| {
        let guard = loads.get_value();
        let request = guard.begin();
        spawn(async move {
            match api::local_call::<BackupStatus>("backup_status", json!({})).await {
                Ok(fresh) if guard.accepts(request) => {
                    status.try_set(Some(fresh));
                }
                Err(problem) if guard.accepts(request) => {
                    error.try_set(Some(problem.message().into()));
                }
                _ => {}
            }
        });
    });
    load.run(());
    let create = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let secret = password.get_untracked();
        if secret.chars().count() < 12 || secret.len() > 4096 {
            error.set(Some(
                "Use a backup passphrase of at least twelve characters.".into(),
            ));
            return;
        }
        if secret != repeat.get_untracked() {
            error.set(Some("The backup passphrases do not match.".into()));
            return;
        }
        busy.set(true);
        error.set(None);
        notice.set(Some("Preparing your encrypted archive…".into()));
        let guard = requests.get_value();
        let request = guard.begin();
        let tracker = writes.get_value();
        let writer = progress.get_value();
        spawn(async move {
            writer.flush();
            tracker.wait_until_idle().await;
            let result = if writer.has_unsaved_positions() {
                Err("Save your reading position before creating a backup. Close Settings and try again.".to_string())
            } else {
                api::local_call::<bool>("create_backup", json!({"passphrase":secret}))
                    .await
                    .map_err(|e| e.message().to_string())
            };
            if !guard.accepts(request) {
                return;
            }
            busy.try_set(false);
            notice.try_set(None);
            match result {
                Ok(true) => {
                    password.try_set(String::new());
                    repeat.try_set(String::new());
                    notice.try_set(Some("Encrypted backup created. Keep the file and its passphrase somewhere safe.".into()));
                }
                Ok(false) => {}
                Err(message) => {
                    error.try_set(Some(message));
                }
            }
        });
    };
    let choose = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        notice.set(Some("Validating the encrypted backup…".into()));
        preview.set(None);
        let id = crate::sharing::request_token();
        token.set(Some(id.clone()));
        let secret = restore_password.get_untracked();
        let guard = requests.get_value();
        let request = guard.begin();
        spawn(async move {
            let result = api::local_call::<Option<BackupPreview>>(
                "preview_backup",
                json!({"passphrase":secret,"requestId":id.clone()}),
            )
            .await;
            if !guard.accepts(request) {
                // A close can race the native command's initial registration.
                // Dispose its late result as well as sending cleanup on close.
                let _ =
                    api::local_call::<()>("cancel_backup_review", json!({"requestId":id})).await;
                return;
            }
            busy.try_set(false);
            notice.try_set(None);
            match result {
                Ok(Some(fresh)) => {
                    restore_password.try_set(String::new());
                    preview.try_set(Some(fresh));
                }
                Ok(None) => {}
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                    load.run(());
                }
            }
        });
    };
    let cancel = UnsyncCallback::new(move |()| {
        if let Some(id) = token.get_untracked() {
            spawn(async move {
                let _ =
                    api::local_call::<()>("cancel_backup_review", json!({"requestId":id})).await;
            });
        }
        token.set(None);
        preview.set(None);
        restore_focus.set(true);
        notice.set(None);
        error.set(None);
    });
    let restore = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Some(id) = token.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(None);
        notice.set(Some("Preparing to restart and restore…".into()));
        let guard = requests.get_value();
        let request = guard.begin();
        let tracker = writes.get_value();
        let writer = progress.get_value();
        spawn(async move {
            writer.flush();
            tracker.wait_until_idle().await;
            if writer.has_unsaved_positions() {
                if guard.accepts(request) {
                    busy.try_set(false);
                    notice.try_set(None);
                    error.try_set(Some("Save your reading position before restoring. Close Settings and try again.".into()));
                }
                return;
            }
            let result = api::local_call::<()>("restore_backup", json!({"requestId":id})).await;
            if !guard.accepts(request) {
                return;
            }
            busy.try_set(false);
            preview.try_set(None);
            token.try_set(None);
            load.run(());
            match result {
                Ok(()) => {
                    notice.try_set(Some(
                        "Restore prepared. Carrel will restart after local saves finish.".into(),
                    ));
                }
                Err(problem) => {
                    notice.try_set(None);
                    error.try_set(Some(format!(
                        "{} Choose the backup again to retry.",
                        problem.message()
                    )));
                }
            }
        });
    };
    let pending_action = UnsyncCallback::new(move |restart: bool| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        spawn(async move {
            let command = if restart {
                "restart_prepared_restore"
            } else {
                "cancel_pending_restore"
            };
            let result = api::local_call::<()>(command, json!({})).await;
            if !guard.accepts(request) {
                return;
            }
            busy.try_set(false);
            load.run(());
            if let Err(problem) = result {
                error.try_set(Some(problem.message().into()));
            } else if !restart {
                notice.try_set(Some(
                    "Prepared restore cancelled. This installation is unchanged.".into(),
                ));
            }
        });
    });
    view! {
        <div class="backup-panel">
            <h3>"Backups"</h3>
            <p>"Keep a private copy of your archive, cached articles, notes, subscriptions and conversation history."</p>
            <p>"An encrypted master recovery key is included only when this desktop holds it. Appearance and keyboard preferences stay on this installation."</p>
            <Show when=move||status.get().is_some_and(|s|s.recovery_pending)>
                <div class="backup-recovery">
                    <p>"Your restored archive is available locally."{move||if status.get().is_some_and(|s|s.has_master_key){" Unlock the original recovery key in Devices to resume your identity, or pair from another desktop belonging to it."}else{" Use Devices to authorize this fresh desktop before syncing or sharing."}}</p>
                    <button type="button" on:click=move |_|preferences.open_settings(crate::preferences::SettingsSection::Devices)>"Open Devices"</button>
                </div>
            </Show>
            <form class="backup-form" on:submit=create>
                <h4>"Create encrypted backup"</h4>
                <label for="backup-passphrase">"New backup passphrase"</label>
                <input id="backup-passphrase" type="password" required minlength="12" maxlength="4096" autocomplete="new-password" readonly=move||busy.get() prop:value=move||password.get() on:input=move|e|password.set(event_target_value(&e))/>
                <label for="backup-repeat">"Repeat backup passphrase"</label>
                <input id="backup-repeat" type="password" required minlength="12" maxlength="4096" autocomplete="new-password" readonly=move||busy.get() prop:value=move||repeat.get() on:input=move|e|repeat.set(event_target_value(&e))/>
                <p>"Use at least twelve characters. Carrel cannot reset this passphrase. Your identity keeps its separate recovery passphrase."</p>
                <button type="submit" disabled=move||busy.get()>"Create encrypted backup"</button>
            </form>
            <section class="backup-restore">
                <h4>"Restore backup"</h4>
                <p>"Restore into an empty installation. The file is fully checked before you choose to restart and import it."</p>
                <Show when=move||status.get().is_some_and(|s|s.pending) fallback=move||view!{
                    <Show when=move||status.get().is_some_and(|s|s.can_restore) fallback=||view!{<p>"Restore is available on an empty installation. Your current archive will be kept."</p>}>
                        <Show when=move||preview.get().is_none() fallback=move||view!{
                            <div class="backup-preview" role="region" aria-label="Backup preview">
                                <h5 node_ref=preview_heading tabindex="-1">"Ready to restore"</h5>
                                <dl>
                                    <dt>"Created"</dt><dd>{move||preview.get().map(|p|p.date).unwrap_or_default()}</dd>
                                    <dt>"Carrel version"</dt><dd>{move||preview.get().map(|p|p.app_version).unwrap_or_default()}</dd>
                                    <dt>"Identity"</dt><dd><code>{move||preview.get().and_then(|p|p.master).unwrap_or_else(||"Local archive; no identity configured".into())}</code></dd>
                                </dl>
                                <p>{move||if preview.get().is_some_and(|p|p.has_master_key){"Includes your encrypted master recovery key. Its original passphrase is still required to unlock it."}else{"This backup contains no master recovery key. A paired identity needs authorization from another own desktop."}}</p>
                                <Show when=move||preview.get().is_some_and(|p|p.cache_incomplete)><p>"Some content was already unavailable on the source desktop. Available cached content is included; missing content cannot be recreated by this backup."</p></Show>
                                <p>"Carrel will use a fresh device key. Nothing will be sent to another desktop or person until you resume your identity."</p>
                                <div class="dialog-actions"><button type="button" disabled=move||busy.get() on:click=move |_|cancel.run(())>"Cancel review"</button><button type="button" disabled=move||busy.get() on:click=restore>"Restart and restore"</button></div>
                            </div>
                        }>
                            <form class="backup-form" on:submit=choose>
                                <label for="restore-passphrase">"Backup passphrase"</label>
                                <input node_ref=restore_input id="restore-passphrase" type="password" required maxlength="4096" autocomplete="off" readonly=move||busy.get() prop:value=move||restore_password.get() on:input=move|e|restore_password.set(event_target_value(&e))/>
                                <button type="submit" disabled=move||busy.get()>"Choose backup and preview"</button>
                            </form>
                        </Show>
                    </Show>
                }>
                    <p>"A restore is prepared for the next launch. Your current installation has not been replaced."</p>
                    <div class="dialog-actions"><button type="button" disabled=move||busy.get() on:click=move |_|pending_action.run(false)>"Cancel prepared restore"</button><button type="button" disabled=move||busy.get() on:click=move |_|pending_action.run(true)>"Restart Carrel"</button></div>
                </Show>
            </section>
            {move||notice.get().map(|text|view!{<p role="status">{text}</p>})}
            {move||error.get().map(|text|view!{<p class="feed-error" role="alert">{text}</p><button type="button" disabled=move||busy.get() on:click=move |_|{error.set(None);load.run(());}>"Refresh backup status"</button>})}
        </div>
    }
}

fn spawn(future: impl std::future::Future<Output = ()> + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(future);
    #[cfg(not(target_arch = "wasm32"))]
    drop(future);
}

#[derive(Clone, Deserialize)]
struct RecoveredPreview {
    master: String,
    key: String,
    name: String,
}

#[component]
pub(crate) fn RecoveredDesktop() -> impl IntoView {
    let code = RwSignal::new(String::new());
    let preview = RwSignal::new(None::<RecoveredPreview>);
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if busy.get_untracked() {
            return;
        }
        let reviewed = preview.get_untracked().is_some();
        let text = code.get_untracked();
        busy.set(true);
        error.set(None);
        notice.set(None);
        let guard = requests.get_value();
        let request = guard.begin();
        spawn(async move {
            let result = if reviewed {
                api::local_call::<()>("connect_recovered_desktop", json!({"code":text}))
                    .await
                    .map(|()| None)
            } else {
                api::local_call::<RecoveredPreview>(
                    "inspect_recovered_desktop",
                    json!({"code":text}),
                )
                .await
                .map(Some)
            };
            if !guard.accepts(request) {
                return;
            }
            busy.try_set(false);
            match result {
                Ok(Some(person)) => {
                    preview.try_set(Some(person));
                }
                Ok(None) => {
                    code.try_set(String::new());
                    preview.try_set(None);
                    notice.try_set(Some(
                        "Desktop authorization saved. Keep both apps open while they reconnect."
                            .into(),
                    ));
                }
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                }
            }
        });
    };
    view! {
        <details class="backup-recovery">
            <summary>"Reconnect a recovered desktop"</summary>
            <p>"On the recovered desktop, resume its original identity, then copy its contact code from Settings → People. Review that code here to authorize its fresh device key under your existing identity."</p>
            <form class="backup-form" on:submit=submit>
                <label for="recovered-desktop-code">"Recovered desktop contact code"</label>
                <textarea id="recovered-desktop-code" required rows="3" maxlength="16384" spellcheck="false" autocomplete="off" readonly=move||busy.get() prop:value=move||code.get() on:input=move|event|{code.set(event_target_value(&event));preview.set(None);}/>
                <Show when=move||preview.get().is_some()>
                    <p>"Authorize "<strong>{move||preview.get().map(|p|p.name).unwrap_or_default()}</strong>"?"</p>
                    <p>"Reading identity: "<code>{move||preview.get().map(|p|p.master).unwrap_or_default()}</code></p>
                    <p>"Fresh device key: "<code>{move||preview.get().map(|p|p.key).unwrap_or_default()}</code></p>
                    <p>"This authorizes your own desktop to sync your private archive, highlights and notes under the identity shown above."</p>
                </Show>
                <button type="submit" disabled=move||busy.get()>{move||if preview.get().is_some(){"Authorize recovered desktop"}else{"Review desktop code"}}</button>
            </form>
            {move||notice.get().map(|message|view!{<p role="status">{message}</p>})}
            {move||error.get().map(|message|view!{<p class="feed-error" role="alert">{message}</p>})}
        </details>
    }
}
