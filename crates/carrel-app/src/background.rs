//! Background workers owned by the desktop shell.

use std::time::Duration;

use tracing::warn;

use crate::commands::feeds;
use crate::state::{AppState, FeedStatusCounts};

const FEED_LOOP_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the conservative feed fetch loop.
pub(crate) fn spawn_feed_loop(state: AppState) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(error) = feeds::fetch_due_feeds(state.clone()).await {
                warn!(%error, "background feed fetch failed");
                state.feed_status.set_idle(
                    format!("Background feed fetch failed: {error}"),
                    FeedStatusCounts {
                        feed_errors: 1,
                        ..FeedStatusCounts::default()
                    },
                );
            }

            tokio::time::sleep(FEED_LOOP_INTERVAL).await;
        }
    });
}
