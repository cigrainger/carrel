//! Default keymap layers from the design document.

use crate::keymap::{Action, Binding, Layer, Sequence};

/// Stable key for the global layer.
pub const GLOBAL_LAYER: &str = "global";
/// Stable key for list routes.
pub const LIST_LAYER: &str = "list";
/// Stable key for reading routes.
pub const READING_LAYER: &str = "reading";
/// Stable key for item actions.
pub const ITEM_ACTION_LAYER: &str = "item-actions";
/// Stable key for reading-view-only extras.
pub const READING_EXTRA_LAYER: &str = "reading-extra";

/// User keymap override parsed from configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingOverride {
    /// Layer key the override applies to.
    pub layer: String,
    /// Replacement sequence.
    pub sequence: Sequence,
    /// Action to bind.
    pub action: Action,
}

impl BindingOverride {
    /// Create a keymap override.
    pub fn new(layer: impl Into<String>, sequence: Sequence, action: Action) -> Self {
        Self {
            layer: layer.into(),
            sequence,
            action,
        }
    }
}

/// Default global bindings.
pub fn global_layer() -> Layer {
    Layer::new(
        GLOBAL_LAYER,
        "Global",
        vec![
            bind("1", "go-today", "Today"),
            bind("2", "go-friends", "Friends"),
            bind("3", "go-library", "Library"),
            bind("4", "go-highlights", "Highlights"),
            bind("5", "go-lists", "Lists"),
            bind("?", "show-keymap-help", "Keymap reference"),
            bind("cmd-k", "open-command-palette", "Command palette").allow_in_input(),
            bind("cmd-,", "open-settings", "Settings").allow_in_input(),
        ],
    )
}

/// Default list-route bindings.
pub fn list_layer() -> Layer {
    Layer::new(
        LIST_LAYER,
        "Lists",
        vec![
            bind("j", "next-item", "Next item"),
            bind("k", "prev-item", "Previous item"),
            bind("J", "next-unread-item", "Next unread item"),
            bind("K", "prev-unread-item", "Previous unread item"),
            bind("o", "open-item", "Open item"),
            bind("Enter", "open-item", "Open item"),
            bind("Esc", "go-back", "Back").allow_in_input(),
            bind("q", "go-back", "Back"),
            bind("g g", "top-of-list", "Top of list"),
            bind("g e", "end-of-list", "End of list"),
            bind("/", "search-list", "Search list"),
            bind("\\", "clear-search", "Clear search"),
        ],
    )
}

/// Default reading-view bindings.
pub fn reading_layer() -> Layer {
    Layer::new(
        READING_LAYER,
        "Reading",
        vec![
            bind("Space", "page-down", "Page down"),
            bind("S-Space", "page-up", "Page up"),
            bind("n", "next-item-mark-read", "Next item and mark read"),
            bind("p", "prev-item-mark-read", "Previous item and mark read"),
            bind(",", "article-top", "Top of article"),
            bind(".", "article-bottom", "Bottom of article"),
            bind("Esc", "go-back", "Back").allow_in_input(),
            bind("q", "go-back", "Back"),
        ],
    )
}

/// Default item-action bindings.
pub fn item_action_layer() -> Layer {
    Layer::new(
        ITEM_ACTION_LAYER,
        "Item Actions",
        vec![
            bind("s", "toggle-star", "Star or unstar"),
            bind("m", "mark-read", "Mark as read"),
            bind("M", "mark-unread", "Mark as unread"),
            bind("t", "tag-item", "Tag"),
            bind("h", "highlight-selection", "Highlight selection"),
            bind("H", "highlight-with-note", "Highlight and add note"),
            bind("e", "send-to-default-ereader", "Send to default ereader"),
            bind("E", "send-to-ereader-picker", "Send to ereader"),
            bind("c", "share-last-audience", "Share with last-used audience"),
            bind("C", "share-audience-picker", "Share with audience"),
            bind("a", "archive-item", "Archive"),
        ],
    )
}

