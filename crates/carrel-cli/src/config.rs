//! Data-directory resolution and install paths.

use std::env;
use std::path::PathBuf;

use crate::error::{CliError, Result};

const DATA_DIR_ENV: &str = "CARREL_DATA_DIR";

/// Resolved CLI context shared by commands.
#[derive(Clone, Debug)]
pub struct Context {
    /// Root data directory for this invocation.
    pub data_dir: PathBuf,
    /// Whether command output should be JSON where supported.
    pub json: bool,
    /// Common paths inside the data directory.
    pub paths: InstallPaths,
}

impl Context {
    /// Resolve the context from a global `--data-dir` override and environment.
    pub fn resolve(data_dir: Option<PathBuf>, json: bool) -> Result<Self> {
        let data_dir = match data_dir {
            Some(path) => path,
            None => match env::var_os(DATA_DIR_ENV) {
                Some(path) => PathBuf::from(path),
                None => default_data_dir()?,
            },
        };

        let paths = InstallPaths::new(data_dir.clone());
        Ok(Self {
            data_dir,
            json,
            paths,
        })
    }
}

/// Common paths inside an initialized Carrel data directory.
#[derive(Clone, Debug)]
pub struct InstallPaths {
    /// Root data directory.
    pub root: PathBuf,
    /// Cozo store directory.
    pub store: PathBuf,
    /// Blob cache directory.
    pub blobs: PathBuf,
    /// Key directory.
    pub keys: PathBuf,
    /// Human-editable config file.
    pub config: PathBuf,
}

impl InstallPaths {
    fn new(root: PathBuf) -> Self {
        Self {
            store: root.join("store"),
            blobs: root.join("blobs"),
            keys: root.join("keys"),
            config: root.join("carrel.toml"),
            root,
        }
    }

    /// Return true if this path appears to contain a Carrel install.
    pub fn is_initialized(&self) -> bool {
        self.config.exists() || self.store.exists() || self.keys.exists()
    }

    /// Require an initialized data directory.
    pub fn require_initialized(&self) -> Result<()> {
        if self.is_initialized() {
            Ok(())
        } else {
            Err(CliError::user(format!(
                "Carrel is not initialized at {}. Run `carrel-cli init` first.",
                self.root.display()
            )))
        }
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
        .ok_or_else(|| CliError::user("could not determine home directory; pass --data-dir"))
}
