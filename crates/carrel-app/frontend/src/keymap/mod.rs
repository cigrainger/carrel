//! Stack-based keymap integration for the webview.

use leptos::prelude::*;

mod event;
mod help;
mod palette;
mod provider;
pub(crate) use help::KeyboardReference;

pub use provider::{
    GlobalKeymapActions, KeymapProvider, use_action_handler, use_keymap_layer, use_keymap_warnings,
};

pub(crate) fn open_palette() {
    provider::use_keymap_context().palette_open.set(true);
}
pub(crate) fn open_help() {
    provider::use_keymap_context().help_open.set(true);
}

/// Menus use the same route-owned action as shortcuts and the command palette.
pub(crate) fn action(name: &'static str) -> (Signal<bool, LocalStorage>, UnsyncCallback<()>) {
    let context = provider::use_keymap_context();
    (
        Signal::derive_local(move || context.has_action(name)),
        UnsyncCallback::new(move |()| {
            #[cfg(target_arch = "wasm32")]
            provider::dispatch_action(context, carrel_core::keymap::Action::named(name));
        }),
    )
}
