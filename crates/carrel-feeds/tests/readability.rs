use std::ffi::OsString;

use carrel_feeds::{
    DEFAULT_USER_AGENT, ExtractOptions, ExtractorUsed, Fetcher, FetcherConfig, TrafilaturaConfig,
    extract_embedded_html, extract_from_html, extract_from_html_with_options, rewrite_images,
    sanitize_html,
};
use proptest::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn extracts_fixture_article() {
    let html = include_str!("readability_fixtures/simple_article.html");
    let article = extract_from_html(html, "https://example.com/posts/readable").unwrap();

    assert_eq!(article.title.as_deref(), Some("Readable Fixture"));
    assert_eq!(article.byline.as_deref(), Some("Ada Lovelace"));
    assert!(article.content_html.contains("first paragraph"));
    assert!(!article.content_html.contains("Subscribe now"));
    assert!(article.word_count >= 10);
    assert_eq!(article.extractor, ExtractorUsed::ReadableReadability);
}

#[test]
fn network_error_shell_is_not_readable_content() {
    let html = r#"
        <main>
          <h1>We can't find the internet</h1>
          <p>Attempting to reconnect</p>
        </main>
    "#;
    let error = extract_from_html(html, "https://example.com/offline").unwrap_err();

    assert!(error.to_string().contains("network error page"));
}

#[test]
fn embedded_feed_content_skips_readability_scoring() {
    let article = extract_embedded_html(
        "<p>Full feed content with <strong>formatting</strong>.</p>",
        "https://example.com/post",
    )
    .unwrap();

    assert_eq!(article.extractor, ExtractorUsed::FeedEmbedded);
    assert!(article.content_html.contains("<strong>formatting</strong>"));
}

#[test]
fn hidden_offline_banners_do_not_reject_or_pollute_readable_articles() {
    let html = r#"
        <html><body>
          <main><article>
            <h1>A readable post</h1>
            <p>This is a real article delivered successfully by the publisher.
               Its readable paragraphs must remain available for offline reading.</p>
          </article></main>
          <div role="alert" hidden>
            <p>We can&#39;t find the internet</p>
            <div hidden><p>Attempting to reconnect</p></div>
          </div>
          <div hidden><p>Hidden server failure</p></div>
        </body></html>
    "#;

    for article in [
        extract_from_html(html, "https://example.com/post").unwrap(),
        extract_embedded_html(html, "https://example.com/post").unwrap(),
    ] {
        assert!(
            article
                .content_html
                .contains("real article delivered successfully")
        );
        assert!(!article.content_html.contains("find the internet"));
        assert!(!article.content_html.contains("Attempting to reconnect"));
        assert!(!article.content_html.contains("Hidden server failure"));
        assert!(article.word_count > 0);
    }
}

#[test]
fn visible_network_error_content_is_still_rejected_from_feeds() {
    let html = "<p>We can’t find the internet</p><p>Attempting to reconnect</p>";
    assert!(extract_embedded_html(html, "https://example.com/post").is_err());
}

#[test]
fn empty_or_entirely_hidden_pages_fail_without_panicking() {
    for html in [
        "<html><body></body></html>",
        "<div hidden>We can't find the internet. Attempting to reconnect.</div>",
    ] {
        for result in [
            extract_from_html(html, "https://example.com/post"),
            extract_embedded_html(html, "https://example.com/post"),
        ] {
            assert!(matches!(
                result,
                Err(carrel_feeds::ExtractError::EmptyContent { .. })
            ));
        }
    }
}

#[test]
fn sanitizer_strips_known_xss_patterns() {
    let cleaned = sanitize_html(
        r#"
        <p onclick="evil()">Hello</p>
        <script>alert(1)</script>
        <img src="javascript:alert(1)" onerror="evil()" style="width: 1px">
        <a href="javascript:alert(1)">bad</a>
        <iframe src="https://evil.example/embed"></iframe>
        <iframe src="https://www.youtube.com/embed/abc"></iframe>
        "#,
    );

    assert!(cleaned.contains("<p>Hello</p>"));
    assert!(!cleaned.contains("script"));
    assert!(!cleaned.contains("javascript:"));
    assert!(!cleaned.contains("onerror"));
    assert!(!cleaned.contains("style="));
    assert!(!cleaned.contains("evil.example"));
    assert!(cleaned.contains("youtube.com/embed/abc"));
}

#[test]
fn sanitizer_drops_byte_order_marks() {
    assert_eq!(sanitize_html("\t\u{feff}"), "");
}

