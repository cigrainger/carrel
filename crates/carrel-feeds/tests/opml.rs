use carrel_core::subscription::FeedSubscription;
use carrel_feeds::opml::{MAX_OPML_BYTES, export_opml, parse_opml};
use proptest::prelude::*;

#[test]
fn imports_rss_atom_and_json_feed_urls_with_nested_folders_and_unicode() {
    let input = r#"<?xml version="1.0" encoding="UTF-8"?><opml version="2.0"><head><title>読む</title></head><body>
        <outline text="Reading &amp; writing"><outline text="Essays">
            <outline type="rss" text="A &lt;quiet&gt; web" xmlUrl="https://example.com/atom?x=1&amp;y=2"/>
            <outline title="文章" xmlUrl="https://example.com/feed.json"></outline>
        </outline></outline><outline xmlUrl="http://other.example/rss"/>
    </body></opml>"#;
    let rows = parse_opml(input.as_bytes()).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].title.as_deref(), Some("A <quiet> web"));
    assert_eq!(rows[0].url, "https://example.com/atom?x=1&y=2");
    assert_eq!(
        rows[0].folder.as_deref(),
        Some("Reading & writing / Essays")
    );
    assert_eq!(rows[1].title.as_deref(), Some("文章"));
    assert_eq!(rows[2].folder, None);
    assert_eq!(rows[2].title, None);
}

#[test]
fn round_trips_portable_metadata_without_http_or_reading_state() {
    let mut original = vec![
        FeedSubscription {
            url: "https://example.com/feed?x=1&y=2".into(),
            title: Some("Quotes \" & < > 日本語".into()),
            folder: Some("Reading / Essays".into()),
        },
        FeedSubscription {
            url: "http://empty.example/rss".into(),
            title: None,
            folder: None,
        },
    ];
    let bytes = export_opml(&original).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("&amp;"));
    assert!(!text.contains("read_state"));
    assert!(!text.contains("etag"));
    let mut imported = parse_opml(&bytes).unwrap();
    original.sort_by(|a, b| a.url.cmp(&b.url));
    imported.sort_by(|a, b| a.url.cmp(&b.url));
    assert_eq!(imported, original);
    assert!(parse_opml(&export_opml(&[]).unwrap()).unwrap().is_empty());
}

#[test]
fn rejects_malformed_xml_dtd_entities_invalid_urls_and_unsupported_encodings() {
    for input in [
        "",
        "<html><body/></html>",
        "<opml><head/></opml>",
        "<opml><body><outline/></opml>",
        "<opml><body/></opml><opml><body/></opml>",
        "<opml><body/><body/></opml>",
        "<!DOCTYPE opml [<!ENTITY secret SYSTEM 'file:///private/data'>]><opml><body/></opml>",
        "<opml><body><outline text='&unknown;' xmlUrl='https://example.com/feed'/></body></opml>",
        "<opml><head><title>&unknown;</title></head><body/></opml>",
        "<opml><body><outline text='&#0;' xmlUrl='https://example.com/feed'/></body></opml>",
        "<opml><body><outline xmlUrl='relative'/></body></opml>",
        "<opml><body><outline xmlUrl='file:///private/data'/></body></opml>",
        "<opml><body><outline xmlUrl='https://user:password@example.com/feed'/></body></opml>",
        "<opml><body><outline xmlUrl='https://example.com' xmlUrl='https://other.example'/></body></opml>",
        "<?xml version='1.0' encoding='ISO-8859-1'?><opml><body/></opml>",
        "<opml version='3.0'><body/></opml>",
        "<opml><outline xmlUrl='https://example.com/feed'/></opml>",
    ] {
        assert!(parse_opml(input.as_bytes()).is_err(), "accepted {input}");
    }
    assert!(parse_opml(&[0xff, 0xfe]).is_err());
    assert!(parse_opml(&vec![b' '; MAX_OPML_BYTES + 1]).is_err());
    let nested = format!(
        "<opml><body>{}{}</body></opml>",
        "<outline text='x'>".repeat(65),
        "</outline>".repeat(65)
    );
    assert!(parse_opml(nested.as_bytes()).is_err());
    let many = format!(
        "<opml><body>{}</body></opml>",
        "<outline xmlUrl='https://example.com/feed'/>".repeat(10_001)
    );
    assert!(parse_opml(many.as_bytes()).is_err());
}

#[test]
fn invalid_export_metadata_never_produces_a_partial_document() {
    for (url, title) in [
        ("file:///private/data", "Valid"),
        ("https://example.com/feed", "Invalid\u{0}"),
    ] {
        assert!(
            export_opml(&[FeedSubscription {
                url: url.into(),
                title: Some(title.into()),
                folder: None
            }])
            .is_err()
        );
    }
}

proptest! {
    #[test]
    fn xml_escaping_preserves_arbitrary_printable_titles(title in "[\\p{L}\\p{N}&<>\"' ]{1,80}") {
        let title = title.trim().to_string();
        prop_assume!(!title.is_empty());
        let subscription = FeedSubscription { url: "https://example.com/feed?x=1&y=2".into(), title: Some(title), folder: Some("Essays & Ideas".into()) };
        let bytes = export_opml(std::slice::from_ref(&subscription)).unwrap();
        prop_assert_eq!(parse_opml(&bytes).unwrap(), vec![subscription]);
    }
}
