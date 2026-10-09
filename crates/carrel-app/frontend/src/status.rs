//! Quiet application status messages.

use leptos::prelude::*;

/// Application-level status surface shared with the bottom strip.
#[derive(Clone, Copy)]
pub(crate) struct StatusController {
    message: RwSignal<Option<StatusMessage>>,
}

impl StatusController {
    /// Current app-level status message.
    pub(crate) fn message(&self) -> RwSignal<Option<StatusMessage>> {
        self.message
    }

    /// Replace the current message with a warning.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn warn(&self, text: impl Into<String>) {
        self.message.try_set(Some(StatusMessage {
            text: text.into(),
            warning: true,
        }));
    }

    /// Replace the current message with neutral status text.
    pub(crate) fn info(&self, text: impl Into<String>) {
        self.message.try_set(Some(StatusMessage {
            text: text.into(),
            warning: false,
        }));
    }
}

/// One message for the status strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusMessage {
    /// Message text.
    pub(crate) text: String,
    /// Whether this should render as a warning.
    pub(crate) warning: bool,
}

/// Provide app status state for app children.
pub(crate) fn provide_status_controller() -> StatusController {
    let controller = StatusController {
        message: RwSignal::new(None),
    };
    provide_context(controller);
    controller
}

/// Read the app status controller.
pub(crate) fn use_status_controller() -> StatusController {
    use_context::<StatusController>().expect("StatusController should be mounted")
}