#[test]
fn sanitizer_renders_tex_math_to_mathml() {
    let cleaned = sanitize_html(r#"<p>Energy <span class="math inline">E = mc^2</span></p>"#);

    assert!(cleaned.contains("<math"));
    assert!(cleaned.contains("<msup>"));
    assert!(!cleaned.contains("class=\"math inline\""));
    assert_eq!(sanitize_html(&cleaned), cleaned);
}

proptest! {
    #[test]
    fn sanitization_is_idempotent(input in ".*") {
        let once = sanitize_html(&input);
        let twice = sanitize_html(&once);
        prop_assert_eq!(once, twice);
    }
}

#[tokio::test]
async fn image_rewrite_uses_blob_urls_and_keeps_failures_best_effort() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3, 4]))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/missing.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let fetcher = Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            min_interval_per_host: std::time::Duration::ZERO,
            ..FetcherConfig::default()
        },
    )
    .unwrap();
    let html = r#"
        <picture>
          <source srcset="/image.png 1x, /missing.png 2x">
          <img src="/image.png" alt="diagram">
        </picture>
    "#;

    let result = rewrite_images(
        html,
        &format!("{}/article", server.uri()),
        &fetcher,
        |bytes| Ok::<_, String>(blake3::hash(bytes).to_hex().to_string()),
    )
    .await;

    let expected_blob = format!("blob://{}", blake3::hash(&[1, 2, 3, 4]).to_hex());
    assert!(result.html.contains(&format!("src=\"{expected_blob}\"")));
    assert!(result.html.contains(&format!("{expected_blob} 1x")));
    assert!(result.html.contains("/missing.png 2x"));
    assert_eq!(result.failures.len(), 1);
}

#[tokio::test]
async fn image_rewrite_adds_dimensions_when_the_blob_has_them() {
    let server = MockServer::start().await;
    let mut png = vec![0_u8; 24];
    png[0..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    png[12..16].copy_from_slice(b"IHDR");
    png[16..20].copy_from_slice(&16_u32.to_be_bytes());
    png[20..24].copy_from_slice(&9_u32.to_be_bytes());

    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sized.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png.clone()))
        .mount(&server)
        .await;

    let fetcher = Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            min_interval_per_host: std::time::Duration::ZERO,
            ..FetcherConfig::default()
        },
    )
    .unwrap();
    let result = rewrite_images(
        r#"<img src="/sized.png" alt="diagram">"#,
        &format!("{}/article", server.uri()),
        &fetcher,
        |bytes| Ok::<_, String>(blake3::hash(bytes).to_hex().to_string()),
    )
    .await;

    assert!(result.html.contains("width=\"16\""));
    assert!(result.html.contains("height=\"9\""));
}

#[test]
fn configured_fallback_can_extract_when_primary_is_empty() {
    let options = ExtractOptions {
        trafilatura: Some(TrafilaturaConfig::new(
            "sh",
            [
                OsString::from("-c"),
                OsString::from(
                    "cat >/dev/null; printf '<article><p>fallback body text</p></article>'",
                ),
            ],
        )),
    };

    let article = extract_from_html_with_options(
        "<html><body></body></html>",
        "https://example.com",
        &options,
    )
    .unwrap();
    assert_eq!(article.extractor, ExtractorUsed::Trafilatura);
    assert!(article.content_html.contains("fallback body text"));
}

#[tokio::test]
async fn image_rewrite_caches_complete_cdn_srcset_urls_once_each() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    let mut candidates = Vec::new();
    let mut expected = Vec::new();
    for width in [400_u32, 800, 1200] {
        let image_path = format!("/cdn-cgi/image/format=auto,quality=85,width={width},/photo.jpg");
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[16..20].copy_from_slice(&width.to_be_bytes());
        png[20..24].copy_from_slice(&600_u32.to_be_bytes());
        expected.push(format!("blob://{} {width}w", blake3::hash(&png).to_hex()));
        candidates.push(format!("{}{image_path} {width}w", server.uri()));
        Mock::given(method("GET"))
            .and(path(&image_path))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png))
            .expect(1)
            .mount(&server)
            .await;
    }
    let srcset = candidates.join(", ");
    let src = candidates[2].split_whitespace().next().unwrap();
    // Img and picture source share candidates; each URL must be fetched once.
    let html = format!(
        r#"<picture><source srcset="{srcset}"><img src="{src}" srcset="{srcset}" alt="photograph"></picture>"#
    );
    let fetcher = Fetcher::with_config(
        DEFAULT_USER_AGENT,
        FetcherConfig {
            min_interval_per_host: std::time::Duration::ZERO,
            ..FetcherConfig::default()
        },
    )
    .unwrap();
    let mut stored = 0;
    let result = rewrite_images(
        &html,
        &format!("{}/article", server.uri()),
        &fetcher,
        |bytes| {
            stored += 1;
            Ok::<_, String>(blake3::hash(bytes).to_hex().to_string())
        },
    )
    .await;
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(stored, 3);
    assert_eq!(
        result
            .html
            .matches(&format!("srcset=\"{}\"", expected.join(", ")))
            .count(),
        2
    );
    assert!(!result.html.contains("cdn-cgi"));
    server.verify().await;
}
