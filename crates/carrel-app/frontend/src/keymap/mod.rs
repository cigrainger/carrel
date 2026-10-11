//! Stack-based keymap integration for the webview.

use leptos::prelude::Set;

mod event;
mod help;
mod palette;
mod provider;

pub use provider::{
    GlobalKeymapActions, KeymapProvider, use_action_handler, use_keymap_layer, use_keymap_warnings,
};

pub(crate) fn open_palette() {
    provider::use_keymap_context().palette_open.set(true);
}
pub(crate) fn open_help() {
    provider::use_keymap_context().help_open.set(true);
}
