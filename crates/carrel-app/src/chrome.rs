//! Native window presentation; the webview continues to own reading preferences.

use serde::Deserialize;
use tauri::{Theme, WebviewWindow, Window, WindowEvent, window::Color};

/// The same stable theme names used by the reading webview.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Appearance {
    Light,
    Sepia,
    Dark,
    Black,
}

impl Appearance {
    fn native_theme(self) -> Theme {
        match self {
            Self::Light | Self::Sepia => Theme::Light,
            Self::Dark | Self::Black => Theme::Dark,
        }
    }

    fn backing_color(self) -> Color {
        match self {
            Self::Light => Color(0x17, 0x3d, 0x32, 255),
            Self::Sepia => Color(0x46, 0x39, 0x23, 255),
            Self::Dark => Color(0x10, 0x2c, 0x22, 255),
            Self::Black => Color(0x0b, 0x1e, 0x17, 255),
        }
    }
}

#[tauri::command]
pub(crate) fn set_window_appearance(
    window: WebviewWindow,
    appearance: Appearance,
) -> tauri::Result<()> {
    window.set_theme(Some(appearance.native_theme()))?;
    window.set_background_color(Some(appearance.backing_color()))
}

pub(crate) fn initialization_script() -> &'static str {
    if cfg!(target_os = "macos") {
        // Tauri concatenates this after its invoke script, which ends in `})()`.
        // The leading semicolon prevents JavaScript from calling that result.
        r#";(() => {
          const apply = () => document.documentElement.setAttribute('data-platform', 'macos');
          if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', apply, { once: true });
          } else { apply(); }
        })();"#
    } else {
        ""
    }
}

pub(crate) fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::Focused(focused) = event {
        use tauri::Manager;
        if let Some(webview) = window.get_webview_window(window.label()) {
            let script = if *focused {
                "document.documentElement?.setAttribute('data-window-active', 'true');"
            } else {
                "document.documentElement?.setAttribute('data-window-active', 'false');"
            };
            if let Err(error) = webview.eval(script) {
                tracing::debug!(%error, "could not update window focus presentation");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_bootstrap_delimits_an_unterminated_preceding_script() {
        assert!(initialization_script().trim_start().starts_with(';'));
        assert!(initialization_script().trim_end().ends_with(';'));
    }

    #[test]
    fn reading_themes_select_matching_native_control_appearance() {
        for name in ["light", "sepia", "dark", "black"] {
            let appearance: Appearance =
                serde_json::from_value(serde_json::Value::String(name.into())).unwrap();
            let expected = if matches!(name, "dark" | "black") {
                Theme::Dark
            } else {
                Theme::Light
            };
            assert_eq!(appearance.native_theme(), expected);

            let Color(red, green, blue, _) = appearance.backing_color();
            let color = format!("--color-nav-bg: #{red:02x}{green:02x}{blue:02x};");
            let stylesheet = include_str!("../frontend/style/tokens.css");
            let section = if name == "light" {
                stylesheet.split("[data-theme=").next().unwrap()
            } else {
                stylesheet
                    .split(&format!("[data-theme=\"{name}\"]"))
                    .nth(1)
                    .unwrap()
                    .split('}')
                    .next()
                    .unwrap()
            };
            assert!(
                section.contains(&color),
                "native backing diverged for {name}"
            );
        }
        assert!(serde_json::from_str::<Appearance>("\"unknown\"").is_err());
    }
}
