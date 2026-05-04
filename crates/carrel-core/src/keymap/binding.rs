//! Keymap binding primitives.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::keymap::KeymapError;

/// Modifier keys held with a key press.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Modifiers {
    /// Shift key.
    pub shift: bool,
    /// Control key.
    pub ctrl: bool,
    /// Alt or Option key.
    pub alt: bool,
    /// Meta key, Command on macOS.
    pub meta: bool,
}

impl Modifiers {
    /// Return true when no modifiers are set.
    pub const fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.meta
    }

    /// Return modifiers with shift enabled.
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Return modifiers with control enabled.
    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// Return modifiers with alt enabled.
    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Return modifiers with meta enabled.
    pub const fn with_meta(mut self) -> Self {
        self.meta = true;
        self
    }
}

/// Logical key code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    /// A keyboard-layout-aware character.
    Character(char),
    /// Enter or Return.
    Enter,
    /// Escape.
    Escape,
    /// Space.
    Space,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
}

/// A single key press.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Key {
    /// Logical key code.
    pub code: KeyCode,
    /// Held modifiers.
    pub modifiers: Modifiers,
}

impl Key {
    /// Create a key from a code and modifiers.
    pub const fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        Self { code, modifiers }
    }

    /// Create an unmodified character key.
    pub fn character(value: char) -> Self {
        let lower = value.to_ascii_lowercase();
        let modifiers = if value.is_ascii_uppercase() {
            Modifiers::default().with_shift()
        } else {
            Modifiers::default()
        };

        Self::new(KeyCode::Character(lower), modifiers)
    }

    /// Create a shifted character key.
    pub fn shifted_character(value: char) -> Self {
        Self::new(
            KeyCode::Character(value.to_ascii_lowercase()),
            Modifiers::default().with_shift(),
        )
    }

    /// Create an Enter key.
    pub fn enter() -> Self {
        Self::new(KeyCode::Enter, Modifiers::default())
    }

    /// Create an Escape key.
    pub fn escape() -> Self {
        Self::new(KeyCode::Escape, Modifiers::default())
    }

    /// Create a Space key.
    pub fn space() -> Self {
        Self::new(KeyCode::Space, Modifiers::default())
    }

    /// Return true if this is Escape without modifiers.
    pub const fn is_escape(self) -> bool {
        matches!(self.code, KeyCode::Escape) && self.modifiers.is_empty()
    }

    /// Return true if this is Command/Meta plus the given character.
    pub const fn is_meta_character(self, value: char) -> bool {
        matches!(self.code, KeyCode::Character(c) if c == value)
            && self.modifiers.meta
            && !self.modifiers.ctrl
            && !self.modifiers.alt
    }

    /// Return true if this is Command/Meta plus Enter.
    pub const fn is_meta_enter(self) -> bool {
        matches!(self.code, KeyCode::Enter)
            && self.modifiers.meta
            && !self.modifiers.ctrl
            && !self.modifiers.alt
    }

    /// Parse a single key from user-editable keymap syntax.
    pub fn parse(input: &str) -> Result<Self, KeymapError> {
        parse_key(input)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut mods = Vec::new();
        if self.modifiers.ctrl {
            mods.push("ctrl");
        }
        if self.modifiers.alt {
            mods.push("alt");
        }
        if self.modifiers.meta {
            mods.push("cmd");
        }

        let key = match self.code {
            KeyCode::Character(value)
                if self.modifiers.shift && value.is_ascii_alphabetic() && mods.is_empty() =>
            {
                value.to_ascii_uppercase().to_string()
            }
            KeyCode::Character(value) => {
                if self.modifiers.shift {
                    mods.push("shift");
                }
                value.to_string()
            }
            KeyCode::Enter => {
                if self.modifiers.shift {
                    mods.push("shift");
                }
                "Enter".to_string()
            }
            KeyCode::Escape => {
                if self.modifiers.shift {
                    mods.push("shift");
                }
                "Esc".to_string()
            }
            KeyCode::Space => {
                if self.modifiers.shift {
                    mods.push("S");
                }
                "Space".to_string()
            }
            KeyCode::ArrowLeft => {
                if self.modifiers.shift {
                    mods.push("shift");
                }
                "←".to_string()
            }
            KeyCode::ArrowRight => {
                if self.modifiers.shift {
                    mods.push("shift");
                }
                "→".to_string()
            }
        };

        if mods.is_empty() {
            write!(f, "{key}")
        } else {
            write!(f, "{}-{key}", mods.join("-"))
        }
    }
}

/// One or more key presses.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Sequence(Vec<Key>);

impl Sequence {
    /// Create a non-empty sequence.
    pub fn new(keys: Vec<Key>) -> Result<Self, KeymapError> {
        if keys.is_empty() {
            Err(KeymapError::EmptySequence)
        } else {
            Ok(Self(keys))
        }
    }

