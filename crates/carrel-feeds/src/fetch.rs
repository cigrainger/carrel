//! Polite HTTP fetching for subscribed feeds.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{
    ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER,
};
use reqwest::{Client, StatusCode};
use texting_robots::{Robot, get_robots_url};
use tokio::sync::Mutex;
use url::Url;

use crate::error::FetchError;
use crate::politeness::{
    DEFAULT_MAX_CONCURRENT_PER_HOST, DEFAULT_MIN_INTERVAL_PER_HOST, HostLimiter, host_key,
};

/// Default user agent sent by Carrel's feed fetcher.
pub const DEFAULT_USER_AGENT: &str = "Carrel/0.1 (+https://carrel.example)";

/// Result of a conditional feed fetch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchResult {
    /// The feed has changed and a response body is available.
    Updated {
        /// Response body bytes.
        body: Bytes,
        /// HTTP metadata useful for the next fetch.
        headers: HttpHeaders,
    },
    /// The server reported that the feed has not changed.
    NotModified {
        /// HTTP metadata useful for the next fetch.
        headers: HttpHeaders,
    },
    /// The server returned a non-success status that should be recorded.
    GoneOrError {
        /// HTTP status code.
        status: u16,
        /// HTTP metadata, including Retry-After if supplied.
        headers: HttpHeaders,
    },
}

/// HTTP metadata retained across feed fetches.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HttpHeaders {
    /// ETag value returned by the server.
    pub etag: Option<String>,
    /// Last-Modified value returned by the server.
    pub last_modified: Option<String>,
    /// Retry-After delay in seconds, when supplied.
    pub retry_after_seconds: Option<i64>,
}

impl HttpHeaders {
    fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            etag: header_to_string(headers, ETAG),
            last_modified: header_to_string(headers, LAST_MODIFIED),
            retry_after_seconds: retry_after_seconds(headers),
        }
    }
}

/// Feed fetcher configuration.
#[derive(Clone, Debug)]
pub struct FetcherConfig {
    /// Maximum concurrent requests to a single host.
    pub max_concurrent_per_host: usize,
    /// Minimum interval between requests to a single host.
    pub min_interval_per_host: Duration,
    /// How long a robots.txt policy remains cached.
    pub robots_cache_ttl: Duration,
    /// Per-request timeout.
    pub request_timeout: Duration,
}

