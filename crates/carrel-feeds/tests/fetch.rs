use std::time::{Duration, Instant};

use carrel_feeds::{
    DEFAULT_USER_AGENT, FetchError, FetchResult, Fetcher, FetcherConfig, HttpHeaders,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetch_sends_conditionals_and_handles_not_modified() {
    let server = MockServer::start().await;
    allow_robots(&server).await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(304).insert_header("etag", "\"abc\""))
        .mount(&server)
        .await;

    let fetcher = test_fetcher();
    let result = fetcher
        .fetch(
            &format!("{}/feed.xml", server.uri()),
            Some("\"abc\""),
            Some("Sun, 03 May 2026 00:00:00 GMT"),
        )
        .await
        .unwrap();

    assert_eq!(
        result,
        FetchResult::NotModified {
            headers: HttpHeaders {
                etag: Some("\"abc\"".to_string()),
                last_modified: None,
                retry_after_seconds: None,
            }
        }
    );

    let requests = server.received_requests().await.unwrap();
    let feed_request = requests
        .iter()
        .find(|request| request.url.path() == "/feed.xml")
        .unwrap();
    assert_eq!(
        feed_request.headers["user-agent"].to_str().unwrap(),
        DEFAULT_USER_AGENT
    );
    assert_eq!(
        feed_request.headers["if-none-match"].to_str().unwrap(),
        "\"abc\""
    );
    assert_eq!(
        feed_request.headers["if-modified-since"].to_str().unwrap(),
        "Sun, 03 May 2026 00:00:00 GMT"
    );
}

#[tokio::test]
async fn fetch_respects_robots_disallow() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /private\n"),
        )
        .mount(&server)
        .await;

    let fetcher = test_fetcher();
    let error = fetcher
        .fetch(&format!("{}/private/feed.xml", server.uri()), None, None)
        .await
        .unwrap_err();

    assert!(matches!(error, FetchError::RobotsDisallowed { .. }));
}

#[tokio::test]
async fn fetch_captures_retry_after_on_429() {
    let server = MockServer::start().await;
    allow_robots(&server).await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
        .mount(&server)
        .await;

    let fetcher = test_fetcher();
    let result = fetcher
        .fetch(&format!("{}/feed.xml", server.uri()), None, None)
        .await
        .unwrap();

    assert_eq!(
        result,
        FetchResult::GoneOrError {
            status: 429,
            headers: HttpHeaders {
                etag: None,
                last_modified: None,
                retry_after_seconds: Some(120),
            }
        }
    );
}

#[tokio::test]
async fn fetch_reports_client_errors_without_parsing() {
    let server = MockServer::start().await;
    allow_robots(&server).await;
    Mock::given(method("GET"))
        .and(path("/missing.xml"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let fetcher = test_fetcher();
    let result = fetcher
        .fetch(&format!("{}/missing.xml", server.uri()), None, None)
        .await
        .unwrap();

    assert_eq!(
        result,
        FetchResult::GoneOrError {
            status: 404,
            headers: HttpHeaders::default(),
        }
    );
}

#[tokio::test]
async fn per_host_interval_delays_back_to_back_requests() {
    let server = MockServer::start().await;
    allow_robots(&server).await;
    Mock::given(method("GET"))
        .and(path("/a.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<rss version=\"2.0\"><channel><title>A</title></channel></rss>"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/b.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<rss version=\"2.0\"><channel><title>B</title></channel></rss>"),
        )
        .mount(&server)
        .await;

    let fetcher = Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            max_concurrent_per_host: 1,
            min_interval_per_host: Duration::from_millis(50),
            robots_cache_ttl: Duration::from_secs(60),
            request_timeout: Duration::from_secs(5),
        },
    )
    .unwrap();
    let start = Instant::now();
    let first_url = format!("{}/a.xml", server.uri());
    let second_url = format!("{}/b.xml", server.uri());
    let first = fetcher.fetch(&first_url, None, None);
    let second = fetcher.fetch(&second_url, None, None);
    let _ = tokio::join!(first, second);

    assert!(start.elapsed() >= Duration::from_millis(45));
}

async fn allow_robots(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(server)
        .await;
}

fn test_fetcher() -> Fetcher {
    Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            max_concurrent_per_host: 2,
            min_interval_per_host: Duration::from_millis(0),
            robots_cache_ttl: Duration::from_secs(60),
            request_timeout: Duration::from_secs(5),
        },
    )
    .unwrap()
}
