//! Normal desktop close flushes the reader and waits for local saves.

pub(crate) fn install_close_handler() {
    #[cfg(target_arch = "wasm32")]
    {
        use leptos::ev;
        use leptos::prelude::*;

        let writer = crate::progress::use_progress_writer();
        let writes = crate::writes::use_pending_writes();
        let status = crate::status::use_status_controller();
        let listener = window_event_listener(
            ev::Custom::<web_sys::Event>::new("carrel-close-requested"),
            move |_| {
                // Scroll notifications may not yet have reached the progress queue.
                if let Some(scroll) = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.query_selector(".reader-scroll").ok().flatten())
                    && let Ok(event) = web_sys::Event::new("scroll")
                {
                    let _ = scroll.dispatch_event(&event);
                }
                writer.flush_for_close();
                let writer = writer.clone();
                let writes = writes.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    writes.wait_until_idle().await;
                    if writer.has_unsaved_positions() {
                        status.warn("Could not save your reading position. Close again to retry.");
                        let _ = crate::api::cancel_app_close().await;
                    } else if crate::api::finish_app_close().await.is_err() {
                        status.warn("Could not close Carrel. Please try again.");
                        let _ = crate::api::cancel_app_close().await;
                    }
                });
            },
        );
        on_cleanup(move || listener.remove());
        wasm_bindgen_futures::spawn_local(async {
            let _ = crate::api::frontend_ready().await;
        });
    }
}