impl Default for FetcherConfig {
    fn default() -> Self {
        Self {
            max_concurrent_per_host: DEFAULT_MAX_CONCURRENT_PER_HOST,
            min_interval_per_host: DEFAULT_MIN_INTERVAL_PER_HOST,
            robots_cache_ttl: Duration::from_secs(24 * 60 * 60),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// A polite feed fetcher with per-host rate limiting and robots.txt caching.
#[derive(Clone, Debug)]
pub struct Fetcher {
    client: Client,
    user_agent: String,
    limiter: Arc<HostLimiter>,
    robots_cache: Arc<Mutex<HashMap<String, CachedRobots>>>,
    robots_cache_ttl: Duration,
}

impl Fetcher {
    /// Build a fetcher using the default configuration.
    pub fn new(user_agent: &str) -> Result<Self, FetchError> {
        Self::with_config(user_agent, FetcherConfig::default())
    }

    /// Build a fetcher with explicit politeness settings.
    pub fn with_config(user_agent: &str, config: FetcherConfig) -> Result<Self, FetchError> {
        let client = Client::builder()
            .user_agent(user_agent)
            .timeout(config.request_timeout)
            .build()
            .map_err(FetchError::ClientBuild)?;

        Ok(Self {
            client,
            user_agent: user_agent.to_string(),
            limiter: Arc::new(HostLimiter::new(
                config.max_concurrent_per_host,
                config.min_interval_per_host,
            )),
            robots_cache: Arc::new(Mutex::new(HashMap::new())),
            robots_cache_ttl: config.robots_cache_ttl,
        })
    }

    /// Fetch a feed with optional ETag and Last-Modified conditional headers.
    pub async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResult, FetchError> {
        let url = parse_http_url(url)?;
        self.ensure_robots_allowed(&url).await?;

        let _permit = self.limiter.acquire(&url).await?;
        let mut request = self.client.get(url.clone());
        if let Some(etag) = etag.filter(|value| !value.is_empty()) {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = last_modified.filter(|value| !value.is_empty()) {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }

        let response = request.send().await.map_err(|source| FetchError::Request {
            url: url.to_string(),
            source,
        })?;
        let status = response.status();
        let headers = HttpHeaders::from_headers(response.headers());

        if status == StatusCode::NOT_MODIFIED {
            return Ok(FetchResult::NotModified { headers });
        }

        if status.is_success() {
            let body = response.bytes().await.map_err(|source| FetchError::Body {
                url: url.to_string(),
                source,
            })?;
            return Ok(FetchResult::Updated { body, headers });
        }

        Ok(FetchResult::GoneOrError {
            status: status.as_u16(),
            headers,
        })
    }

    async fn ensure_robots_allowed(&self, url: &Url) -> Result<(), FetchError> {
        let host = host_key(url);
        if let Some(policy) = self.cached_policy(&host).await {
            return self.apply_policy(url, &policy).await;
        }

        let policy = self.fetch_robots_policy(url).await?;
        self.cache_policy(host, policy).await;

        let host = host_key(url);
        let policy = self
            .cached_policy(&host)
            .await
            .expect("robots policy just cached");
        self.apply_policy(url, &policy).await
    }

    async fn fetch_robots_policy(&self, url: &Url) -> Result<RobotsPolicy, FetchError> {
        let robots_url = get_robots_url(url.as_str()).map_err(|source| FetchError::InvalidUrl {
            url: url.to_string(),
            source,
        })?;
        let robots_url = parse_http_url(&robots_url)?;

        let _permit = self.limiter.acquire(&robots_url).await?;
        let response = self
            .client
            .get(robots_url.clone())
            .send()
            .await
            .map_err(|source| FetchError::Request {
                url: robots_url.to_string(),
                source,
            })?;
        let status = response.status();
        let headers = HttpHeaders::from_headers(response.headers());

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(FetchError::RobotsRateLimited {
                url: url.to_string(),
                retry_after: headers
                    .retry_after_seconds
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .map(Duration::from_secs),
            });
        }

        if status.is_client_error() {
            return Ok(RobotsPolicy::AllowAll);
        }

        if !status.is_success() {
            return Err(FetchError::RobotsUnavailable {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }

        let body = response.bytes().await.map_err(|source| FetchError::Body {
            url: robots_url.to_string(),
            source,
        })?;
        let robot =
            Robot::new(&self.user_agent, &body).map_err(|source| FetchError::RobotsParse {
                url: url.to_string(),
                message: source.to_string(),
            })?;

        if let Some(delay) = robot.delay.and_then(duration_from_seconds) {
            self.limiter.set_min_interval(url, delay).await;
        }

        Ok(RobotsPolicy::Robot(Arc::new(robot)))
    }

    async fn cached_policy(&self, host: &str) -> Option<RobotsPolicy> {
        let cache = self.robots_cache.lock().await;
        let cached = cache.get(host)?;
        if cached.fetched_at.elapsed() <= self.robots_cache_ttl {
            Some(cached.policy.clone())
        } else {
            None
        }
    }

    async fn cache_policy(&self, host: String, policy: RobotsPolicy) {
        let mut cache = self.robots_cache.lock().await;
        cache.insert(
            host,
            CachedRobots {
                fetched_at: Instant::now(),
                policy,
            },
        );
    }

    async fn apply_policy(&self, url: &Url, policy: &RobotsPolicy) -> Result<(), FetchError> {
        match policy {
            RobotsPolicy::AllowAll => Ok(()),
            RobotsPolicy::Robot(robot) if robot.allowed(url.as_str()) => {
                if let Some(delay) = robot.delay.and_then(duration_from_seconds) {
                    self.limiter.set_min_interval(url, delay).await;
                }
                Ok(())
            }
            RobotsPolicy::Robot(_) => Err(FetchError::RobotsDisallowed {
                url: url.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct CachedRobots {
    fetched_at: Instant,
    policy: RobotsPolicy,
}

#[derive(Clone, Debug)]
enum RobotsPolicy {
    AllowAll,
    Robot(Arc<Robot>),
}

fn parse_http_url(raw: &str) -> Result<Url, FetchError> {
    let url = Url::parse(raw).map_err(|source| FetchError::InvalidUrl {
        url: raw.to_string(),
        source,
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            source: url::ParseError::RelativeUrlWithoutBase,
        }),
    }
}

fn header_to_string(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn retry_after_seconds(headers: &HeaderMap) -> Option<i64> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(seconds) = value.trim().parse::<i64>() {
        return Some(seconds.max(0));
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    let now = std::time::SystemTime::now();
    retry_at
        .duration_since(now)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn duration_from_seconds(seconds: f32) -> Option<Duration> {
    if seconds.is_finite() && seconds > 0.0 {
        Some(Duration::from_secs_f32(seconds))
    } else {
        None
    }
}