    /// Create a one-key sequence.
    pub fn single(key: Key) -> Self {
        Self(vec![key])
    }

    /// Parse a sequence from user-editable keymap syntax.
    pub fn parse(input: &str) -> Result<Self, KeymapError> {
        let keys = input
            .split_whitespace()
            .map(Key::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(keys)
    }

    /// Return the keys in this sequence.
    pub fn keys(&self) -> &[Key] {
        &self.0
    }

    /// Return true if this sequence starts `other`.
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        self.0.len() <= other.0.len() && self.0.iter().zip(other.0.iter()).all(|(a, b)| a == b)
    }

    /// Return true if the two sequences are equal or either prefixes the other.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.is_prefix_of(other) || other.is_prefix_of(self)
    }
}

impl fmt::Display for Sequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self
            .0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "{text}")
    }
}

/// Action produced by a matched binding.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Action {
    /// Stable action identifier.
    Named(String),
}

impl Action {
    /// Create a named action.
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    /// Return this action's stable name.
    pub fn name(&self) -> &str {
        match self {
            Self::Named(name) => name,
        }
    }
}

/// A sequence mapped to an action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Binding {
    /// Key sequence.
    pub sequence: Sequence,
    /// Action to dispatch.
    pub action: Action,
    /// Human-readable description for the help overlay.
    pub description: String,
    /// Whether this binding may run while a text input has focus.
    pub allow_in_input: bool,
}

impl Binding {
    /// Create a binding.
    pub fn new(sequence: Sequence, action: Action, description: impl Into<String>) -> Self {
        Self {
            sequence,
            action,
            description: description.into(),
            allow_in_input: false,
        }
    }

    /// Mark the binding as allowed while text input has focus.
    pub const fn allow_in_input(mut self) -> Self {
        self.allow_in_input = true;
        self
    }
}

fn parse_key(input: &str) -> Result<Key, KeymapError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(invalid_key(input, "empty key"));
    }

    let parts = if trimmed.contains('+') {
        trimmed.split('+').collect::<Vec<_>>()
    } else if trimmed.contains('-') {
        trimmed.split('-').collect::<Vec<_>>()
    } else {
        vec![trimmed]
    };

    let Some((key_part, modifier_parts)) = parts.split_last() else {
        return Err(invalid_key(input, "empty key"));
    };

    let mut modifiers = Modifiers::default();
    for modifier in modifier_parts {
        match modifier.to_ascii_lowercase().as_str() {
            "shift" | "s" => modifiers.shift = true,
            "ctrl" | "control" => modifiers.ctrl = true,
            "alt" | "option" => modifiers.alt = true,
            "cmd" | "command" | "meta" | "super" => modifiers.meta = true,
            _ => return Err(invalid_key(input, "unknown modifier")),
        }
    }

    let code = parse_code(key_part).ok_or_else(|| invalid_key(input, "unknown key"))?;
    let mut key = Key::new(code, modifiers);

    if let KeyCode::Character(value) = key.code
        && value.is_ascii_uppercase()
    {
        key.code = KeyCode::Character(value.to_ascii_lowercase());
        key.modifiers.shift = true;
    }

    Ok(key)
}

fn parse_code(input: &str) -> Option<KeyCode> {
    match input {
        "Enter" | "enter" | "Return" | "return" => Some(KeyCode::Enter),
        "Esc" | "esc" | "Escape" | "escape" => Some(KeyCode::Escape),
        "Space" | "space" => Some(KeyCode::Space),
        "ArrowLeft" | "arrowleft" | "Left" | "left" | "←" => Some(KeyCode::ArrowLeft),
        "ArrowRight" | "arrowright" | "Right" | "right" | "→" => Some(KeyCode::ArrowRight),
        value if value.chars().count() == 1 => value.chars().next().map(KeyCode::Character),
        _ => None,
    }
}

fn invalid_key(input: &str, reason: impl Into<String>) -> KeymapError {
    KeymapError::InvalidKey {
        input: input.to_string(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_key_sequences() {
        let sequence = Sequence::parse("g g").unwrap();
        assert_eq!(sequence.to_string(), "g g");

        assert_eq!(Key::parse("J").unwrap(), Key::shifted_character('j'));
        assert_eq!(Key::parse("cmd-k").unwrap().to_string(), "cmd-k");
        assert_eq!(Key::parse("S-Space").unwrap().to_string(), "S-Space");
        assert_eq!(Key::parse("←").unwrap().to_string(), "←");
    }

    #[test]
    fn rejects_empty_or_unknown_keys() {
        assert!(Sequence::parse("").is_err());
        assert!(Key::parse("ctrl-shift-cmd-banana").is_err());
    }
}
