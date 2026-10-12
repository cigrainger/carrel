//! Exact image references in the current archive; text never grants blob access.
use kuchiki::traits::TendrilSink;
use std::collections::BTreeSet;

/// Current cached image digests referenced by image attributes, including srcset.
/// Remote URLs, prose/code examples and older article bodies are excluded.
pub fn cached_image_ids(html: &str) -> Vec<String> {
    let document = kuchiki::parse_html().one(html);
    let mut ids = BTreeSet::new();
    if let Ok(images) = document.select("img, picture source, video[poster]") {
        for image in images {
            let attributes = image.attributes.borrow();
            if let Some(src) = attributes.get("src")
                && image.name.local.as_ref() == "img"
            {
                collect(src, &mut ids);
            }
            if let Some(poster) = attributes.get("poster") {
                collect(poster, &mut ids);
            }
            if let Some(srcset) = attributes.get("srcset") {
                for candidate in super::images::parse_srcset(srcset) {
                    collect(&candidate.url, &mut ids);
                }
            }
        }
    }
    ids.into_iter().collect()
}

/// Remove unapproved automatic resources from a scoped received archive.
/// Links remain deliberate navigation; cached-file links cannot escape the scope.
pub fn filter_cached_resources(html: &str, approved: &BTreeSet<String>) -> String {
    let document = kuchiki::parse_html().one(html);
    if let Ok(elements) = document.select("*") {
        for element in elements {
            let image =
                element.name.local.as_ref() == "img" || element.name.local.as_ref() == "source";
            let mut attributes = element.attributes.borrow_mut();
            for name in ["src", "poster", "data"] {
                if let Some(value) = attributes.get(name) {
                    let allowed = (image || name == "poster")
                        && cached_id(value).is_some_and(|id| approved.contains(id));
                    if !allowed {
                        attributes.remove(name);
                    }
                }
            }
            if let Some(value) = attributes.get("srcset") {
                let candidates: Vec<_> = super::images::parse_srcset(value)
                    .into_iter()
                    .filter(|candidate| {
                        image && cached_id(&candidate.url).is_some_and(|id| approved.contains(id))
                    })
                    .map(|candidate| match candidate.descriptor {
                        Some(descriptor) => format!("{} {}", candidate.url, descriptor),
                        None => candidate.url,
                    })
                    .collect();
                if candidates.is_empty() {
                    attributes.remove("srcset");
                } else {
                    attributes.insert("srcset", candidates.join(", "));
                }
            }
            if attributes.get("href").is_some_and(|href| {
                cached_id(href).is_some()
                    || href.to_ascii_lowercase().starts_with("blob:")
                    || href.to_ascii_lowercase().starts_with("carrel-blob:")
            }) {
                attributes.remove("href");
            }
        }
    }
    let mut bytes = Vec::new();
    if let Ok(body) = document.select_first("body") {
        for node in body.as_node().children() {
            let _ = node.serialize(&mut bytes);
        }
    }
    String::from_utf8(bytes).unwrap_or_default()
}
fn cached_id(url: &str) -> Option<&str> {
    let (scheme, id) = url.split_once("://")?;
    if (scheme.eq_ignore_ascii_case("blob") || scheme.eq_ignore_ascii_case("carrel-blob"))
        && id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Some(id)
    } else {
        None
    }
}
fn collect(url: &str, ids: &mut BTreeSet<String>) {
    let Some(id) = url
        .strip_prefix("blob://")
        .or_else(|| url.strip_prefix("carrel-blob://"))
    else {
        return;
    };
    if id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        ids.insert(id.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_actual_current_image_attributes_authorize_resources() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let private = "c".repeat(64);
        let html = format!(
            r#"<p>blob://{private}</p><code>&lt;img src="blob://{private}"&gt;</code><a href="blob://{private}">Example</a><script src="blob://{private}"></script><picture><source srcset="blob://{a} 1x, carrel-blob://{b} 2x"><img src="blob://{b}"></picture><img src="https://example.org/tracker"><img src="blob://{private}?extra"><img src="blob://short">"#
        );
        assert_eq!(cached_image_ids(&html), [a, b]);
        assert!(cached_image_ids("").is_empty());
    }
    #[test]
    fn a_scoped_body_cannot_load_an_unrelated_cached_digest_or_remote_resource() {
        let approved = "a".repeat(64);
        let private = "b".repeat(64);
        let html = format!(
            r#"<p>Text and <a href="https://example.org">deliberate links</a>.</p><img alt="Approved" src="blob://{approved}" srcset="blob://{private} 2x, carrel-blob://{approved} 1x"><img alt="Private" src="CARREL-BLOB://{private}"><img src="http://carrel-blob.localhost/{private}"><img src="https://tracker.example/pixel"><video src="carrel-blob://{private}"></video><a href="blob://{private}">Cached file</a><math><mi>x</mi></math>"#
        );
        let filtered = filter_cached_resources(&html, &BTreeSet::from([approved.clone()]));
        assert!(filtered.contains(&format!("src=\"blob://{approved}\"")));
        assert!(filtered.contains(&format!("carrel-blob://{approved} 1x")));
        assert!(!filtered.contains(&private));
        assert!(!filtered.contains("tracker.example"));
        assert!(filtered.contains("https://example.org"));
        assert!(filtered.contains("<math>"));
        assert!(filtered.contains("alt=\"Private\""));
    }
}
