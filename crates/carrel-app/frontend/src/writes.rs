//! Track local saves so normal desktop close does not discard queued work.

use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use leptos::prelude::*;

#[derive(Default)]
struct WriteState {
    pending: usize,
    waiters: Vec<Waker>,
}

#[derive(Clone, Default)]
pub(crate) struct PendingWrites(Arc<Mutex<WriteState>>);

pub(crate) struct WriteGuard(Arc<Mutex<WriteState>>);

impl PendingWrites {
    pub(crate) fn begin(&self) -> WriteGuard {
        self.0.lock().expect("write tracker poisoned").pending += 1;
        WriteGuard(self.0.clone())
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) async fn wait_until_idle(&self) {
        poll_fn(|cx| {
            let mut state = self.0.lock().expect("write tracker poisoned");
            if state.pending == 0 {
                Poll::Ready(())
            } else {
                if !state
                    .waiters
                    .iter()
                    .any(|waker| waker.will_wake(cx.waker()))
                {
                    state.waiters.push(cx.waker().clone());
                }
                Poll::Pending
            }
        })
        .await;
    }
}

impl Drop for WriteGuard {
    fn drop(&mut self) {
        let waiters = {
            let mut state = self.0.lock().expect("write tracker poisoned");
            state.pending -= 1;
            if state.pending == 0 {
                std::mem::take(&mut state.waiters)
            } else {
                Vec::new()
            }
        };
        for waker in waiters {
            waker.wake();
        }
    }
}

pub(crate) fn use_pending_writes() -> PendingWrites {
    // Standalone SSR components have no desktop close coordinator.
    use_context::<PendingWrites>().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Wake};

    #[derive(Default)]
    struct WakeCounter(AtomicUsize);

    impl Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn close_waits_for_all_saves_including_work_added_while_waiting() {
        let writes = PendingWrites::default();
        let first = writes.begin();
        let second = writes.begin();
        let counter = Arc::new(WakeCounter::default());
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        let mut idle = pin!(writes.wait_until_idle());
        assert!(idle.as_mut().poll(&mut cx).is_pending());
        assert!(idle.as_mut().poll(&mut cx).is_pending());
        drop(first);
        assert_eq!(counter.0.load(Ordering::SeqCst), 0);
        let third = writes.begin();
        drop(second);
        assert!(idle.as_mut().poll(&mut cx).is_pending());
        drop(third);
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(idle.as_mut().poll(&mut cx).is_ready());
    }

    #[test]
    fn an_empty_tracker_is_ready_and_a_failed_or_cancelled_task_releases_its_guard() {
        let writes = PendingWrites::default();
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(
            pin!(writes.wait_until_idle())
                .as_mut()
                .poll(&mut cx)
                .is_ready()
        );
        {
            let _failed_save = writes.begin();
            assert!(
                pin!(writes.wait_until_idle())
                    .as_mut()
                    .poll(&mut cx)
                    .is_pending()
            );
        }
        assert!(
            pin!(writes.wait_until_idle())
                .as_mut()
                .poll(&mut cx)
                .is_ready()
        );
    }
}
