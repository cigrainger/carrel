//! Entity ID derivation helpers.

use url::Url;

const TRACKING_PARAMS: &[&str] = &[
    "fbclid", "gclid", "igshid", "mc_cid", "mc_eid", "mkt_tok", "ref", "ref_src",
];

/// Derive a stable 64-character hex ID for externally identified content.
pub fn id_for_external(canonical_identifier: &str) -> String {
    hash_bytes(canonicalize_external_identifier(canonical_identifier).as_bytes())
}

/// Derive a stable 64-character hex ID for user-authored content.
pub fn id_for_authored(content: &[u8], author_pubkey: &[u8; 32], created_at_secs: i64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(content);
    hasher.update(author_pubkey);
    hasher.update(&created_at_secs.to_be_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Canonicalize an external identifier before hashing.
///
/// URL identifiers are normalized conservatively: tracker query parameters are
/// dropped, hosts are lowercased, and trailing slashes are stripped. Non-URL
/// identifiers are trimmed and otherwise left alone.
pub fn canonicalize_external_identifier(identifier: &str) -> String {
    let trimmed = identifier.trim();
    let Ok(mut url) = Url::parse(trimmed) else {
        return trimmed.to_string();
    };

    if let Some(host) = url.host_str().map(str::to_ascii_lowercase) {
        let _ = url.set_host(Some(&host));
    }

    let mut query_pairs = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking_param(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    query_pairs.sort();

    url.set_query(None);
    if !query_pairs.is_empty() {
        let query = query_pairs
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        url.set_query(Some(&query));
    }

    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);

    strip_url_trailing_slash(url.as_str())
}

fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn is_tracking_param(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.starts_with("utm_") || TRACKING_PARAMS.contains(&lower.as_str())
}

fn strip_url_trailing_slash(url: &str) -> String {
    let split_at = url.find(['?', '#']).unwrap_or(url.len());
    let (prefix, suffix) = url.split_at(split_at);
    format!("{}{}", prefix.trim_end_matches('/'), suffix)
}
