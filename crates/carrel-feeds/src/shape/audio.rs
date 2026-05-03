//! Embedded-audio detection.

use kuchiki::NodeRef;

const AUDIO_HOSTS: &[&str] = &[
    "soundcloud.com",
    "open.spotify.com",
    "podcasters.spotify.com",
    "anchor.fm",
    "simplecast.com",
    "player.fm",
];

pub(crate) fn detect(document: &NodeRef) -> bool {
    if document
        .select("audio")
        .is_ok_and(|mut matches| matches.next().is_some())
    {
        return true;
    }

    let Ok(iframes) = document.select("iframe[src]") else {
        return false;
    };

    iframes.into_iter().any(|iframe| {
        iframe
            .attributes
            .borrow()
            .get("src")
            .is_some_and(host_matches)
    })
}

fn host_matches(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    AUDIO_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
}
