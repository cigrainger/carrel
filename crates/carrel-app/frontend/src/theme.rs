//! Theme state for the desktop webview.

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "carrel.theme";

use leptos::prelude::*;

/// Color themes supported by the design system.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    #[default]
    Light,
    Sepia,
    Dark,
    Black,
}

impl Theme {
    /// Themes exposed through the developer affordance.
    pub const ALL: [Theme; 4] = [Theme::Light, Theme::Sepia, Theme::Dark, Theme::Black];

    /// Stable value written to `data-theme` and local storage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Sepia => "sepia",
            Theme::Dark => "dark",
            Theme::Black => "black",
        }
    }

    /// Short label for the status-strip control.
    pub const fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Sepia => "Sepia",
            Theme::Dark => "Dark",
            Theme::Black => "Black",
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Theme::Light),
            "sepia" => Some(Theme::Sepia),
            "dark" => Some(Theme::Dark),
            "black" => Some(Theme::Black),
            _ => None,
        }
    }
}

/// Reactive theme controller shared by chrome components.
#[derive(Clone, Copy)]
pub struct ThemeController {
    current: ReadSignal<Theme>,
    set_current: WriteSignal<Theme>,
}

impl ThemeController {
    /// The active theme signal.
    pub const fn current(self) -> ReadSignal<Theme> {
        self.current
    }

    /// Apply and persist a new theme.
    pub fn set(self, theme: Theme) {
        self.set_current.set(theme);
        apply_theme(theme);
        persist_theme(theme);
    }
}

/// Provide theme state to the application tree.
pub fn provide_theme_controller() -> ThemeController {
    let initial = load_theme();
    let (current, set_current) = signal(initial);
    let controller = ThemeController {
        current,
        set_current,
    };

    apply_theme(initial);
    provide_context(controller);
    controller
}

/// Retrieve theme state from context.
pub fn use_theme_controller() -> ThemeController {
    expect_context::<ThemeController>()
}

#[cfg(target_arch = "wasm32")]
fn load_theme() -> Theme {
    local_storage()
        .and_then(|storage| storage.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|value| Theme::from_str(&value))
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn load_theme() -> Theme {
    Theme::default()
}

#[cfg(target_arch = "wasm32")]
fn persist_theme(theme: Theme) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(STORAGE_KEY, theme.as_str());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn persist_theme(_theme: Theme) {}

#[cfg(target_arch = "wasm32")]
fn apply_theme(theme: Theme) {
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute("data-theme", theme.as_str());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_theme(_theme: Theme) {}

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn theme_values_round_trip_from_storage_strings() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_str(theme.as_str()), Some(theme));
        }

        assert_eq!(Theme::from_str("unknown"), None);
    }
}
