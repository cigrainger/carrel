//! Keymap configuration command.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use carrel_core::keymap::default;
use carrel_core::keymap::{Action, Sequence};
use serde::Serialize;
use tauri::State;
use toml::Value;
use toml::map::Map;

use crate::Result;
use crate::state::AppState;

/// User keymap configuration returned to the webview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapConfig {
    /// Resolved keymap file path.
    pub path: String,
    /// Valid override bindings.
    pub overrides: Vec<KeymapBindingOverride>,
    /// Non-fatal configuration warnings.
    pub warnings: Vec<String>,
}

/// One valid user binding override.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapBindingOverride {
    /// Layer key the override applies to.
    pub layer: String,
    /// User-editable key sequence.
    pub key: String,
    /// Stable action name.
    pub action: String,
}

/// Load and validate user keymap overrides.
#[tauri::command]
pub fn keymap_config(state: State<'_, AppState>) -> Result<KeymapConfig> {
    Ok(keymap_config_from_path(&state.paths.keymap_config))
}

pub(crate) fn keymap_config_from_path(path: &Path) -> KeymapConfig {
    let mut config = KeymapConfig {
        path: path.display().to_string(),
        overrides: Vec::new(),
        warnings: Vec::new(),
    };

    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return config,
        Err(error) => {
            config.warnings.push(format!(
                "Could not read keymap config {}: {error}",
                path.display()
            ));
            return config;
        }
    };

    let value = match toml::from_str::<Value>(&text) {
        Ok(value) => value,
        Err(error) => {
            config.warnings.push(format!(
                "Could not parse keymap config {}: {error}",
                path.display()
            ));
            return config;
        }
    };

    parse_keymap_value(&value, &mut config);
    config
}

fn parse_keymap_value(value: &Value, config: &mut KeymapConfig) {
    let Some(bindings) = value.get("bindings") else {
        return;
    };

    let Some(table) = bindings.as_table() else {
        config
            .warnings
            .push("Keymap config [bindings] must be a table".to_string());
        return;
    };

    parse_bindings_table(default::LIST_LAYER, table, config);
}

fn parse_bindings_table(
    default_layer: &str,
    table: &Map<String, Value>,
    config: &mut KeymapConfig,
) {
    for (key, value) in table {
        if let Some(action) = value.as_str() {
            parse_binding(default_layer, key, action, config);
            continue;
        }

        if let Some(layer_table) = value.as_table() {
            if let Some(layer) = normalize_layer(key) {
                parse_bindings_table(layer, layer_table, config);
            } else {
                config
                    .warnings
                    .push(format!("Unknown keymap layer `{key}`"));
            }
            continue;
        }

        config
            .warnings
            .push(format!("Keymap binding `{key}` must map to an action name"));
    }
}

fn parse_binding(layer: &str, key: &str, action: &str, config: &mut KeymapConfig) {
    if !default::is_known_action(action) {
        config
            .warnings
            .push(format!("Unknown keymap action `{action}` for `{key}`"));
        return;
    }

    if let Err(error) = Sequence::parse(key) {
        config
            .warnings
            .push(format!("Invalid keymap binding `{key}`: {error}"));
        return;
    }

    config.overrides.push(KeymapBindingOverride {
        layer: layer.to_string(),
        key: key.to_string(),
        action: Action::named(action).name().to_string(),
    });
}

fn normalize_layer(layer: &str) -> Option<&'static str> {
    match layer {
        "global" => Some(default::GLOBAL_LAYER),
        "list" => Some(default::LIST_LAYER),
        "reading" => Some(default::READING_LAYER),
        "item-actions" | "item_actions" => Some(default::ITEM_ACTION_LAYER),
        "reading-extra" | "reading_extra" => Some(default::READING_EXTRA_LAYER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_keymap_overrides() {
        let value = toml::from_str::<Value>(
            r#"
            [bindings]
            "x" = "next-item"
            "shift+x" = "next-unread-item"

            [bindings.reading]
            "h" = "highlight-selection"
        "#,
        )
        .unwrap();
        let mut config = KeymapConfig {
            path: "/tmp/keymap.toml".to_string(),
            overrides: Vec::new(),
            warnings: Vec::new(),
        };

        parse_keymap_value(&value, &mut config);

        config
            .overrides
            .sort_by(|left, right| (&left.layer, &left.key).cmp(&(&right.layer, &right.key)));

        assert_eq!(
            config.overrides,
            vec![
                KeymapBindingOverride {
                    layer: "list".to_string(),
                    key: "shift+x".to_string(),
                    action: "next-unread-item".to_string(),
                },
                KeymapBindingOverride {
                    layer: "list".to_string(),
                    key: "x".to_string(),
                    action: "next-item".to_string(),
                },
                KeymapBindingOverride {
                    layer: "reading".to_string(),
                    key: "h".to_string(),
                    action: "highlight-selection".to_string(),
                },
            ]
        );
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn invalid_keymap_entries_warn_without_overrides() {
        let value = toml::from_str::<Value>(
            r#"
            [bindings]
            "ctrl-shift-cmd-banana" = "next-item"
            "j" = "not-real"

            [bindings.elsewhere]
            "x" = "next-item"
        "#,
        )
        .unwrap();
        let mut config = KeymapConfig {
            path: "/tmp/keymap.toml".to_string(),
            overrides: Vec::new(),
            warnings: Vec::new(),
        };

        parse_keymap_value(&value, &mut config);

        assert!(config.overrides.is_empty());
        assert_eq!(config.warnings.len(), 3);
    }
}
