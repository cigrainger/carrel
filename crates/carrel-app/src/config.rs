//! Application data-directory resolution.

use std::env;
use std::path::{Path, PathBuf};

use crate::{AppError, Result};

const DATA_DIR_ENV: &str = "CARREL_DATA_DIR";
const CONFIG_DIR_ENV: &str = "CARREL_CONFIG_DIR";

/// Paths inside the local Carrel install.
#[derive(Clone, Debug)]
pub struct InstallPaths {
    pub(crate) blobs: PathBuf,
    pub(crate) keys: PathBuf,
    pub(crate) keymap_config: PathBuf,
    pub(crate) root: PathBuf,
    pub(crate) store: PathBuf,
}

impl InstallPaths {
    pub(crate) fn resolve() -> Result<Self> {
        let root = env::var_os(DATA_DIR_ENV)
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(default_data_dir)?;
        let config_root = env::var_os(CONFIG_DIR_ENV)
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(default_config_dir)?;

        Ok(Self::from_roots(root, config_root))
    }

    pub(crate) fn for_profile(profile_root: &Path) -> Self {
        Self::from_roots(profile_root.join("data"), profile_root.join("config"))
    }

    fn from_roots(root: PathBuf, config_root: PathBuf) -> Self {
        Self {
            blobs: root.join("blobs"),
            keys: root.join("keys"),
            keymap_config: config_root.join("keymap.toml"),
            store: root.join("store"),
            root,
        }
    }

    pub(crate) fn display_root(&self) -> String {
        self.root.display().to_string()
    }
}

fn default_data_dir() -> Result<PathBuf> {
    default_data_dir_for_platform()
}

fn default_config_dir() -> Result<PathBuf> {
    default_config_dir_for_platform()
}

#[cfg(target_os = "macos")]
fn default_data_dir_for_platform() -> Result<PathBuf> {
    Ok(home_dir()?.join("Library/Application Support/Carrel"))
}

#[cfg(target_os = "linux")]
fn default_data_dir_for_platform() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        Ok(PathBuf::from(path).join("carrel"))
    } else {
        Ok(home_dir()?.join(".local/share/carrel"))
    }
}

#[cfg(target_os = "windows")]
fn default_data_dir_for_platform() -> Result<PathBuf> {
    if let Some(path) = env::var_os("APPDATA") {
        Ok(PathBuf::from(path).join("Carrel"))
    } else {
        Ok(home_dir()?.join("AppData/Roaming/Carrel"))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn default_data_dir_for_platform() -> Result<PathBuf> {
    Ok(home_dir()?.join(".carrel"))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn default_config_dir_for_platform() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        Ok(PathBuf::from(path).join("carrel"))
    } else {
        Ok(home_dir()?.join(".config/carrel"))
    }
}

#[cfg(target_os = "windows")]
fn default_config_dir_for_platform() -> Result<PathBuf> {
    if let Some(path) = env::var_os("APPDATA") {
        Ok(PathBuf::from(path).join("Carrel"))
    } else {
        Ok(home_dir()?.join(".config/carrel"))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn default_config_dir_for_platform() -> Result<PathBuf> {
    Ok(home_dir()?.join(".config/carrel"))
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Config("could not determine home directory".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_paths_are_all_beneath_the_profile_without_environment_fallbacks() {
        let profile = env::temp_dir().join("carrel-profile-paths");
        let paths = InstallPaths::for_profile(&profile);
        assert_eq!(paths.root, profile.join("data"));
        assert_eq!(paths.store, profile.join("data/store"));
        assert_eq!(paths.blobs, profile.join("data/blobs"));
        assert_eq!(paths.keys, profile.join("data/keys"));
        assert_eq!(paths.keymap_config, profile.join("config/keymap.toml"));
    }
}
