//! Keyboard shortcut parsing and dispatch.

mod binding;
pub mod default;
mod error;
mod stack;

pub use binding::{Action, Binding, Key, KeyCode, Modifiers, Sequence};
pub use error::KeymapError;
pub use stack::{DispatchResult, KeymapStack, Layer, LayerId};
