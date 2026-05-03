//! Per-host rate limiting and robots policy helpers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::sleep_until;
use url::Url;

use crate::error::FetchError;

/// Default maximum concurrent requests per host.
pub const DEFAULT_MAX_CONCURRENT_PER_HOST: usize = 2;

/// Default minimum delay between requests to the same host.
pub const DEFAULT_MIN_INTERVAL_PER_HOST: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct HostLimiter {
    max_concurrent_per_host: usize,
    default_min_interval: Duration,
    hosts: Mutex<HashMap<String, Arc<HostState>>>,
}

impl HostLimiter {
    pub fn new(max_concurrent_per_host: usize, default_min_interval: Duration) -> Self {
        Self {
            max_concurrent_per_host,
            default_min_interval,
            hosts: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, url: &Url) -> Result<HostPermit, FetchError> {
        let host = host_key(url);
        let state = self.state_for(host.clone()).await;
        let permit = state
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| FetchError::RateLimiterClosed { host: host.clone() })?;

        let mut last_request = state.last_request.lock().await;
        let min_interval = *state.min_interval.lock().await;
        if let Some(last) = *last_request {
            let next_allowed = last + min_interval;
            let now = Instant::now();
            if next_allowed > now {
                sleep_until(next_allowed.into()).await;
            }
        }
        *last_request = Some(Instant::now());
        drop(last_request);

        Ok(HostPermit { _permit: permit })
    }

    pub async fn set_min_interval(&self, url: &Url, min_interval: Duration) {
        let state = self.state_for(host_key(url)).await;
        let mut current = state.min_interval.lock().await;
        if min_interval > *current {
            *current = min_interval;
        }
    }

    async fn state_for(&self, host: String) -> Arc<HostState> {
        let mut hosts = self.hosts.lock().await;
        hosts
            .entry(host)
            .or_insert_with(|| {
                Arc::new(HostState {
                    semaphore: Arc::new(Semaphore::new(self.max_concurrent_per_host)),
                    min_interval: Mutex::new(self.default_min_interval),
                    last_request: Mutex::new(None),
                })
            })
            .clone()
    }
}

#[derive(Debug)]
struct HostState {
    semaphore: Arc<Semaphore>,
    min_interval: Mutex<Duration>,
    last_request: Mutex<Option<Instant>>,
}

/// A held per-host rate-limit permit.
#[derive(Debug)]
pub struct HostPermit {
    _permit: OwnedSemaphorePermit,
}

pub fn host_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    match url.port_or_known_default() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), host),
        None => format!("{}://{}", url.scheme(), host),
    }
}
