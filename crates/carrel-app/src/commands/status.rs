//! App status and install metadata commands.

use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use crate::{Result, config::InstallPaths};

/// Desktop shell and data-store version metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// Cargo package version for the desktop shell.
    pub app_version: String,
    /// Current migrated schema version.
    pub schema_version: u32,
    /// Resolved local data directory.
    pub data_dir: String,
}

/// Return app and local store version metadata.
#[tauri::command]
pub fn version(state: State<'_, AppState>) -> Result<VersionInfo> {
    version_from_parts(&state.store, &state.paths)
}

pub(crate) fn version_from_parts(
    store: &carrel_store::Store,
    paths: &InstallPaths,
) -> Result<VersionInfo> {
    Ok(VersionInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: store.current_schema_version()?,
        data_dir: paths.display_root(),
    })
}

#[cfg(test)]
mod tests {
    use carrel_store::Store;

    use crate::config::InstallPaths;

    use super::*;

    #[test]
    fn reports_package_and_schema_versions() {
        let store = Store::open_in_memory().unwrap();
        store.migrate().unwrap();
        let paths = InstallPaths {
            keymap_config: "/tmp/carrel-test/config/keymap.toml".into(),
            root: "/tmp/carrel-test".into(),
            store: "/tmp/carrel-test/store".into(),
        };

        let info = version_from_parts(&store, &paths).unwrap();

        assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.schema_version, carrel_store::CURRENT_SCHEMA_VERSION);
        assert_eq!(info.data_dir, "/tmp/carrel-test");
    }
}
