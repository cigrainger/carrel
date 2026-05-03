//! Link-density based roundup detection.

use kuchiki::NodeRef;

/// Outbound link density above this threshold is classified as a roundup.
///
/// 0.05 means one outbound link per twenty words. Below that, ordinary essays
/// with references stay in the neutral middle.
pub const LINK_ROUNDUP_DENSITY_THRESHOLD: f64 = 0.05;

pub(crate) fn detect(document: &NodeRef, word_count: usize) -> bool {
    if word_count == 0 {
        return false;
    }

    let link_count = outbound_link_count(document);
    (link_count as f64 / word_count as f64) > LINK_ROUNDUP_DENSITY_THRESHOLD
}

fn outbound_link_count(document: &NodeRef) -> usize {
    let Ok(links) = document.select("a[href]") else {
        return 0;
    };

    links
        .into_iter()
        .filter(|link| {
            link.attributes
                .borrow()
                .get("href")
                .is_some_and(counts_as_outbound)
        })
        .count()
}

fn counts_as_outbound(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("mailto:")
        || href.starts_with("blob:")
    {
        return false;
    }

    true
}
