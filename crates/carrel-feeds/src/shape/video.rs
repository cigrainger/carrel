//! Embedded-video detection.

use kuchiki::NodeRef;

const VIDEO_HOSTS: &[&str] = &[
    "youtube.com",
    "youtube-nocookie.com",
    "youtu.be",
    "player.vimeo.com",
    "vimeo.com",
    "peertube.tv",
];

const VIDEO_CLASS_MARKERS: &[&str] = &["youtube", "vimeo", "peertube", "video-embed", "oembed"];

pub(crate) fn detect(document: &NodeRef) -> bool {
    if has_selector(document, "video") {
        return true;
    }

    has_iframe_host(document, VIDEO_HOSTS) || has_oembed_marker(document, VIDEO_CLASS_MARKERS)
}

fn has_iframe_host(document: &NodeRef, hosts: &[&str]) -> bool {
    let Ok(iframes) = document.select("iframe[src]") else {
        return false;
    };

    iframes.into_iter().any(|iframe| {
        iframe
            .attributes
            .borrow()
            .get("src")
            .is_some_and(|src| host_matches(src, hosts))
    })
}

fn has_oembed_marker(document: &NodeRef, markers: &[&str]) -> bool {
    let Ok(nodes) = document.select("[class]") else {
        return false;
    };

    nodes.into_iter().any(|node| {
        node.attributes
            .borrow()
            .get("class")
            .is_some_and(|classes| contains_marker(classes, markers))
    })
}

fn has_selector(document: &NodeRef, selector: &str) -> bool {
    document
        .select(selector)
        .is_ok_and(|mut matches| matches.next().is_some())
}

fn host_matches(raw: &str, hosts: &[&str]) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    hosts
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
}

fn contains_marker(classes: &str, markers: &[&str]) -> bool {
    let classes = classes.to_ascii_lowercase();
    markers.iter().any(|marker| classes.contains(marker))
}
