//! Coordinate normal native close with the frontend's queued local saves.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State, Window, WindowEvent};

#[derive(Default)]
struct CloseStatus {
    ready: bool,
    pending: bool,
    allowed: bool,
    restart: bool,
}

impl CloseStatus {
    fn request(&mut self) -> bool {
        if !self.ready || self.allowed {
            return false;
        }
        self.pending = true;
        true
    }

    fn finish(&mut self) -> bool {
        if !self.pending {
            return false;
        }
        self.allowed = true;
        true
    }

    fn cancel(&mut self) {
        self.pending = false;
        self.restart = false;
    }
}

#[derive(Default)]
pub(crate) struct CloseState(Mutex<CloseStatus>);

#[tauri::command]
pub(crate) fn frontend_ready(state: State<'_, CloseState>) {
    state.0.lock().expect("close state poisoned").ready = true;
}

#[tauri::command]
pub(crate) fn cancel_app_close(state: State<'_, CloseState>) {
    state.0.lock().expect("close state poisoned").cancel();
}

#[tauri::command]
pub(crate) async fn finish_app_close(
    app: AppHandle,
    state: State<'_, CloseState>,
    data: State<'_, crate::state::AppState>,
) -> crate::Result<()> {
    let (allowed, restart) = {
        let mut status = state.0.lock().expect("close state poisoned");
        (status.finish(), status.restart)
    };
    if allowed {
        crate::sync::shutdown(&data).await;
        if restart {
            app.request_restart();
        } else {
            app.exit(0);
        }
    }
    Ok(())
}

pub(crate) fn request_restore_restart(app: &AppHandle) -> crate::Result<()> {
    app.state::<CloseState>()
        .0
        .lock()
        .expect("close state poisoned")
        .restart = true;
    if !request_close(app) {
        app.state::<CloseState>()
            .0
            .lock()
            .expect("close state poisoned")
            .restart = false;
        return Err(crate::AppError::Config(
            "Restore is prepared. Quit and reopen Carrel to finish it.".into(),
        ));
    }
    Ok(())
}

fn request_close(app: &AppHandle) -> bool {
    let state = app.state::<CloseState>();
    let mut status = state.0.lock().expect("close state poisoned");
    let already_pending = status.pending;
    if !status.request() {
        return false;
    }
    drop(status);
    if !already_pending {
        let result = app
            .get_webview_window("main")
            .map(|window| window.eval("window.dispatchEvent(new Event('carrel-close-requested'))"));
        if !matches!(result, Some(Ok(()))) {
            state.0.lock().expect("close state poisoned").cancel();
            tracing::warn!("Could not request frontend save before close");
        }
    }
    true
}

pub(crate) fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event
        && request_close(window.app_handle())
    {
        api.prevent_close();
    }
}

pub(crate) fn on_run_event(app: &AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::ExitRequested { api, .. } = event
        && request_close(app)
    {
        api.prevent_exit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_can_close_before_the_frontend_is_ready() {
        let mut state = CloseStatus::default();
        assert!(!state.request());
        assert!(!state.finish());
    }

    #[test]
    fn normal_close_and_quit_wait_for_a_save_acknowledgement() {
        let mut state = CloseStatus {
            ready: true,
            ..Default::default()
        };
        assert!(!state.finish());
        assert!(state.request());
        assert!(state.request());
        assert!(state.finish());
        assert!(!state.request());
    }

    #[test]
    fn failed_save_leaves_the_window_open_and_the_next_close_can_retry() {
        let mut state = CloseStatus {
            ready: true,
            ..Default::default()
        };
        assert!(state.request());
        state.cancel();
        assert!(!state.finish());
        assert!(state.request());
        assert!(state.finish());
        assert!(!state.request());
    }

    #[test]
    fn cancelled_save_does_not_turn_a_later_ordinary_quit_into_a_restart() {
        let mut state = CloseStatus {
            ready: true,
            restart: true,
            ..Default::default()
        };
        assert!(state.request());
        state.cancel();
        assert!(!state.restart);
        assert!(state.request());
        assert!(state.finish());
        assert!(!state.restart);
    }
}
