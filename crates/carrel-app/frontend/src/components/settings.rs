//! Installation settings stay above the mounted reading context.

use crate::components::{
    Icon, IconName, Modal, SourceActivityContent, SubscriptionFiles, ThemeControls,
};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Reading,
    Sources,
    People,
    Devices,
    Keyboard,
}

#[component]
pub(crate) fn Settings() -> impl IntoView {
    let preferences = crate::preferences::use_preferences();
    view! { <Show when=move || preferences.settings_open.get()><SettingsDialog/></Show> }
}

#[component]
fn SettingsDialog() -> impl IntoView {
    let preferences = crate::preferences::use_preferences();
    let feeds = crate::feeds::use_feed_controller();
    let selected = RwSignal::new(Section::Reading);
    let add_button = NodeRef::<leptos::html::Button>::new();
    let close = UnsyncCallback::new(move |()| preferences.settings_open.set(false));
    view! {
        <Modal label="Settings" class="settings-dialog" on_cancel=close>
            <header class="settings-heading"><h2>"Settings"</h2><button type="button" aria-label="Close settings" on:click=move |_| close.run(())>"Done"</button></header>
            <div class="settings-layout">
                <nav class="settings-sections" aria-label="Settings sections">
                    {[(Section::Reading,"Reading",IconName::Reading),(Section::Sources,"Sources",IconName::Library),(Section::People,"People",IconName::People),(Section::Devices,"Devices",IconName::Devices),(Section::Keyboard,"Keyboard",IconName::Keyboard)].into_iter().map(|(section,label,icon)| view! {
                        <button type="button" class:active=move || selected.get() == section aria-pressed=move || (selected.get() == section).to_string() on:click=move |_| selected.set(section)><Icon name=icon/>{label}</button>
                    }).collect_view()}
                </nav>
                <div class="settings-content">
                    <section hidden=move || selected.get() != Section::Reading aria-label="Reading settings">
                        <h3>"Reading"</h3><ThemeControls/><crate::preferences::ReadingControls/>
                    </section>
                    <section hidden=move || selected.get() != Section::Sources aria-label="Source settings">
                        <h3>"Sources"</h3>
                        <div class="settings-source-tools"><button node_ref=add_button type="button" on:click=move |_| {
                            if let Some(button) = add_button.get_untracked() { let _ = button.focus(); }
                            feeds.add_open.set(true);
                        }>"Add source"</button><SubscriptionFiles/></div>
                        <SourceActivityContent manage=true on_open=close/>
                    </section>
                    <section hidden=move || selected.get() != Section::Devices aria-label="Device settings"><super::devices::DevicesPanel/></section>
                    <section hidden=move || selected.get() != Section::People aria-label="People settings"><crate::sharing::PeoplePanel/></section>
                    <section hidden=move || selected.get() != Section::Keyboard aria-label="Keyboard settings">
                        <h3>"Keyboard"</h3><p>"Commands (⌘K) finds actions, sources and saved articles. These bindings reflect your current view and keymap."</p>
                        <crate::keymap::KeyboardReference/>
                    </section>
                </div>
            </div>
        </Modal>
    }
}
