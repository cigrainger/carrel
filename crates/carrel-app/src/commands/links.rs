//! Deliberate article links open in the user's system browser.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::{AppError, Result};

#[tauri::command]
pub(crate) fn open_external_url(app: AppHandle, url: String) -> Result<()> {
    let url = web_url(&url)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(AppError::from)
}

fn web_url(value: &str) -> Result<Url> {
    Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or_else(|| AppError::InvalidData {
            context: "external link",
            value: "expected an HTTP or HTTPS URL".into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links_keep_their_query_and_fragment() {
        for url in [
            "http://example.com/article",
            "https://example.com/article?q=a%20b#notes",
        ] {
            assert_eq!(web_url(url).unwrap().as_str(), url);
        }
    }

    #[test]
    fn local_files_executable_schemes_and_malformed_links_cannot_be_opened() {
        for url in [
            "",
            "/tmp/article",
            "file:///tmp/article",
            "javascript:alert(1)",
            "data:text/html,example",
            "mailto:example@example.com",
            "carrel-blob://example",
            "//example.com",
        ] {
            assert!(web_url(url).is_err(), "{url}");
        }
    }
}
