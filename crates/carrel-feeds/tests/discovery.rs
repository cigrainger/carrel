use std::time::Duration;

use carrel_feeds::discovery::{DiscoveryError, MAX_DISCOVERY_BYTES, discover_feeds};
use carrel_feeds::{DEFAULT_USER_AGENT, FetchError, Fetcher, FetcherConfig};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fetcher() -> Fetcher {
    Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            min_interval_per_host: Duration::ZERO,
            ..FetcherConfig::default()
        },
    )
    .unwrap()
}

async fn response(server: &MockServer, route: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    response(
        &server,
        "/robots.txt",
        ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"),
    )
    .await;
    server
}

#[tokio::test]
async fn accepts_direct_rss_atom_and_json_feeds() {
    for body in [
        include_str!("parse_fixtures/rss.xml"),
        include_str!("parse_fixtures/atom.xml"),
        include_str!("parse_fixtures/feed.json"),
    ] {
        let server = server().await;
        response(
            &server,
            "/feed",
            ResponseTemplate::new(200).set_body_string(body),
        )
        .await;
        let url = format!("{}/feed", server.uri());
        let feeds = discover_feeds(&fetcher(), &url).await.unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].url, url);
        assert!(!feeds[0].title.is_empty());
        let requests = server.received_requests().await.unwrap();
        let request = requests.iter().find(|r| r.url.path() == "/feed").unwrap();
        assert_eq!(
            request.headers["user-agent"].to_str().unwrap(),
            DEFAULT_USER_AGENT
        );
        assert!(!request.headers.contains_key("if-none-match"));
    }
}

#[tokio::test]
async fn validates_multiple_advertisements_and_skips_broken_and_non_feed_links() {
    let server = server().await;
    response(
        &server,
        "/blog",
        ResponseTemplate::new(200).set_body_string(
            r#"<head>
        <link rel="alternate" type="application/rss+xml" href="/missing">
        <link rel="alternate" type="application/atom+xml" href="/posts" title="Misleading title">
        <link rel="alternate" type="application/rss+xml" href="/html">
        <link rel="alternate" type="application/rss+xml" href="/large">
        <link rel="alternate" type="application/feed+json" href="/comments" title="Comments">
        </head>"#,
        ),
    )
    .await;
    response(
        &server,
        "/posts",
        ResponseTemplate::new(200).set_body_string(include_str!("parse_fixtures/atom.xml")),
    )
    .await;
    response(
        &server,
        "/comments",
        ResponseTemplate::new(200).set_body_string(include_str!("parse_fixtures/feed.json")),
    )
    .await;
    response(
        &server,
        "/html",
        ResponseTemplate::new(200).set_body_string("<html>not a feed</html>"),
    )
    .await;
    response(
        &server,
        "/large",
        ResponseTemplate::new(200).set_body_string("x".repeat(MAX_DISCOVERY_BYTES + 1)),
    )
    .await;
    let feeds = discover_feeds(&fetcher(), &format!("{}/blog", server.uri()))
        .await
        .unwrap();
    assert_eq!(
        feeds.iter().map(|f| f.url.as_str()).collect::<Vec<_>>(),
        [
            format!("{}/posts", server.uri()),
            format!("{}/comments", server.uri())
        ]
    );
    assert_ne!(feeds[0].title, "Misleading title");
}

#[tokio::test]
async fn follows_page_redirects_resolves_relative_links_and_deduplicates_feed_redirects() {
    let server = server().await;
    response(
        &server,
        "/start",
        ResponseTemplate::new(302).insert_header("location", "/blog/index.html"),
    )
    .await;
    response(
        &server,
        "/blog/index.html",
        ResponseTemplate::new(200).set_body_string(
            r#"<head>
        <link rel="alternate" type="application/atom+xml" href="feed.xml">
        <link rel="alternate" type="application/atom+xml" href="/alias">
        </head>"#,
        ),
    )
    .await;
    response(
        &server,
        "/alias",
        ResponseTemplate::new(302).insert_header("location", "/blog/feed.xml"),
    )
    .await;
    response(
        &server,
        "/blog/feed.xml",
        ResponseTemplate::new(200).set_body_string(include_str!("parse_fixtures/atom.xml")),
    )
    .await;
    let feeds = discover_feeds(&fetcher(), &format!("{}/start", server.uri()))
        .await
        .unwrap();
    assert_eq!(feeds.len(), 1);
    assert_eq!(feeds[0].url, format!("{}/blog/feed.xml", server.uri()));
}

#[tokio::test]
async fn reports_no_feed_http_not_modified_and_oversized_documents() {
    let server = server().await;
    response(
        &server,
        "/html",
        ResponseTemplate::new(200).set_body_string("<html>No advertisements</html>"),
    )
    .await;
    response(&server, "/unchanged", ResponseTemplate::new(304)).await;
    response(
        &server,
        "/large",
        ResponseTemplate::new(200).set_body_string("x".repeat(MAX_DISCOVERY_BYTES + 1)),
    )
    .await;
    let fetcher = fetcher();
    let root = server.uri();
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{root}/html")).await,
        Err(DiscoveryError::NoFeeds(_))
    ));
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{root}/missing")).await,
        Err(DiscoveryError::HttpStatus(404))
    ));
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{root}/unchanged")).await,
        Err(DiscoveryError::NotModified)
    ));
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{root}/large")).await,
        Err(DiscoveryError::TooLarge)
    ));
}

#[tokio::test]
async fn discovery_respects_robots_and_never_requests_disallowed_candidates() {
    let server = MockServer::start().await;
    response(
        &server,
        "/robots.txt",
        ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /private\n"),
    )
    .await;
    response(
        &server,
        "/blog",
        ResponseTemplate::new(200).set_body_string(
            r#"<link rel="alternate" type="application/rss+xml" href="/private/feed">"#,
        ),
    )
    .await;
    let fetcher = fetcher();
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{}/private", server.uri())).await,
        Err(DiscoveryError::Fetch(FetchError::RobotsDisallowed { .. }))
    ));
    assert!(matches!(
        discover_feeds(&fetcher, &format!("{}/blog", server.uri())).await,
        Err(DiscoveryError::NoFeeds(_))
    ));
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|r| r.url.path().starts_with("/private"))
    );
}
