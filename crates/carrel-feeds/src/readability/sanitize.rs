//! HTML sanitization for readable article content.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use kuchiki::traits::TendrilSink;

/// Sanitize readable article HTML to Carrel's storage-safe subset.
pub fn sanitize_html(html: &str) -> String {
    let html = strip_byte_order_marks(html);
    let mut builder = Builder::new();
    builder
        .tags(allowed_tags())
        .clean_content_tags(clean_content_tags())
        .tag_attributes(tag_attributes())
        .generic_attributes(HashSet::from(["class", "id", "title", "lang"]))
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None);

    let cleaned = builder.clean(html.as_ref()).to_string();
    let cleaned = strip_unsafe_iframes(&cleaned);
    let cleaned = render_math_spans(&cleaned);
    strip_byte_order_marks(&cleaned).into_owned()
}

fn strip_byte_order_marks(input: &str) -> Cow<'_, str> {
    if input.contains('\u{feff}') {
        Cow::Owned(input.replace('\u{feff}', ""))
    } else {
        Cow::Borrowed(input)
    }
}

fn allowed_tags() -> HashSet<&'static str> {
    HashSet::from([
        "a",
        "abbr",
        "annotation",
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
        "math",
        "mark",
        "mfrac",
        "mi",
        "mn",
        "mo",
        "mrow",
        "ms",
        "mspace",
        "msqrt",
        "msub",
        "msup",
        "mtable",
        "mtd",
        "mtext",
        "mtr",
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
        "semantics",
        "small",
        "span",
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
        ("annotation", HashSet::from(["encoding"])),
        (
            "img",
            HashSet::from(["alt", "height", "src", "srcset", "width"]),
        ),
        ("math", HashSet::from(["xmlns"])),
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
    const ALLOWED_IFRAME_HOSTS: &[&str] = &[
        "youtube.com",
        "youtube-nocookie.com",
        "youtu.be",
        "player.vimeo.com",
        "vimeo.com",
        "peertube.tv",
        "soundcloud.com",
        "open.spotify.com",
        "podcasters.spotify.com",
        "anchor.fm",
        "simplecast.com",
        "player.fm",
    ];

    let Ok(url) = url::Url::parse(src) else {
        return false;
    };

    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(host);

    ALLOWED_IFRAME_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
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

fn render_math_spans(html: &str) -> String {
    let document = kuchiki::parse_html().one(html);
    let Ok(nodes) = document.select("span.math") else {
        return html.to_string();
    };
    let nodes = nodes.collect::<Vec<_>>();

    for node in nodes {
        let display = node
            .attributes
            .borrow()
            .get("class")
            .is_some_and(|class| class.split_whitespace().any(|class| class == "display"));
        let raw_tex = node.text_contents();
        let tex = strip_math_delimiters(&raw_tex, display);
        let Some(rendered) = render_katex_math(tex, display) else {
            continue;
        };
        let fragment = kuchiki::parse_html().one(rendered);
        let Ok(body) = fragment.select_first("body") else {
            continue;
        };
        let children = body.as_node().children().collect::<Vec<_>>();
        for child in children {
            node.as_node().insert_before(child);
        }
        node.as_node().detach();
    }

    serialize_body_children(&document)
}

fn strip_math_delimiters(value: &str, display: bool) -> &str {
    let value = value.trim();
    if display {
        return value
            .strip_prefix("$$")
            .and_then(|value| value.strip_suffix("$$"))
            .or_else(|| {
                value
                    .strip_prefix(r"\[")
                    .and_then(|value| value.strip_suffix(r"\]"))
            })
            .unwrap_or(value)
            .trim();
    }

    value
        .strip_prefix('$')
        .and_then(|value| value.strip_suffix('$'))
        .or_else(|| {
            value
                .strip_prefix(r"\(")
                .and_then(|value| value.strip_suffix(r"\)"))
        })
        .unwrap_or(value)
        .trim()
}

fn render_katex_math(tex: &str, display: bool) -> Option<String> {
    if tex.is_empty() {
        return None;
    }

    let opts = katex::Opts::builder()
        .display_mode(display)
        .output_type(katex::OutputType::Mathml)
        .throw_on_error(false)
        .build()
        .ok()?;
    katex::render_with_opts(tex, opts).ok()
}
