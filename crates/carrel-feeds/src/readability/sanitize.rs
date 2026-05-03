//! HTML sanitization for readable article content.

use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use kuchiki::traits::TendrilSink;

/// Sanitize readable article HTML to Carrel's storage-safe subset.
pub fn sanitize_html(html: &str) -> String {
    let mut builder = Builder::new();
    builder
        .tags(allowed_tags())
        .clean_content_tags(clean_content_tags())
        .tag_attributes(tag_attributes())
        .generic_attributes(HashSet::from(["id", "title", "lang"]))
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None);

    let cleaned = builder.clean(html).to_string();
    strip_unsafe_iframes(&cleaned)
}

fn allowed_tags() -> HashSet<&'static str> {
    HashSet::from([
        "a",
        "abbr",
        "audio",
        "b",
        "bdi",
        "bdo",
        "blockquote",
        "br",
        "caption",
        "cite",
        "code",
        "col",
        "colgroup",
        "dd",
        "del",
        "details",
        "dfn",
        "dl",
        "dt",
        "em",
        "figcaption",
        "figure",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "iframe",
        "img",
        "ins",
        "kbd",
        "li",
        "mark",
        "ol",
        "p",
        "picture",
        "pre",
        "q",
        "rp",
        "rt",
        "ruby",
        "s",
        "samp",
        "small",
        "source",
        "strong",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "time",
        "tr",
        "u",
        "ul",
        "var",
        "video",
        "wbr",
    ])
}

fn clean_content_tags() -> HashSet<&'static str> {
    HashSet::from([
        "button", "embed", "form", "input", "link", "meta", "object", "option", "script", "select",
        "style",
    ])
}

fn tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    HashMap::from([
        ("a", HashSet::from(["href"])),
        ("audio", HashSet::from(["controls", "src"])),
        ("blockquote", HashSet::from(["cite"])),
        ("col", HashSet::from(["span"])),
        ("iframe", HashSet::from(["src", "title"])),
        ("img", HashSet::from(["alt", "src", "srcset"])),
        ("ol", HashSet::from(["start"])),
        ("q", HashSet::from(["cite"])),
        ("source", HashSet::from(["src", "srcset", "type"])),
        ("td", HashSet::from(["colspan", "headers", "rowspan"])),
        (
            "th",
            HashSet::from(["colspan", "headers", "rowspan", "scope"]),
        ),
        ("time", HashSet::from(["datetime"])),
        ("video", HashSet::from(["controls", "poster", "src"])),
    ])
}

fn strip_unsafe_iframes(html: &str) -> String {
    let document = kuchiki::parse_html().one(html);
    if let Ok(iframes) = document.select("iframe") {
        for iframe in iframes {
            let keep = iframe
                .attributes
                .borrow()
                .get("src")
                .is_some_and(is_allowed_iframe_src);
            if !keep {
                iframe.as_node().detach();
            }
        }
    }

    serialize_body_children(&document)
}

fn is_allowed_iframe_src(src: &str) -> bool {
    let Ok(url) = url::Url::parse(src) else {
        return false;
    };

    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    matches!(
        host.strip_prefix("www.").unwrap_or(host),
        "youtube.com" | "youtube-nocookie.com" | "player.vimeo.com" | "vimeo.com"
    )
}

fn serialize_body_children(document: &kuchiki::NodeRef) -> String {
    let Ok(body) = document.select_first("body") else {
        return document.to_string();
    };

    body.as_node()
        .children()
        .map(|child| child.to_string())
        .collect()
}
