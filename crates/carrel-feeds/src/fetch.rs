//! Polite HTTP fetching for subscribed feeds.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{
    ETAG, HeaderMap, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, RETRY_AFTER,
};
use reqwest::{Client, Response, StatusCode};
use texting_robots::{Robot, get_robots_url};
use tokio::sync::Mutex;
use url::Url;

use crate::error::FetchError;
use crate::politeness::{
    DEFAULT_MAX_CONCURRENT_PER_HOST, DEFAULT_MIN_INTERVAL_PER_HOST, HostLimiter, host_key,
};

/// Default user agent sent by Carrel's feed fetcher.
pub const DEFAULT_USER_AGENT: &str = "Carrel/0.1 (+https://github.com/cigrainger/carrel)";

// Bound decoded bytes, not just Content-Length: compressed and chunked responses
// can otherwise grow without limit before parsing starts. RFC 9309 requires
// robots parsers to support at least 500 KiB; keep a separate 512 KiB budget.
const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ROBOTS_BYTES: usize = 512 * 1024;

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
    document_client: Client,
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
        // Robots lookup may follow redirects; documents need a policy check at every hop.
        let document_client = Client::builder()
            .user_agent(user_agent)
            .timeout(config.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(FetchError::ClientBuild)?;

        Ok(Self {
            client,
            document_client,
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
        self.fetch_document(url, etag, last_modified)
            .await
            .map(|(_, result)| result)
    }

    /// Fetch a document and retain its final URL for resolving advertised links.
    pub async fn fetch_document(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<(String, FetchResult), FetchError> {
        let original = parse_http_url(url)?;
        let mut url = original.clone();
        let mut redirects = 0;
        let (response, _permit) = loop {
            self.ensure_robots_allowed(&url).await?;
            let permit = self.limiter.acquire(&url).await?;
            let mut request = self.document_client.get(url.clone());
            if url.origin() == original.origin() {
                if let Some(etag) = etag.filter(|value| !value.is_empty()) {
                    request = request.header(IF_NONE_MATCH, etag);
                }
                if let Some(last_modified) = last_modified.filter(|value| !value.is_empty()) {
                    request = request.header(IF_MODIFIED_SINCE, last_modified);
                }
            }
            let response = request.send().await.map_err(|source| FetchError::Request {
                url: url.to_string(),
                source,
            })?;
            if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
            {
                if redirects == 10 {
                    return Err(FetchError::RedirectLimit {
                        url: url.to_string(),
                    });
                }
                let next = url
                    .join(location)
                    .map_err(|source| FetchError::InvalidUrl {
                        url: url.to_string(),
                        source,
                    })?;
                url = parse_http_url(next.as_str())?;
                redirects += 1;
                drop(permit);
                continue;
            }
            break (response, permit);
        };
        let status = response.status();
        let final_url = response.url().to_string();
        let headers = HttpHeaders::from_headers(response.headers());

        if status == StatusCode::NOT_MODIFIED {
            return Ok((final_url, FetchResult::NotModified { headers }));
        }

        if status.is_success() {
            let body = read_bounded_body(response, url.as_str(), MAX_DOCUMENT_BYTES).await?;
            return Ok((final_url, FetchResult::Updated { body, headers }));
        }

        Ok((
            final_url,
            FetchResult::GoneOrError {
                status: status.as_u16(),
                headers,
            },
        ))
    }

    async fn ensure_robots_allowed(&self, url: &Url) -> Result<(), FetchError> {
        let host = host_key(url);
        if let Some(policy) = self.cached_policy(&host).await {
            return self.apply_policy(url, &policy).await;
        }

        let policy = self.fetch_robots_policy(url).await?;
        // A zero TTL deliberately disables reuse. Apply this fetched policy
        // directly even if its cache entry expires before the next await.
        self.cache_policy(host, policy.clone()).await;
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

        let body = read_bounded_body(response, robots_url.as_str(), MAX_ROBOTS_BYTES).await?;
        let robot =
            Robot::new(&self.user_agent, &body).map_err(|source| FetchError::RobotsParse {
                url: url.to_string(),
                message: source.to_string(),
            })?;

        if let Some(delay) = crawl_delay(robot.delay, url.as_str())? {
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
                if let Some(delay) = crawl_delay(robot.delay, url.as_str())? {
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

async fn read_bounded_body(
    mut response: Response,
    url: &str,
    limit: usize,
) -> Result<Bytes, FetchError> {
    let too_large = || FetchError::BodyTooLarge {
        url: url.to_string(),
        limit,
    };
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|source| FetchError::Body {
        url: url.to_string(),
        source,
    })? {
        if chunk.len() > limit - body.len() {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.into())
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
    let mut url = Url::parse(raw).map_err(|source| FetchError::InvalidUrl {
        url: raw.to_string(),
        source,
    })?;
    if matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
    {
        url.set_fragment(None);
        Ok(url)
    } else {
        Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            source: url::ParseError::RelativeUrlWithoutBase,
        })
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

fn crawl_delay(seconds: Option<f32>, url: &str) -> Result<Option<Duration>, FetchError> {
    let Some(seconds) = seconds else {
        return Ok(None);
    };
    if seconds <= 0.0 {
        return Ok(None);
    }
    // A parsed float can be finite yet too large for Duration or the platform
    // clock. Fail closed rather than panic or ignore the publisher's delay.
    Duration::try_from_secs_f32(seconds)
        .ok()
        .filter(|delay| Instant::now().checked_add(*delay).is_some())
        .map(Some)
        .ok_or_else(|| FetchError::RobotsParse {
            url: url.to_string(),
            message: "crawl delay cannot be scheduled safely".into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn untrusted_crawl_delays_are_checked_before_scheduling() {
        for seconds in [f32::MAX, f32::INFINITY, f32::NAN] {
            assert!(matches!(
                crawl_delay(Some(seconds), "https://fixture.invalid/"),
                Err(FetchError::RobotsParse { .. })
            ));
        }
        assert_eq!(
            crawl_delay(Some(1.5), "fixture").unwrap(),
            Some(Duration::from_millis(1500))
        );
        for seconds in [None, Some(0.0), Some(-1.0)] {
            assert_eq!(crawl_delay(seconds, "fixture").unwrap(), None);
        }
    }

    // A raw chunked response exercises the path where no Content-Length is
    // available. The small test budget keeps boundary tests fast and readable.
    async fn chunked_response(chunks: &[&[u8]]) -> Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut wire =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        for chunk in chunks {
            wire.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            wire.extend_from_slice(chunk);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"0\r\n\r\n");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 512];
            while !request.ends_with(b"\r\n\r\n") {
                let length = stream.read(&mut buffer).await.unwrap();
                assert_ne!(length, 0, "client closed before sending request headers");
                request.extend_from_slice(&buffer[..length]);
                assert!(request.len() <= 8192, "unexpectedly large test request");
            }
            stream.write_all(&wire).await.unwrap();
        });
        Client::new()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn decoded_body_budget_handles_empty_exact_and_streamed_overflow() {
        for (chunks, expected) in [
            (vec![], Some(&b""[..])),
            (vec![&b"ab"[..], &b"cd"[..]], Some(&b"abcd"[..])),
            (vec![&b"abc"[..], &b"de"[..]], None),
        ] {
            let response = chunked_response(&chunks).await;
            assert_eq!(response.content_length(), None);
            let result = read_bounded_body(response, "https://fixture.invalid/", 4).await;
            match expected {
                Some(bytes) => assert_eq!(result.unwrap().as_ref(), bytes),
                None => assert!(matches!(
                    result,
                    Err(FetchError::BodyTooLarge { limit: 4, .. })
                )),
            }
        }
    }
}
