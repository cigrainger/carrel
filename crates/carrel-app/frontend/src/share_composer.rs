//! Audience presentation; native commands expand and validate reviewed selections.
use crate::api::SendMode;
use crate::{
    api,
    components::Modal,
    requests::Requests,
    sharing::{ShareTarget, request_token, use_sharing},
};
use leptos::{html, prelude::*};
use serde_json::json;

#[component]
pub(crate) fn ShareComposer(target: ShareTarget) -> impl IntoView {
    let sharing = use_sharing();
    let target = StoredValue::new(target);
    let people = RwSignal::new(Vec::<String>::new());
    let groups = RwSignal::new(Vec::<String>::new());
    let review = RwSignal::new(None::<api::AudienceReview>);
    let reviewing = RwSignal::new(false);
    let mode = RwSignal::new(None::<SendMode>);
    let note = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let saved = RwSignal::new(None::<String>);
    let submit = NodeRef::<html::Button>::new();
    let token = StoredValue::new(request_token());
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let previews = StoredValue::new(Requests::default());
    let cleanup = previews.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let preview = UnsyncCallback::new(move |()| {
        let selected_people = people.get_untracked();
        let selected_groups = groups.get_untracked();
        let guard = previews.get_value();
        let request = guard.begin();
        review.set(None);
        error.set(None);
        mode.set(None);
        if selected_people.is_empty() && selected_groups.is_empty() {
            reviewing.set(false);
            return;
        }
        reviewing.set(true);
        spawn(async move {
            let result = api::sharing_call::<api::AudienceReview>(
                "review_share_audience",
                json!({"people":selected_people,"groups":selected_groups}),
            )
            .await;
            if !guard.accepts(request) {
                return;
            }
            match result {
                Ok(value) => {
                    review.try_set(Some(value));
                }
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                }
            }
            reviewing.try_set(false);
        });
    });
    let change = UnsyncCallback::new(move |(is_group, key, checked): (bool, String, bool)| {
        let selection = if is_group { groups } else { people };
        selection.update(|choices| {
            choices.retain(|choice| choice != &key);
            if checked {
                choices.push(key);
            }
        });
        preview.run(());
    });
    let multiple = Memo::new(move |_| {
        review.with(|review| {
            review
                .as_ref()
                .is_some_and(|review| review.recipients.len() > 1)
        })
    });
    let ready = Memo::new(move |_| {
        review.with(|review| {
            review
                .as_ref()
                .is_some_and(|review| !review.recipients.is_empty())
        }) && (!multiple.get() || mode.get().is_some())
            && !reviewing.get()
            && !busy.get()
    });
    let cancel = UnsyncCallback::new(move |()| sharing.target.set(None));
    view! {
        <Modal label="Share article" class="feed-dialog share-dialog share-composer" on_cancel=cancel>
            <form on:keydown=move |event|{
                if (event.meta_key()||event.ctrl_key())&&event.key()=="Enter" {event.prevent_default();event.stop_propagation();if let Some(button)=submit.get_untracked(){button.click();}}
            } on:submit=move |event| {
                event.prevent_default();if !ready.get_untracked()||saved.get_untracked().is_some(){return;}
                let Some(review)=review.get_untracked() else{return;};
                let note=note.get_untracked();let mode=mode.get_untracked().unwrap_or(SendMode::Separately);
                let args=json!({"request":{"requestId":token.get_value(),"itemId":target.get_value().id,"selection":review.selection,"mode":mode,"note":if note.trim().is_empty(){None}else{Some(note.trim().to_owned())}}});
                let guard=requests.get_value();let request=guard.begin();let write=writes.get_value().begin();busy.set(true);error.set(None);
                spawn(async move {
                    let result=api::sharing_call::<api::SendReceipt>("send_article_batch",args).await;
                    drop(write);sharing.refresh();if result.is_ok(){sharing.history_changed();}
                    if !guard.accepts(request){return;}
                    match result {Ok(receipt)=>{saved.try_set(Some(receipt.message));},Err(problem)=>{error.try_set(Some(problem.message().into()));}}
                    busy.try_set(false);
                });
            }>
                <h3>"Share article"</h3><p class="share-title">{target.get_value().title}</p>
                <Show when=move ||saved.get().is_none() fallback=move ||view! {
                    <p role="status">{move ||saved.get().unwrap_or_default()}</p>
                    <p class="capture-notice">"Saved here; delivery waits while desktops are offline."</p>
                    <div class="feed-dialog-actions"><button type="button" on:click=move |_|cancel.run(())>"Done"</button></div>
                }>
                    <fieldset class="share-choices" disabled=move ||busy.get()>
                        <legend>"Share with"</legend>
                        <p class="capture-notice">"Choose people, groups, or both. Overlapping members receive one share."</p>
                        <div class="share-choice-list">
                            <For each={move ||sharing.people.get().people.into_iter().filter(|person|person.followed).collect::<Vec<_>>()} key=|person|person.key.clone() let:person>
                                {let key=StoredValue::new(person.key);view! {
                                    <label><input type="checkbox" prop:checked=move ||people.with(|choices|choices.contains(&key.get_value())) on:change=move |event|change.run((false,key.get_value(),event_target_checked(&event)))/>
                                        <span>{move ||sharing.people.with(|snapshot|snapshot.people.iter().find(|person|person.key==key.get_value()).map(|person|person.name.clone()).unwrap_or_else(||key.get_value()))}</span>
                                    </label>
                                }}
                            </For>
                            <For each=move ||sharing.people.get().groups key=|group|group.id.clone() let:group>
                                {let id=StoredValue::new(group.id);view! {
                                    <label><input type="checkbox" prop:checked=move ||groups.with(|choices|choices.contains(&id.get_value())) on:change=move |event|change.run((true,id.get_value(),event_target_checked(&event)))/>
                                        <span>{move ||sharing.people.with(|snapshot|snapshot.groups.iter().find(|group|group.id==id.get_value()).map(|group|format!("Group: {}",group.name)).unwrap_or_else(||"Group unavailable — choose again".into()))}</span>
                                    </label>
                                }}
                            </For>
                        </div>
                        <button type="button" on:click=move |_|{people.set(Vec::new());groups.set(Vec::new());preview.run(());}>"Clear recipients"</button>
                    </fieldset>
                    <Show when=move ||review.with(|review|review.is_some())>
                        <p class="share-audience"><strong>"Recipients: "</strong>{move ||review.with(|review|review.as_ref().map(|review|review.recipients.iter().map(|person|person.name.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default())}</p>
                    </Show>
                    <Show when=move ||multiple.get()>
                        <fieldset class="share-mode" disabled=move ||busy.get()><legend>"How would you like to share?"</legend>
                            <label><input type="radio" name="send-mode" value="conversation" prop:checked=move ||mode.get()==Some(SendMode::Conversation) on:change=move |_|mode.set(Some(SendMode::Conversation))/><span><strong>"Start a group conversation"</strong><small>"Everyone selected can see the participants and replies. You are introducing them through this article."</small></span></label>
                            <label><input type="radio" name="send-mode" value="separately" prop:checked=move ||mode.get()==Some(SendMode::Separately) on:change=move |_|mode.set(Some(SendMode::Separately))/><span><strong>"Send separately"</strong><small>"Each person receives a private share and conversation with you. Other recipients stay private."</small></span></label>
                        </fieldset>
                    </Show>
                    <label for="share-note">"A note (optional)"</label>
                    <textarea id="share-note" rows="4" maxlength="65536" readonly=move ||busy.get() prop:value=move ||note.get() on:input=move |event|note.set(event_target_value(&event))/>
                    <p class="capture-notice">"Shares this article and opening note. Your private highlights and notes stay private."</p>
                    {move ||error.get().map(|message|view!{<p class="feed-error" role="alert">{message}</p><button type="button" disabled=move ||busy.get() on:click=move |_|preview.run(())>"Review recipients again"</button>})}
                    <div class="feed-dialog-actions"><button type="button" on:click=move |_|cancel.run(())>"Cancel"</button><button node_ref=submit type="submit" disabled=move ||!ready.get()>"Share article"</button></div>
                </Show>
            </form>
        </Modal>
    }.into_any()
}

#[cfg(target_arch = "wasm32")]
fn spawn(task: impl std::future::Future<Output = ()> + 'static) {
    leptos::task::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(_task: impl std::future::Future<Output = ()> + 'static) {}
