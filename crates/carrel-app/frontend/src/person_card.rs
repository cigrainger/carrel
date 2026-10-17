//! A deliberate introduction preserves the thread, its draft and its trust boundary.
use crate::{
    api,
    components::{Modal, copy_text::CopyTextField},
    requests::Requests,
    sharing::use_sharing,
};
use leptos::{html, prelude::*};
use serde_json::json;

#[component]
pub(crate) fn ConversationPersonButton(
    share_id: String,
    #[prop(into)] person: Signal<api::Participant>,
) -> impl IntoView {
    let share = StoredValue::new(share_id);
    let key = StoredValue::new(person.get_untracked().key);
    let open = RwSignal::new(false);
    // Keep the nested dialog's view type out of every message renderer.
    view! {
        <button class="conversation-person-button" type="button" aria-label=move ||format!("View {}",person.with(|person|person.name.clone())) on:click=move |event|{
            crate::components::focus_dialog_trigger(&event);open.set(true);
        }>
            <crate::conversations::Avatar name=Signal::derive(move ||person.with(|person|person.name.clone())) image=Signal::derive(move ||person.with(|person|person.image.clone()))/>
            <strong>{move ||person.with(|person|person.name.clone())}</strong>
        </button>
        <Show when=move ||open.get()>
            <PersonCard share_id=share.get_value() person_key=key.get_value() on_close=UnsyncCallback::new(move |()|open.set(false))/>
        </Show>
    }.into_any()
}

