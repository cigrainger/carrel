//! Stack-based keymap integration for the webview.

mod event;
mod help;
mod provider;

pub use provider::{
    GlobalKeymapActions, KeymapProvider, use_action_handler, use_keymap_layer, use_keymap_warnings,
};
