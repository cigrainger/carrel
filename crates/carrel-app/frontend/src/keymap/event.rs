//! Keyboard event translation for keymap dispatch.

#[cfg(target_arch = "wasm32")]
use carrel_core::keymap::{Key, KeyCode, Modifiers};

/// Return true when this key may bypass text-input suspension.
#[cfg(target_arch = "wasm32")]
pub fn is_global_passthrough_key(key: Key) -> bool {
    key.is_escape() || key.is_meta_character('k') || key.is_meta_enter()
}

/// Translate a browser keyboard event to the core key representation.
#[cfg(target_arch = "wasm32")]
pub fn key_from_event(event: &web_sys::KeyboardEvent) -> Option<Key> {
    let mut modifiers = Modifiers {
        shift: false,
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    };

    let code = match event.key().as_str() {
        "Enter" => {
            modifiers.shift = event.shift_key();
            KeyCode::Enter
        }
        "Escape" | "Esc" => {
            modifiers.shift = event.shift_key();
            KeyCode::Escape
        }
        "ArrowLeft" => {
            modifiers.shift = event.shift_key();
            KeyCode::ArrowLeft
        }
        "ArrowRight" => {
            modifiers.shift = event.shift_key();
            KeyCode::ArrowRight
        }
        " " | "Spacebar" => {
            modifiers.shift = event.shift_key();
            KeyCode::Space
        }
        value => {
            let (code, shift) = key_code_from_character(value, event.shift_key())?;
            modifiers.shift = shift;
            code
        }
    };

    Some(Key::new(code, modifiers))
}

#[cfg(target_arch = "wasm32")]
fn key_code_from_character(value: &str, shifted: bool) -> Option<(KeyCode, bool)> {
    let mut chars = value.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    if character.is_ascii_alphabetic() {
        return Some((KeyCode::Character(character.to_ascii_lowercase()), shifted));
    }

    Some((KeyCode::Character(character.to_ascii_lowercase()), false))
}

/// Return true when focus is inside a text-editing element.
#[cfg(target_arch = "wasm32")]
pub fn is_text_input_focused() -> bool {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return false;
    };
    let Some(element) = document.active_element() else {
        return false;
    };

    let tag = element.tag_name().to_ascii_lowercase();
    if matches!(tag.as_str(), "input" | "textarea" | "select") {
        return true;
    }

    element
        .get_attribute("contenteditable")
        .is_some_and(|value| value.is_empty() || value.eq_ignore_ascii_case("true"))
}
