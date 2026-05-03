use carrel_feeds::parse_feed;

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