#[component]
fn PersonCard(share_id: String, person_key: String, on_close: UnsyncCallback<()>) -> impl IntoView {
    let sharing = use_sharing();
    sharing.refresh();
    let share = StoredValue::new(share_id);
    let key = StoredValue::new(person_key);
    let data = RwSignal::new(None::<api::ConversationPerson>);
    let connecting = RwSignal::new(false);
    let contact = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let reviewed = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let contact_input = NodeRef::<html::Textarea>::new();
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        if connecting.get()
            && let Some(input) = contact_input.get()
        {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = input.focus_with_options(&options);
        }
    });
    let requests = StoredValue::new(Requests::default());
    let cleanup = requests.get_value();
    on_cleanup(move || cleanup.invalidate());
    let mutations = StoredValue::new(Requests::default());
    let cleanup = mutations.get_value();
    on_cleanup(move || cleanup.invalidate());
    let writes = StoredValue::new(crate::writes::use_pending_writes());
    let load = UnsyncCallback::new(move |()| {
        let guard = requests.get_value();
        let request = guard.begin();
        let args = json!({"shareId":share.get_value(),"key":key.get_value()});
        spawn(async move {
            let result =
                api::sharing_call::<api::ConversationPerson>("get_conversation_person", args).await;
            if !guard.accepts(request) {
                return;
            }
            match result {
                Ok(person) => {
                    data.try_set(Some(person));
                }
                Err(problem) => {
                    error.try_set(Some(problem.message().into()));
                }
            }
        });
    });
    load.run(());
    view! {
        <Modal label="Person" class="feed-dialog person-card" on_cancel=on_close>
            <header><h3>"Person"</h3><button type="button" on:click=move |_|on_close.run(())>"Done"</button></header>
            <Show when=move ||data.with(|data|data.is_some())>
                <div class="person-card-heading">
                    <crate::conversations::Avatar name=Signal::derive(move ||data.with(|data|data.as_ref().map(|data|data.person.name.clone()).unwrap_or_default())) image=Signal::derive(move ||data.with(|data|data.as_ref().and_then(|data|data.person.image.clone())))/>
                    <h4>{move ||data.with(|data|data.as_ref().map(|data|data.person.name.clone()).unwrap_or_default())}</h4>
                </div>
                <p>{move ||data.with(|data|data.as_ref().map(|data|if data.is_self{"This is you.".into()}else{format!("Introduced through {}'s share.",data.introduced_by)}).unwrap_or_default())}</p>
                <CopyTextField id="introduced-person-key" label="Verified public key" button="Copy this public key" text=Signal::derive(move ||key.get_value())/>
                <p class="capture-notice">"This key belongs to a participant in the original signed share. Recognizing the person is your choice."</p>
                <Show when=move ||data.with(|data|data.as_ref().is_some_and(|data|!data.is_self))>
                    <Show when=move ||!connecting.get() fallback=move ||view! {
                        <section class="person-card-connect">
                            <h4>"Exchange contact codes"</h4>
                            <p>"Exchange codes privately with this person, then each choose to follow the other. Ask how to reach them in this conversation."</p>
                            <Show when=move ||sharing.people.with(|people|people.contact_code.is_some()) fallback=move ||view!{<p>"Your contact code is preparing. You can return to the conversation and try again."</p><button type="button" on:click=move |_|sharing.refresh()>"Refresh your code"</button>}>
                                <CopyTextField id="introduction-own-code" label="Your contact code" button="Copy your contact code" text=Signal::derive(move ||sharing.people.with(|people|people.contact_code.clone().unwrap_or_default()))/>
                            </Show>
                            <p class="capture-notice">"Your contact code includes this desktop's connection address. Send it directly to this person; posting it in the conversation shares it with everyone there."</p>
                            <form on:submit=move |event|{
                                event.prevent_default();if busy.get_untracked()||saved.get_untracked(){return;}
                                let committing=reviewed.get_untracked();let command=if committing{"connect_conversation_person"}else{"inspect_conversation_person"};
                                let guard=mutations.get_value();let request=guard.begin();
                                let write=committing.then(||writes.get_value().begin());
                                let mut args=json!({"shareId":share.get_value(),"key":key.get_value(),"contact":contact.get_untracked()});
                                if committing {args["name"]=json!(name.get_untracked());}
                                busy.set(true);error.set(None);
                                spawn(async move {
                                    let result=api::sharing_call::<serde_json::Value>(command,args).await;drop(write);
                                    if committing {sharing.refresh();}
                                    if !guard.accepts(request){return;}
                                    match result {Ok(_)=>{if committing{saved.try_set(true);load.run(());}else{reviewed.try_set(true);}},Err(problem)=>{error.try_set(Some(problem.message().into()));}}
                                    busy.try_set(false);
                                });
                            }>
                                <label for="introduced-person-code">"Their contact code"</label>
                                <textarea node_ref=contact_input id="introduced-person-code" rows="3" maxlength="32768" readonly=move ||busy.get()||saved.get() prop:value=move ||contact.get() on:input=move |event|{contact.set(event_target_value(&event));reviewed.set(false);}/>
                                <label for="introduced-person-name">"Name for this person (optional)"</label>
                                <input id="introduced-person-name" maxlength="256" readonly=move ||busy.get()||saved.get() prop:value=move ||name.get() on:input=move |event|name.set(event_target_value(&event))/>
                                <Show when=move ||reviewed.get()&&!saved.get()><p>"The signed code matches this participant's key. Follow only if you recognize the person."</p></Show>
                                <Show when=move ||saved.get() fallback=move ||view! {
                                    <button type="submit" disabled=move ||busy.get()||contact.get().trim().is_empty()>{move ||if reviewed.get(){"Follow this person"}else{"Review contact code"}}</button>
                                }><p role="status">"Person followed on this desktop. They also need to follow you before direct exchange."</p></Show>
                            </form>
                        </section>
                    }>
                        <p>{move ||data.with(|data|data.as_ref().map(|data|if data.followed&&data.connected{"You follow this person and have their contact code."}else if data.followed{"You follow this person. Exchange contact codes to connect directly."}else{"This conversation is your shared context. Following is a separate choice."}).unwrap_or_default())}</p>
                        <button type="button" on:click=move |_|{connecting.set(true);}>{move ||data.with(|data|if data.as_ref().is_some_and(|data|data.followed){"Exchange contact codes"}else{"Connect"})}</button>
                    </Show>
                </Show>
            </Show>
            {move ||error.get().map(|message|view!{<p class="feed-error" role="alert">{message}</p><Show when=move ||data.get().is_none()><button type="button" on:click=move |_|{error.set(None);load.run(());}>"Try again"</button></Show>})}
        </Modal>
    }.into_any()
}
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl std::future::Future<Output = ()> + 'static) {
    leptos::task::spawn_local(task);
}
#[cfg(not(target_arch = "wasm32"))]
fn spawn(_task: impl std::future::Future<Output = ()> + 'static) {}
