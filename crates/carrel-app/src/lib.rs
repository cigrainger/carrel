//! Tauri shell for the Carrel desktop application.

#![deny(unsafe_code)]

mod commands;
mod config;
mod error;
mod state;

pub use crate::error::{AppError, Result};

/// Start the desktop application.
pub fn run() -> Result<()> {
    init_tracing();

    let paths = config::InstallPaths::resolve()?;
    let store = carrel_store::Store::open(&paths.store)?;
    store.migrate()?;

    tauri::Builder::default()
        .manage(state::AppState::new(store, paths))
        .invoke_handler(tauri::generate_handler![
            commands::items::get_item,
            commands::items::list_items,
            commands::keymap::keymap_config,
            commands::status::version,
        ])
        .run(tauri::generate_context!())
        .map_err(AppError::from)
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "carrel_app=info,carrel_store=warn,tauri=warn".into());

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .without_time()
        .try_init();
}
