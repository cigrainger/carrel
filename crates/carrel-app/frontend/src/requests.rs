//! Route lifetime guards for asynchronous responses.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) struct Requests(Arc<AtomicU64>);

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl Requests {
    pub(crate) fn begin(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub(crate) fn accepts(&self, request: u64) -> bool {
        self.0.load(Ordering::Relaxed) == request
    }
    pub(crate) fn invalidate(&self) {
        self.begin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slower_previous_request_cannot_replace_a_newer_result() {
        let requests = Requests::default();
        let old = requests.begin();
        let current = requests.begin();
        assert!(requests.accepts(current));
        assert!(!requests.accepts(old));
        requests.invalidate();
        assert!(!requests.accepts(current));
    }
}
