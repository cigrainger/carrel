//! Application data-directory resolution.

use std::env;
use std::path::PathBuf;

use crate::{AppError, Result};

const DATA_DIR_ENV: &str = "CARREL_DATA_DIR";

/// Paths inside the local Carrel install.
#[derive(Clone, Debug)]
pub struct InstallPaths {
    pub(crate) root: PathBuf,
    pub(crate) store: PathBuf,
}

impl InstallPaths {
    pub(crate) fn resolve() -> Result<Self> {
        let root = env::var_os(DATA_DIR_ENV)
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(default_data_dir)?;

        Ok(Self {
            store: root.join("store"),
            root,
        })
    }

    pub(crate) fn display_root(&self) -> String {
        self.root.display().to_string()
    }
}

fn default_data_dir() -> Result<PathBuf> {
    default_data_dir_for_platform()
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

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Config("could not determine home directory".to_string()))
}