/// Extra bindings used only inside the reading view.
pub fn reading_extra_layer() -> Layer {
    Layer::new(
        READING_EXTRA_LAYER,
        "Reading Extras",
        vec![
            bind("←", "previous-unread-with-prompt", "Previous unread"),
            bind("→", "next-unread-with-prompt", "Next unread"),
            bind("i", "show-item-info", "Show item info"),
        ],
    )
}

/// Return true when an action name is known to the default keymap.
pub fn is_known_action(action: &str) -> bool {
    known_action_names().contains(&action)
}

/// Stable action names in the default keymap.
pub fn known_action_names() -> &'static [&'static str] {
    &[
        "go-today",
        "go-friends",
        "go-library",
        "go-highlights",
        "go-lists",
        "show-keymap-help",
        "open-command-palette",
        "open-settings",
        "next-item",
        "prev-item",
        "next-unread-item",
        "prev-unread-item",
        "open-item",
        "go-back",
        "top-of-list",
        "end-of-list",
        "search-list",
        "clear-search",
        "page-down",
        "page-up",
        "next-item-mark-read",
        "prev-item-mark-read",
        "article-top",
        "article-bottom",
        "toggle-star",
        "mark-read",
        "mark-unread",
        "tag-item",
        "highlight-selection",
        "highlight-with-note",
        "send-to-default-ereader",
        "send-to-ereader-picker",
        "share-last-audience",
        "share-audience-picker",
        "archive-item",
        "previous-unread-with-prompt",
        "next-unread-with-prompt",
        "show-item-info",
    ]
}

/// Apply user overrides to a default layer.
pub fn apply_overrides(layer: &mut Layer, overrides: &[BindingOverride]) {
    let layer_key = layer.key().to_string();

    for override_binding in overrides
        .iter()
        .filter(|override_binding| override_binding.layer == layer_key)
    {
        let action_name = override_binding.action.name();
        let existing = layer
            .bindings()
            .iter()
            .find(|binding| binding.action.name() == action_name);
        let description = existing
            .map(|binding| binding.description.clone())
            .unwrap_or_else(|| action_name.to_string());
        let allow_in_input = existing
            .map(|binding| binding.allow_in_input)
            .unwrap_or(false);

        layer
            .bindings_mut()
            .retain(|binding| binding.action.name() != action_name);

        let mut binding = Binding::new(
            override_binding.sequence.clone(),
            override_binding.action.clone(),
            description,
        );
        binding.allow_in_input = allow_in_input;
        layer.bindings_mut().push(binding);
    }
}

fn bind(sequence: &str, action: &str, description: &str) -> Binding {
    Binding::new(
        Sequence::parse(sequence).expect("default key sequence should parse"),
        Action::named(action),
        description,
    )
}

#[cfg(test)]
mod tests {
    use crate::keymap::{KeymapStack, Sequence};

    use super::*;

    #[test]
    fn default_keymap_layers_have_no_conflicts() {
        let mut stack = KeymapStack::new();

        for layer in [
            global_layer(),
            list_layer(),
            reading_layer(),
            item_action_layer(),
            reading_extra_layer(),
        ] {
            stack.push(layer).unwrap();
        }
    }

    #[test]
    fn all_design_actions_are_registered_as_known() {
        for layer in [
            global_layer(),
            list_layer(),
            reading_layer(),
            item_action_layer(),
            reading_extra_layer(),
        ] {
            for binding in layer.bindings() {
                assert!(is_known_action(binding.action.name()));
            }
        }
    }

    #[test]
    fn overrides_replace_the_default_binding_for_an_action() {
        let mut layer = list_layer();
        let overrides = vec![BindingOverride::new(
            LIST_LAYER,
            Sequence::parse("x").unwrap(),
            Action::named("next-item"),
        )];

        apply_overrides(&mut layer, &overrides);

        assert!(
            layer
                .bindings()
                .iter()
                .any(|binding| binding.action.name() == "next-item"
                    && binding.sequence.to_string() == "x")
        );
        assert!(
            !layer
                .bindings()
                .iter()
                .any(|binding| binding.action.name() == "next-item"
                    && binding.sequence.to_string() == "j")
        );
    }
}
