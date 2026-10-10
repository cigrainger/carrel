use carrel_feeds::parse_feed;

#[test]
fn escaped_code_remains_text_through_feed_parsing_and_sanitization() {
    let html = "<p>Élodie &amp; friends</p><pre><code>&lt;script type=\"module\"&gt;alert('example');&lt;/script&gt;\nif (a &lt; b &amp;&amp; b &gt; 0) {}</code></pre><script>unsafe()</script>";
    let atom = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Example</title><id>feed</id><updated>2026-01-01T00:00:00Z</updated><entry><id>one</id><title>Code</title><updated>2026-01-01T00:00:00Z</updated><content type="html">{}</content></entry></feed>"#,
        html_escape::encode_text(html)
    );
    let rss = format!(
        r#"<rss version="2.0"><channel><title>Example</title><link>https://example.com</link><description>Feed</description><item><guid>one</guid><title>Code</title><description><![CDATA[{html}]]></description></item></channel></rss>"#
    );
    let json = serde_json::json!({"version":"https://jsonfeed.org/version/1.1", "title":"Example", "items":[{"id":"one", "content_html":html}]}).to_string();
    for feed in [atom, rss, json] {
        let parsed = parse_feed(feed.as_bytes(), "https://example.com/feed").unwrap();
        let entry = &parsed.entries[0];
        let body = entry
            .content_html
            .as_deref()
            .or(entry.summary_html.as_deref())
            .unwrap();
        let article =
            carrel_feeds::extract_embedded_html(body, "https://example.com/article").unwrap();
        let text = carrel_feeds::html_to_text(&article.content_html);
        assert!(
            text.contains("<script type=\"module\">alert('example');</script>"),
            "{text}"
        );
        assert!(text.contains("if (a < b && b > 0) {}"), "{text}");
        assert!(text.contains("Élodie & friends"));
        assert!(!article.content_html.contains("<script>"));
        assert!(!text.contains("unsafe()"));
    }
}

#[test]
fn optional_and_blank_entry_author_names_fall_back_to_feed_authors() {
    for authors in [
        r#"[{"url":"https://example.com/author"}]"#,
        r#"[{"name":" \t "}]"#,
        "[]",
    ] {
        let feed = format!(
            r#"{{"version":"https://jsonfeed.org/version/1.1","title":"Example",
                "authors":[{{"name":" Élodie "}}],
                "items":[{{"id":"one","content_text":"Text","authors":{authors}}}]}}"#
        );
        let parsed = parse_feed(feed.as_bytes(), "https://example.com/feed.json").unwrap();
        assert_eq!(parsed.entries[0].authors, vec!["Élodie"]);
    }
}

#[test]
fn named_entry_authors_take_precedence_and_unnamed_authors_are_omitted() {
    let feed = br#"{"version":"https://jsonfeed.org/version/1.1","title":"Example",
        "authors":[{"name":"Feed author"}],
        "items":[{"id":"one","content_text":"Text",
            "authors":[{"url":"https://example.com/author"},{"name":" Ada "},{"name":" "}]}]}"#;
    let parsed = parse_feed(feed, "https://example.com/feed.json").unwrap();
    assert_eq!(parsed.entries[0].authors, vec!["Ada"]);
}

#[test]
fn parses_rss_atom_and_json_feed() {
    let rss = parse_feed(
        include_bytes!("parse_fixtures/rss.xml"),
        "https://example.com/feed.xml",
    )
    .unwrap();
    assert_eq!(rss.title.as_deref(), Some("Example RSS"));
    assert_eq!(rss.entries.len(), 1);
    assert_eq!(rss.entries[0].feed_guid, "rss-1");
    assert_eq!(rss.entries[0].title.as_deref(), Some("One & Two"));
    assert_eq!(rss.entries[0].authors, vec!["Ada"]);
    assert_eq!(
        rss.entries[0].content_html.as_deref(),
        Some("<p>Full RSS</p>")
    );

    let atom = parse_feed(
        include_bytes!("parse_fixtures/atom.xml"),
        "https://example.com/blog/feed.atom",
    )
    .unwrap();
    assert_eq!(atom.title.as_deref(), Some("Example Atom"));
    assert_eq!(
        atom.entries[0].url.as_deref(),
        Some("https://example.com/blog/atom-one")
    );
    assert_eq!(atom.entries[0].language.as_deref(), Some("en"));

    let json_feed = parse_feed(
        include_bytes!("parse_fixtures/feed.json"),
        "https://example.com/feed.json",
    )
    .unwrap();
    assert_eq!(json_feed.title.as_deref(), Some("Example JSON"));
    assert_eq!(json_feed.entries[0].feed_guid, "json-1");
    assert_eq!(
        json_feed.entries[0].summary_html.as_deref(),
        Some("Short JSON summary")
    );
}
#[test]
fn empty_publisher_titles_remain_untitled() {
    let parsed = carrel_feeds::parse_feed(
        br#"<rss version="2.0"><channel><title>Example</title>
          <item><guid>one</guid><title></title><description>A short post.</description></item>
          <item><guid>two</guid><title>   </title></item>
        </channel></rss>"#,
        "https://example.com/feed.xml",
    )
    .unwrap();
    assert_eq!(parsed.entries.len(), 2);
    assert!(parsed.entries.iter().all(|entry| entry.title.is_none()));
}
